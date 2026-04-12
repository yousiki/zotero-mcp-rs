use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use rmcp::schemars::{self, JsonSchema};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroCreator {
    pub creator_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoteroTag {
    pub tag: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub tag_type: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroItemData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    #[serde(default)]
    pub item_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creators: Option<Vec<ZoteroCreator>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_added: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstract_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<String>,
    #[serde(rename = "DOI", skip_serializing_if = "Option::is_none")]
    pub doi: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(rename = "ISSN", skip_serializing_if = "Option::is_none")]
    pub issn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ZoteroTag>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collections: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_item: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md5: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtime: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation_comment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation_page_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotation_position: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_mode: Option<String>,
    #[serde(flatten, default)]
    pub extra_fields: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoteroLibrary {
    #[serde(rename = "type")]
    pub library_type: String,
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroItemMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_children: Option<i64>,
    #[serde(flatten, default)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoteroItem {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<ZoteroLibrary>,
    #[serde(default)]
    pub data: ZoteroItemData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<ZoteroItemMeta>,
    #[serde(flatten, default)]
    pub extra_fields: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ZoteroCollectionData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_collection: Option<Value>,
}

impl ZoteroCollectionData {
    pub fn parent_collection_key(&self) -> Option<&str> {
        match &self.parent_collection {
            Some(Value::String(s)) if !s.is_empty() => Some(s.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ZoteroCollection {
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
    #[serde(default)]
    pub data: ZoteroCollectionData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<HashMap<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttachmentDetails {
    pub key: String,
    pub title: String,
    pub filename: String,
    pub content_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WriteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub successful: Option<HashMap<String, ZoteroItem>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unchanged: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<HashMap<String, WriteError>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WriteError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct SearchCondition {
    pub field: String,
    pub operation: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FulltextData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_pages: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    #[serde(rename = "type")]
    pub library_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feed_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feed_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_check: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_check_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creators: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_added: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abstract_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_time: Option<String>,
}

pub fn crossref_type_map() -> HashMap<&'static str, &'static str> {
    let mut map = HashMap::new();
    map.insert("journal-article", "journalArticle");
    map.insert("book", "book");
    map.insert("book-chapter", "bookSection");
    map.insert("proceedings-article", "conferencePaper");
    map.insert("report", "report");
    map.insert("dissertation", "thesis");
    map.insert("posted-content", "preprint");
    map.insert("monograph", "book");
    map.insert("reference-entry", "encyclopediaArticle");
    map.insert("dataset", "document");
    map.insert("peer-review", "document");
    map.insert("edited-book", "book");
    map.insert("standard", "document");
    map.insert("other", "document");
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_zotero_item() {
        let json = r#"{
            "key": "ABCD1234",
            "version": 42,
            "data": {
                "key": "ABCD1234",
                "version": 42,
                "itemType": "journalArticle",
                "title": "Test Paper",
                "creators": [
                    {"creatorType": "author", "firstName": "John", "lastName": "Doe"}
                ],
                "date": "2024",
                "DOI": "10.1234/test",
                "tags": [{"tag": "machine learning"}, {"tag": "AI"}],
                "collections": ["COLL0001"],
                "extra": "citation key: test2024"
            }
        }"#;
        let item: ZoteroItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.key, "ABCD1234");
        assert_eq!(item.data.item_type, "journalArticle");
        assert_eq!(item.data.title.as_deref(), Some("Test Paper"));
        assert_eq!(item.data.doi.as_deref(), Some("10.1234/test"));
        let tags = item.data.tags.as_ref().unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].tag, "machine learning");
    }

    #[test]
    fn test_deserialize_zotero_collection() {
        let json = r#"{
            "key": "COLL0001",
            "version": 5,
            "data": {
                "key": "COLL0001",
                "version": 5,
                "name": "My Collection",
                "parentCollection": "PRNT0001"
            }
        }"#;
        let coll: ZoteroCollection = serde_json::from_str(json).unwrap();
        assert_eq!(coll.key, "COLL0001");
        assert_eq!(coll.data.name, "My Collection");
        assert_eq!(coll.data.parent_collection_key(), Some("PRNT0001"));
    }

    #[test]
    fn test_collection_no_parent() {
        let json = r#"{
            "key": "COLL0001",
            "data": {"name": "Root Collection", "parentCollection": false}
        }"#;
        let coll: ZoteroCollection = serde_json::from_str(json).unwrap();
        assert_eq!(coll.data.parent_collection_key(), None);
    }

    #[test]
    fn test_deserialize_annotation_item() {
        let json = r##"{
            "key": "ANN00001",
            "data": {
                "itemType": "annotation",
                "annotationType": "highlight",
                "annotationText": "Important finding",
                "annotationColor": "#ffd400",
                "annotationPageLabel": "42",
                "parentItem": "ABCD1234"
            }
        }"##;
        let item: ZoteroItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.data.annotation_type.as_deref(), Some("highlight"));
        assert_eq!(
            item.data.annotation_text.as_deref(),
            Some("Important finding")
        );
    }

    #[test]
    fn test_write_response_deserialize() {
        let json = r#"{
            "success": {"0": "NEWKEY1"},
            "failed": {}
        }"#;
        let resp: WriteResponse = serde_json::from_str(json).unwrap();
        let success = resp.success.unwrap();
        assert_eq!(success.get("0").map(|s| s.as_str()), Some("NEWKEY1"));
    }

    #[test]
    fn test_crossref_type_map() {
        let map = crossref_type_map();
        assert_eq!(map.get("journal-article"), Some(&"journalArticle"));
        assert_eq!(map.get("book"), Some(&"book"));
        assert_eq!(map.get("posted-content"), Some(&"preprint"));
    }
}
