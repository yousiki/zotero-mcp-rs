use std::sync::OnceLock;

use regex::Regex;

use crate::shared::types::{AttachmentDetails, ZoteroCreator, ZoteroItem};

// ---------------------------------------------------------------------------
// Regex helpers (compiled once)
// ---------------------------------------------------------------------------

fn br_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)<br\s*/?\s*>").unwrap())
}

fn close_p_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)</p>").unwrap())
}

fn tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"<[^>]+>").unwrap())
}

fn year_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(19|20)\d{2}").unwrap())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Truncate text to `max_len` chars, appending "..." if truncated.
pub fn truncate(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        return text.to_string();
    }
    let mut end = max_len;
    while !text.is_char_boundary(end) && end > 0 {
        end -= 1;
    }
    format!("{}...", text[..end].trim())
}

/// Escape HTML special chars: & < > " '
pub fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Format creator list as "Last, First; Last, First" string.
pub fn format_creators(creators: &[ZoteroCreator]) -> String {
    creators
        .iter()
        .filter_map(|c| {
            if let Some(name) = &c.name {
                let name = name.trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
            let last = c.last_name.as_deref().unwrap_or("").trim();
            let first = c.first_name.as_deref().unwrap_or("").trim();
            match (!last.is_empty(), !first.is_empty()) {
                (true, true) => Some(format!("{}, {}", last, first)),
                (true, false) => Some(last.to_string()),
                (false, true) => Some(first.to_string()),
                _ => None,
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Clean HTML: strip tags, decode entities, optionally collapse whitespace.
///
/// Converts `<br>` / `<br/>` → newline, `</p>` → newline, strips remaining
/// tags, then decodes `&amp;` `&lt;` `&gt;` `&quot;` `&#39;` `&nbsp;`.
pub fn clean_html(raw: &str, collapse_whitespace: bool) -> String {
    let result = br_regex().replace_all(raw, "\n");
    let result = close_p_regex().replace_all(&result, "\n");
    let result = tag_regex().replace_all(&result, "");

    let result = result
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    if !collapse_whitespace {
        return result.trim().to_string();
    }

    result
        .split('\n')
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Format a Zotero item as a Markdown summary line with optional tags.
pub fn format_item_result(item: &ZoteroItem, index: Option<usize>, show_tags: bool) -> String {
    let data = &item.data;
    let abstract_len = 180;
    let mut lines: Vec<String> = Vec::new();

    let prefix = match index {
        Some(i) => format!("{}. ", i),
        None => "- ".to_string(),
    };

    let title = data.title.as_deref().unwrap_or("(untitled)");
    lines.push(format!("{}**{}**", prefix, title));
    lines.push(format!("  - Key: {}", item.key));
    lines.push(format!("  - Type: {}", data.item_type));

    let creators_str = format_creators(data.creators.as_deref().unwrap_or(&[]));
    if !creators_str.is_empty() {
        lines.push(format!("  - Creators: {}", creators_str));
    }

    if let Some(date) = &data.date
        && !date.is_empty()
    {
        lines.push(format!("  - Date: {}", date));
    }

    if let Some(pub_title) = &data.publication_title
        && !pub_title.is_empty()
    {
        lines.push(format!("  - Publication: {}", pub_title));
    }

    if let Some(doi) = &data.doi
        && !doi.is_empty()
    {
        lines.push(format!("  - DOI: {}", doi));
    }

    if show_tags
        && let Some(tags) = &data.tags
        && !tags.is_empty()
    {
        let tag_str: String = tags
            .iter()
            .map(|t| t.tag.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("  - Tags: {}", tag_str));
    }

    if let Some(abstract_note) = &data.abstract_note
        && !abstract_note.is_empty()
    {
        let cleaned = clean_html(abstract_note, true);
        let shortened = truncate(&cleaned, abstract_len);
        if !shortened.is_empty() {
            lines.push(format!("  - Abstract: {}", shortened));
        }
    }

    lines.join("\n")
}

/// Format a Zotero item as full Markdown metadata.
pub fn format_item_metadata(item: &ZoteroItem, include_abstract: bool) -> String {
    let data = &item.data;
    let mut lines: Vec<String> = Vec::new();

    let title = data.title.as_deref().unwrap_or("(untitled)");
    lines.push(format!("# {}", title));
    lines.push(String::new());
    lines.push(format!("- **Key:** {}", item.key));
    lines.push(format!("- **Type:** {}", data.item_type));

    let creators_str = format_creators(data.creators.as_deref().unwrap_or(&[]));
    if !creators_str.is_empty() {
        lines.push(format!("- **Creators:** {}", creators_str));
    }
    if let Some(v) = &data.date
        && !v.is_empty()
    {
        lines.push(format!("- **Date:** {}", v));
    }
    if let Some(v) = &data.publication_title
        && !v.is_empty()
    {
        lines.push(format!("- **Publication:** {}", v));
    }
    if let Some(v) = &data.volume
        && !v.is_empty()
    {
        lines.push(format!("- **Volume:** {}", v));
    }
    if let Some(v) = &data.issue
        && !v.is_empty()
    {
        lines.push(format!("- **Issue:** {}", v));
    }
    if let Some(v) = &data.pages
        && !v.is_empty()
    {
        lines.push(format!("- **Pages:** {}", v));
    }
    if let Some(v) = &data.publisher
        && !v.is_empty()
    {
        lines.push(format!("- **Publisher:** {}", v));
    }
    if let Some(v) = &data.doi
        && !v.is_empty()
    {
        lines.push(format!("- **DOI:** {}", v));
    }
    if let Some(v) = &data.url
        && !v.is_empty()
    {
        lines.push(format!("- **URL:** {}", v));
    }
    if let Some(tags) = &data.tags
        && !tags.is_empty()
    {
        let tag_str: String = tags
            .iter()
            .map(|t| t.tag.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(format!("- **Tags:** {}", tag_str));
    }
    if let Some(cols) = &data.collections
        && !cols.is_empty()
    {
        lines.push(format!("- **Collections:** {}", cols.join(", ")));
    }
    if let Some(v) = &data.extra
        && !v.is_empty()
    {
        lines.push(format!("- **Extra:** {}", v));
    }

    if include_abstract
        && let Some(abstract_note) = &data.abstract_note
        && !abstract_note.is_empty()
    {
        lines.push(String::new());
        lines.push("## Abstract".to_string());
        lines.push(String::new());
        lines.push(clean_html(abstract_note, false));
    }

    lines.join("\n")
}

/// Generate a BibTeX entry for a Zotero item.
pub fn generate_bibtex(item: &ZoteroItem) -> String {
    let data = &item.data;
    let entry_type = map_item_type_to_bibtex(&data.item_type);
    let cite_key = build_fallback_cite_key(item);

    let fields: Vec<(&str, Option<String>)> = vec![
        ("title", data.title.clone()),
        (
            "author",
            format_creators_for_bibtex(data.creators.as_deref().unwrap_or(&[])),
        ),
        ("year", extract_year(data.date.as_deref())),
        ("journal", data.publication_title.clone()),
        ("volume", data.volume.clone()),
        ("number", data.issue.clone()),
        ("pages", data.pages.clone()),
        ("publisher", data.publisher.clone()),
        ("doi", data.doi.clone()),
        ("url", data.url.clone()),
    ];

    let mut lines = vec![format!("@{}{{{},", entry_type, cite_key)];
    for (field, value) in fields {
        if let Some(v) = value
            && !v.is_empty()
        {
            lines.push(format!("  {} = {{{}}},", field, escape_bibtex(&v)));
        }
    }
    lines.push("}".to_string());
    lines.join("\n")
}

/// Find the best attachment (PDF preferred) from a list of children.
pub fn find_best_attachment(children: &[ZoteroItem]) -> Option<AttachmentDetails> {
    let attachments: Vec<&ZoteroItem> = children
        .iter()
        .filter(|child| child.data.item_type == "attachment")
        .collect();

    if attachments.is_empty() {
        return None;
    }

    fn score(item: &ZoteroItem) -> i32 {
        let ct = item
            .data
            .content_type
            .as_deref()
            .unwrap_or("")
            .to_lowercase();
        if ct.contains("pdf") {
            3
        } else if ct.contains("html") {
            2
        } else {
            1
        }
    }

    let best = attachments.into_iter().max_by_key(|a| score(a))?;

    Some(AttachmentDetails {
        key: best.key.clone(),
        title: best
            .data
            .title
            .as_deref()
            .unwrap_or("Attachment")
            .to_string(),
        filename: best
            .data
            .filename
            .as_deref()
            .or(best.data.title.as_deref())
            .unwrap_or("")
            .to_string(),
        content_type: best
            .data
            .content_type
            .as_deref()
            .unwrap_or("application/octet-stream")
            .to_string(),
    })
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn map_item_type_to_bibtex(item_type: &str) -> &'static str {
    match item_type {
        "journalArticle" => "article",
        "conferencePaper" => "inproceedings",
        "book" => "book",
        "bookSection" => "incollection",
        "thesis" => "phdthesis",
        "report" => "techreport",
        "webpage" => "misc",
        "preprint" => "article",
        _ => "misc",
    }
}

fn format_creators_for_bibtex(creators: &[ZoteroCreator]) -> Option<String> {
    let names: Vec<String> = creators
        .iter()
        .filter_map(|c| {
            if let Some(name) = &c.name {
                let name = name.trim();
                if !name.is_empty() {
                    return Some(name.to_string());
                }
            }
            let first = c.first_name.as_deref().unwrap_or("").trim().to_string();
            let last = c.last_name.as_deref().unwrap_or("").trim().to_string();
            let parts: Vec<&str> = [first.as_str(), last.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
            if parts.is_empty() {
                None
            } else {
                Some(parts.join(" "))
            }
        })
        .collect();

    if names.is_empty() {
        None
    } else {
        Some(names.join(" and "))
    }
}

fn extract_year(date: Option<&str>) -> Option<String> {
    date.and_then(|d| year_regex().find(d).map(|m| m.as_str().to_string()))
}

fn build_fallback_cite_key(item: &ZoteroItem) -> String {
    let year = extract_year(item.data.date.as_deref()).unwrap_or_else(|| "n.d.".to_string());
    let last_name = item
        .data
        .creators
        .as_ref()
        .and_then(|creators| creators.first())
        .map(|c| {
            if let Some(last) = &c.last_name {
                last.clone()
            } else if let Some(name) = &c.name {
                name.split_whitespace().last().unwrap_or("item").to_string()
            } else {
                "item".to_string()
            }
        })
        .unwrap_or_else(|| "item".to_string());

    let clean: String = last_name
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect();
    format!("{}{}", clean, year)
}

fn escape_bibtex(value: &str) -> String {
    value.replace(['{', '}'], "")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::types::{ZoteroCreator, ZoteroItem, ZoteroItemData, ZoteroTag};

    fn make_item(f: impl FnOnce(&mut ZoteroItem)) -> ZoteroItem {
        let mut item = ZoteroItem {
            key: "ABCD1234".to_string(),
            data: ZoteroItemData {
                item_type: "journalArticle".to_string(),
                title: Some("Test Paper".to_string()),
                creators: Some(vec![ZoteroCreator {
                    creator_type: "author".to_string(),
                    first_name: Some("John".to_string()),
                    last_name: Some("Smith".to_string()),
                    name: None,
                }]),
                date: Some("2024".to_string()),
                publication_title: Some("Nature".to_string()),
                doi: Some("10.1038/xxx".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        f(&mut item);
        item
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello...");
        assert_eq!(truncate("", 5), "");
        assert_eq!(truncate("abc", 3), "abc");
        assert_eq!(truncate("abcd", 3), "abc...");
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(
            escape_html(r#"<b>test & "quotes"</b>"#),
            "&lt;b&gt;test &amp; &quot;quotes&quot;&lt;/b&gt;"
        );
        assert_eq!(escape_html("it's"), "it&#39;s");
    }

    #[test]
    fn test_clean_html_strips_tags() {
        assert_eq!(clean_html("<p>Hello <b>World</b></p>", true), "Hello World");
    }

    #[test]
    fn test_clean_html_decodes_entities() {
        assert_eq!(
            clean_html("&amp; &lt; &gt; &quot; &#39;", true),
            "& < > \" '"
        );
    }

    #[test]
    fn test_clean_html_converts_br_to_newline() {
        assert_eq!(
            clean_html("line1<br>line2<br/>line3", true),
            "line1\nline2\nline3"
        );
        // Also handles <BR /> with spaces
        assert_eq!(clean_html("a<BR />b", true), "a\nb");
    }

    #[test]
    fn test_format_creators_full_name() {
        let creators = vec![
            ZoteroCreator {
                creator_type: "author".to_string(),
                first_name: Some("John".to_string()),
                last_name: Some("Smith".to_string()),
                name: None,
            },
            ZoteroCreator {
                creator_type: "author".to_string(),
                first_name: Some("Jane".to_string()),
                last_name: Some("Jones".to_string()),
                name: None,
            },
        ];
        assert_eq!(format_creators(&creators), "Smith, John; Jones, Jane");
    }

    #[test]
    fn test_format_creators_single_name() {
        let creators = vec![ZoteroCreator {
            creator_type: "author".to_string(),
            first_name: None,
            last_name: None,
            name: Some("ACME Corp".to_string()),
        }];
        assert_eq!(format_creators(&creators), "ACME Corp");
    }

    #[test]
    fn test_format_item_result_basic() {
        let item = make_item(|item| {
            item.data.tags = Some(vec![
                ZoteroTag {
                    tag: "machine learning".to_string(),
                    tag_type: None,
                },
                ZoteroTag {
                    tag: "AI".to_string(),
                    tag_type: None,
                },
            ]);
        });
        let result = format_item_result(&item, Some(1), true);
        assert!(result.contains("1. **Test Paper**"));
        assert!(result.contains("  - Key: ABCD1234"));
        assert!(result.contains("  - Type: journalArticle"));
        assert!(result.contains("  - Creators: Smith, John"));
        assert!(result.contains("  - Date: 2024"));
        assert!(result.contains("  - Publication: Nature"));
        assert!(result.contains("  - DOI: 10.1038/xxx"));
        assert!(result.contains("  - Tags: machine learning, AI"));
    }

    #[test]
    fn test_generate_bibtex_journal_article() {
        let item = make_item(|_| {});
        let bib = generate_bibtex(&item);
        assert!(bib.starts_with("@article{Smith2024,"), "got: {}", bib);
        assert!(bib.contains("title = {Test Paper},"));
        assert!(bib.contains("author = {John Smith},"));
        assert!(bib.contains("year = {2024},"));
        assert!(bib.contains("journal = {Nature},"));
        assert!(bib.contains("doi = {10.1038/xxx},"));
    }

    #[test]
    fn test_generate_bibtex_book() {
        let item = make_item(|item| {
            item.data.item_type = "book".to_string();
            item.data.publisher = Some("Publisher Inc".to_string());
        });
        let bib = generate_bibtex(&item);
        assert!(bib.starts_with("@book{Smith2024,"), "got: {}", bib);
        assert!(bib.contains("publisher = {Publisher Inc},"));
    }
}
