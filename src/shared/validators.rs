use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::shared::types::ZoteroCreator;

/// Represents inputs that can be either a single string or array of strings.
/// Used for tool parameters that accept `string | string[]` in TypeScript.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrList {
    Single(String),
    List(Vec<String>),
}

impl StringOrList {
    pub fn into_vec(self) -> Vec<String> {
        match self {
            StringOrList::Single(s) => {
                if s.trim().is_empty() {
                    vec![]
                } else {
                    let trimmed = s.trim();
                    if trimmed.starts_with('[') && trimmed.ends_with(']') {
                        if let Ok(parsed) = serde_json::from_str::<Vec<String>>(trimmed) {
                            return parsed
                                .into_iter()
                                .map(|v| v.trim().to_string())
                                .filter(|v| !v.is_empty())
                                .collect();
                        }
                    }

                    s.split(',')
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty())
                        .collect()
                }
            }
            StringOrList::List(v) => v
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }
}

/// Parse a StringOrList input (or None) into a Vec<String>.
/// Handles: None → [], single string (comma-split), array.
pub fn parse_str_list(value: Option<StringOrList>) -> Vec<String> {
    match value {
        None => vec![],
        Some(v) => v.into_vec(),
    }
}

/// Normalize a numeric limit: clamp to [1, max_val], return default_val if invalid.
pub fn normalize_limit(value: Option<i64>, default_val: i64, max_val: i64) -> i64 {
    match value {
        None => default_val,
        Some(v) if v <= 0 => default_val,
        Some(v) => v.min(max_val).max(1),
    }
}

/// Check if a string is a valid Zotero collection key (8 uppercase alphanumeric chars).
pub fn is_collection_key(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() == 8
        && trimmed
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Deduplicate strings, case-insensitively, preserving original case and order.
pub fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    for value in values {
        let trimmed = value.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        let lower = trimmed.to_lowercase();
        if seen.insert(lower) {
            out.push(trimmed);
        }
    }

    out
}

/// Parse creator name strings into ZoteroCreator structs.
/// Supports: "Last, First", "First Last", "Single Name"
pub fn parse_creator_names(values: Vec<String>) -> Vec<ZoteroCreator> {
    let mut creators = Vec::new();

    for value in values {
        let v = value.trim().to_string();
        if v.is_empty() {
            continue;
        }

        if v.contains(',') {
            let parts: Vec<&str> = v.splitn(2, ',').collect();
            let last = parts[0].trim().to_string();
            let first = parts
                .get(1)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();

            creators.push(ZoteroCreator {
                creator_type: "author".to_string(),
                first_name: Some(first),
                last_name: Some(last),
                name: None,
            });
        } else {
            let parts: Vec<&str> = v.split_whitespace().collect();
            if parts.len() >= 2 {
                let last = parts[parts.len() - 1].to_string();
                let first = parts[..parts.len() - 1].join(" ");
                creators.push(ZoteroCreator {
                    creator_type: "author".to_string(),
                    first_name: Some(first),
                    last_name: Some(last),
                    name: None,
                });
            } else {
                creators.push(ZoteroCreator {
                    creator_type: "author".to_string(),
                    first_name: None,
                    last_name: None,
                    name: Some(v),
                });
            }
        }
    }

    creators
}

/// Result of a Zotero write response.
#[derive(Debug)]
pub struct WriteStatus {
    pub ok: bool,
    pub message: String,
    pub data: Option<Value>,
}

/// Parse a Zotero write API response (from WriteResponse struct) into WriteStatus.
pub fn handle_write_response(response: &Value) -> WriteStatus {
    let obj = match response.as_object() {
        Some(o) => o,
        None => {
            return WriteStatus {
                ok: false,
                message: "Empty write response".to_string(),
                data: None,
            }
        }
    };

    if let Some(failed) = obj.get("failed").and_then(|v| v.as_object()) {
        if !failed.is_empty() {
            let reason: Vec<String> = failed
                .iter()
                .filter_map(|(idx, err)| {
                    err.get("message")
                        .and_then(|m| m.as_str())
                        .map(|msg| format!("{}: {}", idx, msg))
                })
                .collect();

            return WriteStatus {
                ok: false,
                message: format!("Write failed - {}", reason.join("; ")),
                data: Some(response.clone()),
            };
        }
    }

    let success_count = [
        obj.get("successful")
            .and_then(|v| v.as_object())
            .map(|o| o.len())
            .unwrap_or(0),
        obj.get("success")
            .and_then(|v| v.as_object())
            .map(|o| o.len())
            .unwrap_or(0),
        obj.get("unchanged")
            .and_then(|v| v.as_object())
            .map(|o| o.len())
            .unwrap_or(0),
    ]
    .iter()
    .sum::<usize>();

    if success_count == 0 {
        return WriteStatus {
            ok: false,
            message: "Write response contained no successful items".to_string(),
            data: Some(response.clone()),
        };
    }

    WriteStatus {
        ok: true,
        message: format!("Write succeeded for {} item(s)", success_count),
        data: Some(response.clone()),
    }
}

/// Extract the first created key from a Zotero write response.
pub fn extract_created_key(data: &Value) -> Option<String> {
    if let Some(success) = data.get("success").and_then(|v| v.as_object()) {
        for v in success.values() {
            if let Some(s) = v.as_str() {
                return Some(s.to_string());
            }
        }
    }

    if let Some(successful) = data.get("successful").and_then(|v| v.as_object()) {
        for v in successful.values() {
            if let Some(key) = v.get("key").and_then(|k| k.as_str()) {
                return Some(key.to_string());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_str_list_none() {
        assert_eq!(parse_str_list(None), Vec::<String>::new());
    }

    #[test]
    fn test_parse_str_list_single_string() {
        let input = Some(StringOrList::Single("tag1".to_string()));
        assert_eq!(parse_str_list(input), vec!["tag1"]);
    }

    #[test]
    fn test_parse_str_list_comma_separated() {
        let input = Some(StringOrList::Single("tag1, tag2, tag3".to_string()));
        assert_eq!(parse_str_list(input), vec!["tag1", "tag2", "tag3"]);
    }

    #[test]
    fn test_parse_str_list_array() {
        let input = Some(StringOrList::List(vec![
            "a".to_string(),
            "b".to_string(),
            "  c  ".to_string(),
        ]));
        assert_eq!(parse_str_list(input), vec!["a", "b", "c"]);
    }

    #[test]
    fn test_parse_str_list_empty_string() {
        let input = Some(StringOrList::Single("".to_string()));
        assert_eq!(parse_str_list(input), Vec::<String>::new());
    }

    #[test]
    fn test_normalize_limit_defaults() {
        assert_eq!(normalize_limit(None, 10, 200), 10);
        assert_eq!(normalize_limit(Some(0), 10, 200), 10);
        assert_eq!(normalize_limit(Some(-5), 10, 200), 10);
    }

    #[test]
    fn test_normalize_limit_clamp() {
        assert_eq!(normalize_limit(Some(300), 10, 200), 200);
        assert_eq!(normalize_limit(Some(50), 10, 200), 50);
    }

    #[test]
    fn test_is_collection_key_valid() {
        assert!(is_collection_key("ABCD1234"));
        assert!(is_collection_key("12345678"));
        assert!(is_collection_key("ZZZZZZZZ"));
    }

    #[test]
    fn test_is_collection_key_invalid() {
        assert!(!is_collection_key("abcd1234"));
        assert!(!is_collection_key("ABCD123"));
        assert!(!is_collection_key("ABCD12345"));
        assert!(!is_collection_key("ABCD123!"));
    }

    #[test]
    fn test_dedupe_strings() {
        let input = vec![
            "hello".to_string(),
            "HELLO".to_string(),
            "world".to_string(),
            "hello".to_string(),
        ];
        let result = dedupe_strings(input);
        assert_eq!(result, vec!["hello", "world"]);
    }

    #[test]
    fn test_dedupe_empty_removed() {
        let input = vec![
            "a".to_string(),
            "".to_string(),
            "  ".to_string(),
            "b".to_string(),
        ];
        let result = dedupe_strings(input);
        assert_eq!(result, vec!["a", "b"]);
    }

    #[test]
    fn test_parse_creator_last_first() {
        let creators = parse_creator_names(vec!["Smith, John".to_string()]);
        assert_eq!(creators.len(), 1);
        assert_eq!(creators[0].last_name.as_deref(), Some("Smith"));
        assert_eq!(creators[0].first_name.as_deref(), Some("John"));
    }

    #[test]
    fn test_parse_creator_first_last() {
        let creators = parse_creator_names(vec!["John Smith".to_string()]);
        assert_eq!(creators.len(), 1);
        assert_eq!(creators[0].last_name.as_deref(), Some("Smith"));
        assert_eq!(creators[0].first_name.as_deref(), Some("John"));
    }

    #[test]
    fn test_parse_creator_single_name() {
        let creators = parse_creator_names(vec!["Plato".to_string()]);
        assert_eq!(creators[0].name.as_deref(), Some("Plato"));
        assert_eq!(creators[0].first_name, None);
        assert_eq!(creators[0].last_name, None);
    }

    #[test]
    fn test_handle_write_response_success() {
        let resp = json!({"success": {"0": "NEWKEY1"}, "failed": {}});
        let status = handle_write_response(&resp);
        assert!(status.ok);
        assert!(status.message.contains("1 item"));
    }

    #[test]
    fn test_handle_write_response_failed() {
        let resp = json!({"failed": {"0": {"code": 400, "message": "Bad request"}}});
        let status = handle_write_response(&resp);
        assert!(!status.ok);
        assert!(status.message.contains("Bad request"));
    }

    #[test]
    fn test_extract_created_key() {
        let data = json!({"success": {"0": "ABCD1234"}});
        assert_eq!(extract_created_key(&data), Some("ABCD1234".to_string()));
    }
}
