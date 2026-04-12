use crate::shared::types::{SearchCondition, ZoteroItem};
use std::cmp::Ordering;
use std::collections::HashSet;

// --- Tag clause parsing ---

#[derive(Debug, Clone, PartialEq)]
pub struct TagClause {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

/// Splits a raw tag clause string into include/exclude lists.
/// "tag1|tag2" are OR clauses (include), "-tag" marks exclusion.
pub fn parse_tag_clause(raw_clause: &str) -> TagClause {
    let parts: Vec<&str> = raw_clause
        .split('|')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    let effective = if parts.is_empty() {
        vec![raw_clause.trim()]
    } else {
        parts
    };

    let mut include = Vec::new();
    let mut exclude = Vec::new();

    for part in effective {
        if part.is_empty() {
            continue;
        }
        if let Some(rest) = part.strip_prefix('-') {
            let v = rest.trim().to_lowercase();
            if !v.is_empty() {
                exclude.push(v);
            }
        } else {
            include.push(part.to_lowercase());
        }
    }

    TagClause { include, exclude }
}

/// Item must match ALL clauses (AND across clauses).
/// Within a clause, includes are OR'd and all excludes must be absent.
pub fn matches_tag_clauses(item: &ZoteroItem, clauses: &[TagClause]) -> bool {
    let tags: HashSet<String> = item
        .data
        .tags
        .as_ref()
        .map(|ts| ts.iter().map(|t| t.tag.to_lowercase()).collect())
        .unwrap_or_default();

    clauses.iter().all(|clause| {
        let include_pass =
            clause.include.is_empty() || clause.include.iter().any(|c| tags.contains(c));
        let exclude_pass = clause.exclude.iter().all(|c| !tags.contains(c));
        include_pass && exclude_pass
    })
}

// --- Citation key ---

/// Extracts "citation key: xxx" from the extra field text.
pub fn read_citation_key_from_extra(extra: &str) -> Option<String> {
    for line in extra.lines() {
        let line = line.trim();
        if let Some(colon_pos) = line.find(':') {
            let key_part = line[..colon_pos].trim();
            let normalized: String = key_part
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            if normalized == "citation key" {
                let value = line[colon_pos + 1..].trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

// --- Field normalization and extraction ---

fn canonical_field(field: &str) -> String {
    let n = field.trim().to_lowercase();
    match n.as_str() {
        "creator" => "creators".to_string(),
        "tag" => "tags".to_string(),
        "year" => "date".to_string(),
        _ => n,
    }
}

fn opt_to_vec(opt: &Option<String>) -> Vec<String> {
    match opt {
        Some(s) if !s.trim().is_empty() => vec![s.clone()],
        _ => vec![],
    }
}

/// Extract string values from a ZoteroItem for a given field name.
pub fn extract_values(item: &ZoteroItem, field: &str) -> Vec<String> {
    let key = canonical_field(field);
    let data = &item.data;

    match key.as_str() {
        "creators" => data
            .creators
            .as_ref()
            .map(|creators| {
                creators
                    .iter()
                    .filter_map(|c| {
                        if let Some(name) = &c.name {
                            if !name.is_empty() {
                                return Some(name.clone());
                            }
                        }
                        let parts: Vec<&str> = [c.first_name.as_deref(), c.last_name.as_deref()]
                            .into_iter()
                            .flatten()
                            .filter(|s| !s.is_empty())
                            .collect();
                        if parts.is_empty() {
                            None
                        } else {
                            Some(parts.join(" "))
                        }
                    })
                    .collect()
            })
            .unwrap_or_default(),

        "tags" => data
            .tags
            .as_ref()
            .map(|tags| {
                tags.iter()
                    .map(|t| t.tag.clone())
                    .filter(|s| !s.trim().is_empty())
                    .collect()
            })
            .unwrap_or_default(),

        "date" => opt_to_vec(&data.date),
        "abstract" | "abstractnote" => opt_to_vec(&data.abstract_note),
        "title" => opt_to_vec(&data.title),
        "itemtype" => {
            if data.item_type.trim().is_empty() {
                vec![]
            } else {
                vec![data.item_type.clone()]
            }
        }
        "key" => opt_to_vec(&data.key),
        "version" => data
            .version
            .map(|n| vec![n.to_string()])
            .unwrap_or_default(),
        "dateadded" => opt_to_vec(&data.date_added),
        "datemodified" => opt_to_vec(&data.date_modified),
        "publicationtitle" => opt_to_vec(&data.publication_title),
        "volume" => opt_to_vec(&data.volume),
        "issue" => opt_to_vec(&data.issue),
        "pages" => opt_to_vec(&data.pages),
        "doi" => opt_to_vec(&data.doi),
        "url" => opt_to_vec(&data.url),
        "publisher" => opt_to_vec(&data.publisher),
        "place" => opt_to_vec(&data.place),
        "issn" => opt_to_vec(&data.issn),
        "extra" => opt_to_vec(&data.extra),
        "note" => opt_to_vec(&data.note),
        "contenttype" => opt_to_vec(&data.content_type),
        "filename" => opt_to_vec(&data.filename),
        "md5" => opt_to_vec(&data.md5),
        "mtime" => data.mtime.map(|n| vec![n.to_string()]).unwrap_or_default(),
        "parentitem" => opt_to_vec(&data.parent_item),
        "annotationtype" => opt_to_vec(&data.annotation_type),
        "annotationtext" => opt_to_vec(&data.annotation_text),
        "annotationcomment" => opt_to_vec(&data.annotation_comment),
        "annotationcolor" => opt_to_vec(&data.annotation_color),
        "annotationpagelabel" => opt_to_vec(&data.annotation_page_label),
        "annotationposition" => opt_to_vec(&data.annotation_position),
        "linkmode" => opt_to_vec(&data.link_mode),
        "collections" => data.collections.as_ref().cloned().unwrap_or_default(),
        _ => {
            // Fallback: try extra_fields with original name, then canonical
            for try_key in [field, key.as_str()] {
                if let Some(v) = data.extra_fields.get(try_key) {
                    if let Some(s) = v.as_str() {
                        if !s.trim().is_empty() {
                            return vec![s.to_string()];
                        }
                    }
                    if let Some(n) = v.as_f64() {
                        return vec![n.to_string()];
                    }
                }
            }
            vec![]
        }
    }
}

// --- Date helpers ---

/// Simple date parser for common Zotero formats.
/// Handles YYYY-MM-DD, YYYY-MM, YYYY (and ISO with trailing time).
/// Returns milliseconds since Unix epoch, or None.
fn parse_date_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Strip time portion after 'T' or first space
    let date_part = s
        .split('T')
        .next()
        .unwrap_or(s)
        .split(' ')
        .next()
        .unwrap_or(s);

    let parts: Vec<&str> = date_part.split('-').collect();
    match parts.len() {
        3 => {
            let year: i32 = parts[0].parse().ok()?;
            let month: u32 = parts[1].parse().ok()?;
            let day: u32 = parts[2].parse().ok()?;
            if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
                return None;
            }
            Some(date_to_epoch_ms(year, month, day))
        }
        2 => {
            let year: i32 = parts[0].parse().ok()?;
            let month: u32 = parts[1].parse().ok()?;
            if !(1..=12).contains(&month) {
                return None;
            }
            Some(date_to_epoch_ms(year, month, 1))
        }
        1 => {
            let year: i32 = parts[0].parse().ok()?;
            if !(1..=9999).contains(&year) {
                return None;
            }
            Some(date_to_epoch_ms(year, 1, 1))
        }
        _ => None,
    }
}

/// Converts a calendar date to milliseconds since Unix epoch
/// using the Julian Day Number formula.
fn date_to_epoch_ms(year: i32, month: u32, day: u32) -> i64 {
    let y = year as i64;
    let m = month as i64;
    let d = day as i64;
    let a = (14 - m) / 12;
    let y_adj = y + 4800 - a;
    let m_adj = m + 12 * a - 3;
    let jdn =
        d + (153 * m_adj + 2) / 5 + 365 * y_adj + y_adj / 4 - y_adj / 100 + y_adj / 400 - 32045;
    (jdn - 2_440_588) * 86_400_000
}

/// Extracts a 4-digit year (1900–2099) from a string.
fn extract_year(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    if bytes.len() < 4 {
        return None;
    }
    for i in 0..=bytes.len() - 4 {
        if bytes[i].is_ascii_digit()
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3].is_ascii_digit()
        {
            if let Ok(y) = s[i..i + 4].parse::<i64>() {
                if (1900..=2099).contains(&y) {
                    return Some(y);
                }
            }
        }
    }
    None
}

// --- Condition evaluation ---

/// Evaluate one SearchCondition against a ZoteroItem.
///
/// Supports: is, isNot, contains, doesNotContain, beginsWith, endsWith,
/// isGreaterThan, isLessThan, isBefore, isAfter.
pub fn evaluate_condition(item: &ZoteroItem, condition: &SearchCondition) -> bool {
    let op = condition.operation.as_str();
    let values = extract_values(item, &condition.field);

    // Empty-field handling
    if values.is_empty() {
        return match op {
            "isNot" => !condition.value.trim().is_empty(),
            "doesNotContain" => true,
            _ => false,
        };
    }

    // Numeric comparisons
    if op == "isGreaterThan" || op == "isLessThan" {
        let target: f64 = match condition.value.parse() {
            Ok(n) => n,
            Err(_) => return false,
        };
        let nums: Vec<f64> = values
            .iter()
            .filter_map(|v| v.parse::<f64>().ok())
            .collect();
        if nums.is_empty() {
            // For date-like fields, try year extraction as fallback
            let canon = canonical_field(&condition.field);
            if canon == "date" {
                let years: Vec<i64> = values.iter().filter_map(|v| extract_year(v)).collect();
                if years.is_empty() {
                    return false;
                }
                return if op == "isGreaterThan" {
                    years.iter().any(|&y| (y as f64) > target)
                } else {
                    years.iter().any(|&y| (y as f64) < target)
                };
            }
            return false;
        }
        return if op == "isGreaterThan" {
            nums.iter().any(|&n| n > target)
        } else {
            nums.iter().any(|&n| n < target)
        };
    }

    // Date comparisons
    if op == "isBefore" || op == "isAfter" {
        let target_ms = match parse_date_ms(&condition.value) {
            Some(ms) => ms,
            None => return false,
        };
        let dates: Vec<i64> = values.iter().filter_map(|v| parse_date_ms(v)).collect();
        if dates.is_empty() {
            return false;
        }
        return if op == "isBefore" {
            dates.iter().any(|&d| d < target_ms)
        } else {
            dates.iter().any(|&d| d > target_ms)
        };
    }

    // String comparisons (case-insensitive)
    let target = condition.value.to_lowercase();
    match op {
        "is" => values.iter().any(|v| v.to_lowercase() == target),
        "isNot" => values.iter().all(|v| v.to_lowercase() != target),
        "contains" => values.iter().any(|v| v.to_lowercase().contains(&target)),
        "doesNotContain" => values.iter().all(|v| !v.to_lowercase().contains(&target)),
        "beginsWith" => values.iter().any(|v| v.to_lowercase().starts_with(&target)),
        "endsWith" => values.iter().any(|v| v.to_lowercase().ends_with(&target)),
        _ => false,
    }
}

// --- Sorting helpers ---

#[derive(Debug, Clone, PartialEq)]
pub enum ComparableValue {
    Number(f64),
    Date(i64),
    Text(String),
}

/// Extract the first comparable value from an item for a given field (for sorting).
pub fn first_comparable_value(item: &ZoteroItem, field: &str) -> ComparableValue {
    let values = extract_values(item, field);
    if values.is_empty() {
        return ComparableValue::Text(String::new());
    }
    let first = &values[0];
    if let Ok(n) = first.parse::<f64>() {
        return ComparableValue::Number(n);
    }
    if let Some(ms) = parse_date_ms(first) {
        return ComparableValue::Date(ms);
    }
    ComparableValue::Text(first.to_lowercase())
}

/// Compare two ComparableValues for sorting.
/// Kind ordering: Number < Date < Text.
pub fn compare_comparable(a: &ComparableValue, b: &ComparableValue) -> Ordering {
    match (a, b) {
        (ComparableValue::Text(x), ComparableValue::Text(y)) => x.cmp(y),
        (ComparableValue::Number(x), ComparableValue::Number(y)) => {
            x.partial_cmp(y).unwrap_or(Ordering::Equal)
        }
        (ComparableValue::Date(x), ComparableValue::Date(y)) => x.cmp(y),
        // Text always sorts last
        (ComparableValue::Text(_), _) => Ordering::Greater,
        (_, ComparableValue::Text(_)) => Ordering::Less,
        // Number before Date
        (ComparableValue::Number(_), ComparableValue::Date(_)) => Ordering::Less,
        (ComparableValue::Date(_), ComparableValue::Number(_)) => Ordering::Greater,
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::types::{
        SearchCondition, ZoteroCreator, ZoteroItem, ZoteroItemData, ZoteroTag,
    };

    fn make_item(title: &str, date: &str) -> ZoteroItem {
        ZoteroItem {
            key: "TEST0001".to_string(),
            data: ZoteroItemData {
                title: Some(title.to_string()),
                date: if date.is_empty() {
                    None
                } else {
                    Some(date.to_string())
                },
                item_type: "journalArticle".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn cond(field: &str, op: &str, value: &str) -> SearchCondition {
        SearchCondition {
            field: field.to_string(),
            operation: op.to_string(),
            value: value.to_string(),
        }
    }

    // --- Tag clause tests ---

    #[test]
    fn test_parse_tag_clause_simple() {
        let c = parse_tag_clause("machine learning");
        assert_eq!(c.include, vec!["machine learning"]);
        assert!(c.exclude.is_empty());
    }

    #[test]
    fn test_parse_tag_clause_or() {
        let c = parse_tag_clause("ai|ml|deep learning");
        assert_eq!(c.include, vec!["ai", "ml", "deep learning"]);
        assert!(c.exclude.is_empty());
    }

    #[test]
    fn test_parse_tag_clause_exclude() {
        let c = parse_tag_clause("-draft|published");
        assert_eq!(c.include, vec!["published"]);
        assert_eq!(c.exclude, vec!["draft"]);
    }

    #[test]
    fn test_matches_tag_clauses() {
        let mut item = make_item("Test", "2024");
        item.data.tags = Some(vec![
            ZoteroTag {
                tag: "AI".to_string(),
                tag_type: None,
            },
            ZoteroTag {
                tag: "draft".to_string(),
                tag_type: None,
            },
        ]);

        let c1 = parse_tag_clause("ai");
        assert!(matches_tag_clauses(&item, &[c1]));

        let c2 = parse_tag_clause("-draft");
        assert!(!matches_tag_clauses(&item, &[c2]));

        let c3 = parse_tag_clause("ml|ai");
        assert!(matches_tag_clauses(&item, &[c3]));

        let c4 = parse_tag_clause("nonexistent");
        assert!(!matches_tag_clauses(&item, &[c4]));
    }

    // --- Citation key ---

    #[test]
    fn test_citation_key_from_extra() {
        assert_eq!(
            read_citation_key_from_extra("citation key: doe2024"),
            Some("doe2024".to_string())
        );
        assert_eq!(
            read_citation_key_from_extra("Some line\nCitation Key: smith2023\nAnother line"),
            Some("smith2023".to_string())
        );
        assert_eq!(read_citation_key_from_extra("no key here"), None);
        assert_eq!(read_citation_key_from_extra(""), None);
    }

    // --- evaluate_condition string ops ---

    #[test]
    fn test_evaluate_is() {
        let item = make_item("Deep Learning", "2024");
        assert!(evaluate_condition(
            &item,
            &cond("title", "is", "Deep Learning")
        ));
        assert!(evaluate_condition(
            &item,
            &cond("title", "is", "deep learning")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("title", "is", "Machine Learning")
        ));
    }

    #[test]
    fn test_evaluate_is_not() {
        let item = make_item("Deep Learning", "2024");
        assert!(!evaluate_condition(
            &item,
            &cond("title", "isNot", "Deep Learning")
        ));
        assert!(evaluate_condition(
            &item,
            &cond("title", "isNot", "Machine Learning")
        ));
    }

    #[test]
    fn test_evaluate_contains() {
        let item = make_item("Deep Learning for NLP", "2024");
        assert!(evaluate_condition(
            &item,
            &cond("title", "contains", "Learning")
        ));
        assert!(evaluate_condition(
            &item,
            &cond("title", "contains", "deep")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("title", "contains", "Vision")
        ));
    }

    #[test]
    fn test_evaluate_does_not_contain() {
        let item = make_item("Deep Learning", "2024");
        assert!(evaluate_condition(
            &item,
            &cond("title", "doesNotContain", "Vision")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("title", "doesNotContain", "deep")
        ));
    }

    #[test]
    fn test_evaluate_begins_with() {
        let item = make_item("Deep Learning", "2024");
        assert!(evaluate_condition(
            &item,
            &cond("title", "beginsWith", "Deep")
        ));
        assert!(evaluate_condition(
            &item,
            &cond("title", "beginsWith", "deep")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("title", "beginsWith", "Learning")
        ));
    }

    #[test]
    fn test_evaluate_ends_with() {
        let item = make_item("Deep Learning", "2024");
        assert!(evaluate_condition(
            &item,
            &cond("title", "endsWith", "Learning")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("title", "endsWith", "Deep")
        ));
    }

    // --- Numeric / date comparisons ---

    #[test]
    fn test_evaluate_is_greater_than_year() {
        let item = make_item("Test", "2024");
        assert!(evaluate_condition(
            &item,
            &cond("date", "isGreaterThan", "2020")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("date", "isGreaterThan", "2025")
        ));
    }

    #[test]
    fn test_evaluate_is_less_than() {
        let item = make_item("Test", "2024");
        assert!(evaluate_condition(
            &item,
            &cond("date", "isLessThan", "2025")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("date", "isLessThan", "2020")
        ));
    }

    #[test]
    fn test_evaluate_is_before_date() {
        let item = make_item("Test", "2024-06-15");
        assert!(evaluate_condition(
            &item,
            &cond("date", "isBefore", "2025-01-01")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("date", "isBefore", "2024-01-01")
        ));
    }

    #[test]
    fn test_evaluate_is_after_date() {
        let item = make_item("Test", "2024-06-15");
        assert!(evaluate_condition(
            &item,
            &cond("date", "isAfter", "2024-01-01")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("date", "isAfter", "2025-01-01")
        ));
    }

    // --- Empty field behavior ---

    #[test]
    fn test_evaluate_empty_field_is_not() {
        let item = make_item("Test", "");
        assert!(evaluate_condition(&item, &cond("date", "isNot", "2024")));
        assert!(evaluate_condition(
            &item,
            &cond("date", "doesNotContain", "anything")
        ));
        assert!(!evaluate_condition(&item, &cond("date", "is", "2024")));
    }

    // --- Creators ---

    #[test]
    fn test_evaluate_creators() {
        let mut item = make_item("Test", "2024");
        item.data.creators = Some(vec![
            ZoteroCreator {
                creator_type: "author".to_string(),
                first_name: Some("John".to_string()),
                last_name: Some("Doe".to_string()),
                name: None,
            },
            ZoteroCreator {
                creator_type: "author".to_string(),
                first_name: None,
                last_name: None,
                name: Some("Jane Smith".to_string()),
            },
        ]);
        assert!(evaluate_condition(
            &item,
            &cond("creators", "contains", "doe")
        ));
        assert!(evaluate_condition(
            &item,
            &cond("creators", "is", "Jane Smith")
        ));
        assert!(!evaluate_condition(
            &item,
            &cond("creators", "is", "Unknown")
        ));
    }

    // --- Sorting / comparable ---

    #[test]
    fn test_first_comparable_value() {
        let item = make_item("Test", "2024-06-15");
        match first_comparable_value(&item, "title") {
            ComparableValue::Text(s) => assert_eq!(s, "test"),
            other => panic!("Expected Text, got {:?}", other),
        }
        match first_comparable_value(&item, "date") {
            ComparableValue::Date(_) => {}
            other => panic!("Expected Date, got {:?}", other),
        }
    }

    #[test]
    fn test_first_comparable_numeric() {
        let mut item = make_item("Test", "");
        item.data.volume = Some("42".to_string());
        match first_comparable_value(&item, "volume") {
            ComparableValue::Number(n) => assert!((n - 42.0).abs() < f64::EPSILON),
            other => panic!("Expected Number, got {:?}", other),
        }
    }

    #[test]
    fn test_compare_comparable() {
        assert_eq!(
            compare_comparable(&ComparableValue::Number(1.0), &ComparableValue::Number(2.0)),
            Ordering::Less
        );
        assert_eq!(
            compare_comparable(
                &ComparableValue::Text("a".into()),
                &ComparableValue::Text("b".into())
            ),
            Ordering::Less
        );
        assert_eq!(
            compare_comparable(
                &ComparableValue::Number(1.0),
                &ComparableValue::Text("a".into())
            ),
            Ordering::Less
        );
        assert_eq!(
            compare_comparable(
                &ComparableValue::Text("a".into()),
                &ComparableValue::Date(0)
            ),
            Ordering::Greater
        );
    }

    // --- Extra fields fallback ---

    #[test]
    fn test_extract_values_extra_fields() {
        let mut item = make_item("Test", "");
        item.data.extra_fields.insert(
            "customField".to_string(),
            serde_json::Value::String("custom_value".to_string()),
        );
        let vals = extract_values(&item, "customField");
        assert_eq!(vals, vec!["custom_value"]);
    }

    // --- Year alias ---

    #[test]
    fn test_year_field_alias() {
        let item = make_item("Test", "March 2023");
        assert!(evaluate_condition(
            &item,
            &cond("year", "isGreaterThan", "2020")
        ));
    }
}
