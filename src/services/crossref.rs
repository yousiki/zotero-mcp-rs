#![allow(dead_code)]

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clients::webdav::WebDavClient;
use crate::clients::zotero::ZoteroClient;
use crate::shared::formatters::{clean_html, format_item_result};
use crate::shared::types::{ZoteroCreator, ZoteroItemData, ZoteroTag, crossref_type_map};
use crate::shared::validators::{extract_created_key, handle_write_response};

use super::oa_sources::try_attach_oa_pdf;

// ---------------------------------------------------------------------------
// URL encoding helper
// ---------------------------------------------------------------------------

/// Percent-encode a string for use in URI path segments (like `encodeURIComponent`).
fn encode_uri_component(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// CrossRef API types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrossrefWork {
    #[serde(rename = "type")]
    pub work_type: Option<String>,
    pub title: Option<Vec<String>>,
    pub author: Option<Vec<CrossrefPerson>>,
    pub editor: Option<Vec<CrossrefPerson>>,
    #[serde(rename = "DOI")]
    pub doi: Option<String>,
    #[serde(rename = "URL")]
    pub url: Option<Value>,
    pub volume: Option<String>,
    pub issue: Option<String>,
    pub page: Option<String>,
    pub publisher: Option<String>,
    #[serde(rename = "ISSN")]
    pub issn: Option<Vec<String>>,
    #[serde(rename = "abstract")]
    pub abstract_note: Option<String>,
    pub relation: Option<Value>,
    #[serde(rename = "container-title")]
    pub container_title: Option<Vec<String>>,
    pub published: Option<CrossrefDate>,
    #[serde(rename = "published-print")]
    pub published_print: Option<CrossrefDate>,
    #[serde(rename = "published-online")]
    pub published_online: Option<CrossrefDate>,
    pub issued: Option<CrossrefDate>,
    #[serde(flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrossrefPerson {
    pub given: Option<String>,
    pub family: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CrossrefDate {
    #[serde(rename = "date-parts")]
    pub date_parts: Option<Vec<Vec<Option<i64>>>>,
}

#[derive(Debug, Deserialize)]
struct CrossrefResponse {
    message: Option<CrossrefWork>,
}

// ---------------------------------------------------------------------------
// Public result type
// ---------------------------------------------------------------------------

pub struct AddCrossrefResult {
    pub key: String,
    pub result: String,
}

// ---------------------------------------------------------------------------
// Date parsing
// ---------------------------------------------------------------------------

/// Parse a date from a CrossRef work, trying published-print, published-online,
/// published, and issued in order. Returns "YYYY", "YYYY-MM", or "YYYY-MM-DD".
pub fn parse_crossref_date(work: &CrossrefWork) -> Option<String> {
    let candidates = [
        &work.published_print,
        &work.published_online,
        &work.published,
        &work.issued,
    ];

    for candidate in &candidates {
        let date = match candidate {
            Some(d) => d,
            None => continue,
        };

        let parts_outer = match &date.date_parts {
            Some(p) if !p.is_empty() => p,
            _ => continue,
        };

        let parts = &parts_outer[0];
        if parts.is_empty() {
            continue;
        }

        let year = match parts[0] {
            Some(y) => y,
            None => continue,
        };

        let yyyy = format!("{}", year);

        let month = parts.get(1).and_then(|v| *v);
        let month = match month {
            Some(m) => m,
            None => return Some(yyyy),
        };

        let mm = format!("{:02}", month);

        let day = parts.get(2).and_then(|v| *v);
        let day = match day {
            Some(d) => d,
            None => return Some(format!("{}-{}", yyyy, mm)),
        };

        let dd = format!("{:02}", day);
        return Some(format!("{}-{}-{}", yyyy, mm, dd));
    }

    None
}

// ---------------------------------------------------------------------------
// Creator mapping
// ---------------------------------------------------------------------------

/// Map CrossRef author/editor arrays into Zotero creator structs.
pub fn map_crossref_creators(work: &CrossrefWork) -> Vec<ZoteroCreator> {
    let mut out = Vec::new();

    fn push_people(
        people: &Option<Vec<CrossrefPerson>>,
        creator_type: &str,
        out: &mut Vec<ZoteroCreator>,
    ) {
        let people = match people {
            Some(p) => p,
            None => return,
        };
        for person in people {
            let given = person
                .given
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let family = person
                .family
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let name = person
                .name
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());

            if let Some(name) = name {
                out.push(ZoteroCreator {
                    creator_type: creator_type.to_string(),
                    first_name: None,
                    last_name: None,
                    name: Some(name.to_string()),
                });
            } else if family.is_some() || given.is_some() {
                out.push(ZoteroCreator {
                    creator_type: creator_type.to_string(),
                    first_name: given.map(|s| s.to_string()),
                    last_name: family.map(|s| s.to_string()),
                    name: None,
                });
            }
        }
    }

    push_people(&work.author, "author", &mut out);
    push_people(&work.editor, "editor", &mut out);
    out
}

// ---------------------------------------------------------------------------
// Main add-via-crossref function
// ---------------------------------------------------------------------------

/// Fetch metadata from CrossRef for a DOI, create a Zotero item, and attempt
/// to attach an Open Access PDF.
pub async fn add_via_crossref(
    client: &ZoteroClient,
    doi: &str,
    collection_keys: Vec<String>,
    tags: Vec<ZoteroTag>,
    webdav: &WebDavClient,
) -> anyhow::Result<AddCrossrefResult> {
    // 1. Fetch from CrossRef
    let url = format!(
        "https://api.crossref.org/works/{}",
        encode_uri_component(doi)
    );
    let http = reqwest::Client::new();
    let resp = http
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("CrossRef lookup failed ({})", resp.status());
    }

    let payload: CrossrefResponse = resp.json().await?;
    let work = payload
        .message
        .ok_or_else(|| anyhow::anyhow!("CrossRef returned no metadata"))?;

    // 2. Map type
    let type_map = crossref_type_map();
    let work_type_str = work.work_type.as_deref().unwrap_or("");
    let mapped_type = type_map
        .get(work_type_str)
        .copied()
        .unwrap_or("journalArticle");

    // 3. Get template and build item data
    let template = client.get_item_template(mapped_type, None).await?;
    let container_title = work
        .container_title
        .as_ref()
        .and_then(|ct| ct.first())
        .cloned();

    let url_value = match &work.url {
        Some(Value::String(s)) => s.clone(),
        _ => format!("https://doi.org/{}", doi),
    };

    let abstract_text = work
        .abstract_note
        .as_deref()
        .map(|s| clean_html(s, true))
        .unwrap_or_default();

    let mut item_data = ZoteroItemData {
        item_type: mapped_type.to_string(),
        title: Some(
            work.title
                .as_ref()
                .and_then(|t| t.first())
                .cloned()
                .unwrap_or_default(),
        ),
        creators: Some(map_crossref_creators(&work)),
        date: parse_crossref_date(&work),
        doi: Some(doi.to_string()),
        url: Some(url_value),
        abstract_note: Some(abstract_text),
        tags: Some(tags),
        collections: Some(collection_keys),
        ..Default::default()
    };

    // Set optional fields only if valid for this item type (guarded by template)
    if (template.volume.is_some() || template.extra_fields.contains_key("volume"))
        && let Some(v) = &work.volume
    {
        item_data.volume = Some(v.clone());
    }
    if (template.issue.is_some() || template.extra_fields.contains_key("issue"))
        && let Some(v) = &work.issue
    {
        item_data.issue = Some(v.clone());
    }
    if (template.pages.is_some() || template.extra_fields.contains_key("pages"))
        && let Some(v) = &work.page
    {
        item_data.pages = Some(v.clone());
    }
    if (template.publisher.is_some() || template.extra_fields.contains_key("publisher"))
        && let Some(v) = &work.publisher
    {
        item_data.publisher = Some(v.clone());
    }
    if (template.issn.is_some() || template.extra_fields.contains_key("ISSN"))
        && let Some(issns) = &work.issn
        && let Some(first) = issns.first()
    {
        item_data.issn = Some(first.clone());
    }

    // Map container-title to the correct field based on item type
    if let Some(ct) = &container_title {
        if template.extra_fields.contains_key("proceedingsTitle") {
            item_data
                .extra_fields
                .insert("proceedingsTitle".to_string(), Value::String(ct.clone()));
        } else if template.extra_fields.contains_key("bookTitle") {
            item_data
                .extra_fields
                .insert("bookTitle".to_string(), Value::String(ct.clone()));
        } else {
            item_data.publication_title = Some(ct.clone());
        }
    }

    // 4. Create item
    let create_resp = client.create_items(&[item_data]).await?;
    let resp_value = serde_json::to_value(&create_resp)?;
    let write_status = handle_write_response(&resp_value);
    if !write_status.ok {
        anyhow::bail!("{}", write_status.message);
    }
    let data = write_status
        .data
        .ok_or_else(|| anyhow::anyhow!("No data in write response"))?;
    let created_key = extract_created_key(&data)
        .ok_or_else(|| anyhow::anyhow!("Create succeeded but no item key was returned"))?;

    // 5. Try OA PDF attachment
    let crossref_value = serde_json::to_value(&work).ok();
    let attached =
        try_attach_oa_pdf(client, &created_key, doi, webdav, crossref_value.as_ref()).await;

    // 6. Format result
    let created_item = client.get_item(&created_key).await?;
    let mut lines = vec![format_item_result(&created_item, None, true)];
    if let Some(pdf_url) = attached {
        lines.push(format!("Attached PDF: {}", pdf_url));
    }

    Ok(AddCrossrefResult {
        key: created_key,
        result: lines.join("\n"),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_crossref_date_full() {
        let work = CrossrefWork {
            published_print: Some(CrossrefDate {
                date_parts: Some(vec![vec![Some(2024), Some(1), Some(15)]]),
            }),
            ..Default::default()
        };
        assert_eq!(parse_crossref_date(&work), Some("2024-01-15".to_string()));
    }

    #[test]
    fn test_parse_crossref_date_year_month() {
        let work = CrossrefWork {
            published_print: Some(CrossrefDate {
                date_parts: Some(vec![vec![Some(2024), Some(1)]]),
            }),
            ..Default::default()
        };
        assert_eq!(parse_crossref_date(&work), Some("2024-01".to_string()));
    }

    #[test]
    fn test_parse_crossref_date_year_only() {
        let work = CrossrefWork {
            published_print: Some(CrossrefDate {
                date_parts: Some(vec![vec![Some(2024)]]),
            }),
            ..Default::default()
        };
        assert_eq!(parse_crossref_date(&work), Some("2024".to_string()));
    }

    #[test]
    fn test_parse_crossref_date_empty() {
        let work = CrossrefWork::default();
        assert_eq!(parse_crossref_date(&work), None);
    }

    #[test]
    fn test_parse_crossref_date_fallback_to_issued() {
        let work = CrossrefWork {
            issued: Some(CrossrefDate {
                date_parts: Some(vec![vec![Some(2023), Some(6), Some(1)]]),
            }),
            ..Default::default()
        };
        assert_eq!(parse_crossref_date(&work), Some("2023-06-01".to_string()));
    }

    #[test]
    fn test_parse_crossref_date_none_parts() {
        let work = CrossrefWork {
            published_print: Some(CrossrefDate {
                date_parts: Some(vec![vec![None]]),
            }),
            issued: Some(CrossrefDate {
                date_parts: Some(vec![vec![Some(2022)]]),
            }),
            ..Default::default()
        };
        assert_eq!(parse_crossref_date(&work), Some("2022".to_string()));
    }

    #[test]
    fn test_map_crossref_creators_authors() {
        let work = CrossrefWork {
            author: Some(vec![
                CrossrefPerson {
                    given: Some("John".to_string()),
                    family: Some("Smith".to_string()),
                    name: None,
                },
                CrossrefPerson {
                    given: Some("Jane".to_string()),
                    family: Some("Doe".to_string()),
                    name: None,
                },
            ]),
            ..Default::default()
        };
        let creators = map_crossref_creators(&work);
        assert_eq!(creators.len(), 2);
        assert_eq!(creators[0].creator_type, "author");
        assert_eq!(creators[0].first_name.as_deref(), Some("John"));
        assert_eq!(creators[0].last_name.as_deref(), Some("Smith"));
        assert_eq!(creators[1].first_name.as_deref(), Some("Jane"));
        assert_eq!(creators[1].last_name.as_deref(), Some("Doe"));
    }

    #[test]
    fn test_map_crossref_creators_editors() {
        let work = CrossrefWork {
            editor: Some(vec![CrossrefPerson {
                given: Some("Alice".to_string()),
                family: Some("Editor".to_string()),
                name: None,
            }]),
            ..Default::default()
        };
        let creators = map_crossref_creators(&work);
        assert_eq!(creators.len(), 1);
        assert_eq!(creators[0].creator_type, "editor");
        assert_eq!(creators[0].first_name.as_deref(), Some("Alice"));
        assert_eq!(creators[0].last_name.as_deref(), Some("Editor"));
    }

    #[test]
    fn test_map_crossref_creators_name_field() {
        let work = CrossrefWork {
            author: Some(vec![CrossrefPerson {
                given: None,
                family: None,
                name: Some("UNESCO".to_string()),
            }]),
            ..Default::default()
        };
        let creators = map_crossref_creators(&work);
        assert_eq!(creators.len(), 1);
        assert_eq!(creators[0].creator_type, "author");
        assert_eq!(creators[0].name.as_deref(), Some("UNESCO"));
        assert_eq!(creators[0].first_name, None);
        assert_eq!(creators[0].last_name, None);
    }

    #[test]
    fn test_map_crossref_creators_mixed() {
        let work = CrossrefWork {
            author: Some(vec![
                CrossrefPerson {
                    given: Some("John".to_string()),
                    family: Some("Smith".to_string()),
                    name: None,
                },
                CrossrefPerson {
                    given: None,
                    family: None,
                    name: Some("WHO".to_string()),
                },
            ]),
            editor: Some(vec![CrossrefPerson {
                given: Some("Ed".to_string()),
                family: Some("Itor".to_string()),
                name: None,
            }]),
            ..Default::default()
        };
        let creators = map_crossref_creators(&work);
        assert_eq!(creators.len(), 3);
        assert_eq!(creators[0].creator_type, "author");
        assert_eq!(creators[0].last_name.as_deref(), Some("Smith"));
        assert_eq!(creators[1].creator_type, "author");
        assert_eq!(creators[1].name.as_deref(), Some("WHO"));
        assert_eq!(creators[2].creator_type, "editor");
    }

    #[test]
    fn test_map_crossref_creators_empty() {
        let work = CrossrefWork::default();
        let creators = map_crossref_creators(&work);
        assert!(creators.is_empty());
    }

    #[test]
    fn test_crossref_work_deserialize() {
        let json = r#"{
            "type": "journal-article",
            "title": ["Test Paper"],
            "author": [{"given": "John", "family": "Smith"}],
            "DOI": "10.1234/test",
            "volume": "42",
            "issue": "3",
            "page": "100-110",
            "publisher": "Test Publisher",
            "ISSN": ["1234-5678"],
            "abstract": "<p>This is the abstract.</p>",
            "container-title": ["Test Journal"],
            "published-print": {"date-parts": [[2024, 1, 15]]}
        }"#;
        let work: CrossrefWork = serde_json::from_str(json).unwrap();
        assert_eq!(work.work_type.as_deref(), Some("journal-article"));
        assert_eq!(work.title.as_ref().unwrap()[0], "Test Paper");
        assert_eq!(work.doi.as_deref(), Some("10.1234/test"));
        assert_eq!(work.volume.as_deref(), Some("42"));
        assert_eq!(
            work.abstract_note.as_deref(),
            Some("<p>This is the abstract.</p>")
        );
        assert_eq!(parse_crossref_date(&work), Some("2024-01-15".to_string()));
    }
}
