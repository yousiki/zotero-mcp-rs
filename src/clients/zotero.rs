#![allow(dead_code)]

use std::collections::HashMap;
use std::future::Future;

use reqwest::{Client, Method, RequestBuilder, Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::shared::types::{
    FulltextData, WriteResponse, ZoteroCollection, ZoteroCollectionData, ZoteroItem, ZoteroItemData,
};

#[derive(Error, Debug)]
pub enum ZoteroApiError {
    #[error("Zotero API error ({status}): {message}")]
    ApiError { status: u16, message: String },
    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, ZoteroApiError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TagEntry {
    pub tag: String,
}

#[derive(Clone)]
pub struct ZoteroClient {
    base_url: String,
    api_key: String,
    pub library_id: String,
    pub library_type: String,
    http: Client,
}

impl ZoteroClient {
    pub fn new(api_key: impl Into<String>, library_id: impl Into<String>, library_type: impl Into<String>) -> Self {
        Self::with_base_url(api_key, library_id, library_type, "https://api.zotero.org")
    }

    pub fn with_base_url(
        api_key: impl Into<String>,
        library_id: impl Into<String>,
        library_type: impl Into<String>,
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            api_key: api_key.into(),
            library_id: library_id.into(),
            library_type: library_type.into(),
            http: Client::builder().no_proxy().build().expect("failed to build reqwest client"),
        }
    }

    pub async fn get_items(&self, params: HashMap<String, String>) -> Result<Vec<ZoteroItem>> {
        self.request_json(Method::GET, &format!("{}/items", self.library_path()), Some(&params), None::<&Value>, None)
            .await
    }

    pub async fn get_item(&self, key: &str) -> Result<ZoteroItem> {
        self.request_json(
            Method::GET,
            &format!("{}/items/{}", self.library_path(), key),
            None,
            None::<&Value>,
            None,
        )
        .await
    }

    pub async fn get_item_children(&self, key: &str) -> Result<Vec<ZoteroItem>> {
        self.request_json(
            Method::GET,
            &format!("{}/items/{}/children", self.library_path(), key),
            None,
            None::<&Value>,
            None,
        )
        .await
    }

    pub async fn get_item_fulltext(&self, key: &str) -> Result<FulltextData> {
        self.request_json(
            Method::GET,
            &format!("{}/items/{}/fulltext", self.library_path(), key),
            None,
            None::<&Value>,
            None,
        )
        .await
    }

    pub async fn get_collections(&self, params: HashMap<String, String>) -> Result<Vec<ZoteroCollection>> {
        self.request_json(
            Method::GET,
            &format!("{}/collections", self.library_path()),
            Some(&params),
            None::<&Value>,
            None,
        )
        .await
    }

    pub async fn get_collection(&self, key: &str) -> Result<ZoteroCollection> {
        self.request_json(
            Method::GET,
            &format!("{}/collections/{}", self.library_path(), key),
            None,
            None::<&Value>,
            None,
        )
        .await
    }

    pub async fn get_collection_items(&self, key: &str, params: HashMap<String, String>) -> Result<Vec<ZoteroItem>> {
        self.request_json(
            Method::GET,
            &format!("{}/collections/{}/items", self.library_path(), key),
            Some(&params),
            None::<&Value>,
            None,
        )
        .await
    }

    pub async fn get_tags(&self, params: HashMap<String, String>) -> Result<Vec<TagEntry>> {
        self.request_json(
            Method::GET,
            &format!("{}/tags", self.library_path()),
            Some(&params),
            None::<&Value>,
            None,
        )
        .await
    }

    pub async fn get_groups(&self) -> Result<Vec<Value>> {
        self.request_json(
            Method::GET,
            &format!("/users/{}/groups", self.library_id),
            None,
            None::<&Value>,
            None,
        )
        .await
    }

    pub async fn get_item_template(&self, item_type: &str, link_mode: Option<&str>) -> Result<ZoteroItemData> {
        let mut params = HashMap::from([("itemType".to_string(), item_type.to_string())]);
        if let Some(link_mode) = link_mode {
            params.insert("linkMode".to_string(), link_mode.to_string());
        }

        self.request_json(Method::GET, "/items/new", Some(&params), None::<&Value>, None)
            .await
    }

    pub async fn create_items(&self, items: &[ZoteroItemData]) -> Result<WriteResponse> {
        self.request_json(
            Method::POST,
            &format!("{}/items", self.library_path()),
            None,
            Some(items),
            None,
        )
        .await
    }

    pub async fn update_item(&self, key: &str, data: &ZoteroItemData, version: Option<i64>) -> Result<()> {
        self.request_empty(
            Method::PATCH,
            &format!("{}/items/{}", self.library_path(), key),
            None,
            Some(data),
            version.map(version_header),
        )
        .await
    }

    pub async fn delete_item(&self, key: &str, version: Option<i64>) -> Result<bool> {
        let response = self
            .send(
                self.request_builder(
                    Method::DELETE,
                    &format!("{}/items/{}", self.library_path(), key),
                    None,
                    None::<&Value>,
                    version.map(version_header),
                ),
            )
            .await?;
        Ok(response.status().as_u16() == 204)
    }

    pub async fn add_to_collection(&self, collection_key: &str, item_key: &str) -> Result<()> {
        let item = self.get_item(item_key).await?;
        let version = item.version;
        let mut data = item.data;
        let collections = data.collections.get_or_insert_with(Vec::new);
        if !collections.iter().any(|key| key == collection_key) {
            collections.push(collection_key.to_string());
        }
        self.update_item(item_key, &data, version).await
    }

    pub async fn remove_from_collection(&self, collection_key: &str, item_key: &str) -> Result<()> {
        let item = self.get_item(item_key).await?;
        let version = item.version;
        let mut data = item.data;
        let filtered = data
            .collections
            .unwrap_or_default()
            .into_iter()
            .filter(|key| key != collection_key)
            .collect::<Vec<_>>();
        data.collections = Some(filtered);
        self.update_item(item_key, &data, version).await
    }

    pub async fn create_collections(&self, data: &[ZoteroCollectionData]) -> Result<WriteResponse> {
        self.request_json(
            Method::POST,
            &format!("{}/collections", self.library_path()),
            None,
            Some(data),
            None,
        )
        .await
    }

    pub async fn delete_collection(&self, key: &str, version: Option<i64>) -> Result<bool> {
        let response = self
            .send(
                self.request_builder(
                    Method::DELETE,
                    &format!("{}/collections/{}", self.library_path(), key),
                    None,
                    None::<&Value>,
                    version.map(version_header),
                ),
            )
            .await?;
        Ok(response.status().as_u16() == 204)
    }

    pub async fn paginate<F, Fut, T>(
        &self,
        f: F,
        params: HashMap<String, String>,
        max_items: Option<usize>,
    ) -> Result<Vec<T>>
    where
        F: Fn(HashMap<String, String>) -> Fut,
        Fut: Future<Output = Result<Vec<T>>>,
    {
        let limit = 100usize;
        let mut start = params
            .get("start")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let mut results = Vec::new();

        loop {
            let mut page_params = params.clone();
            page_params.insert("limit".to_string(), limit.to_string());
            page_params.insert("start".to_string(), start.to_string());

            let mut page = f(page_params).await?;
            let page_len = page.len();
            if page_len == 0 {
                break;
            }

            results.append(&mut page);

            if let Some(max_items) = max_items {
                if results.len() >= max_items {
                    results.truncate(max_items);
                    break;
                }
            }

            if page_len < limit {
                break;
            }

            start += limit;
        }

        Ok(results)
    }

    fn library_path(&self) -> String {
        if self.library_type.eq_ignore_ascii_case("group") {
            format!("/groups/{}", self.library_id)
        } else {
            format!("/users/{}", self.library_id)
        }
    }

    fn build_url(&self, path: &str) -> String {
        if path.starts_with('/') {
            format!("{}{}", self.base_url, path)
        } else {
            format!("{}/{}", self.base_url, path)
        }
    }

    fn request_builder<B: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        params: Option<&HashMap<String, String>>,
        body: Option<&B>,
        extra_header: Option<(&'static str, String)>,
    ) -> RequestBuilder {
        let mut builder = self
            .http
            .request(method, self.build_url(path))
            .header("Zotero-API-Key", &self.api_key)
            .header("Zotero-API-Version", "3");

        if let Some(params) = params {
            builder = builder.query(params);
        }

        if let Some((name, value)) = extra_header {
            builder = builder.header(name, value);
        }

        if let Some(body) = body {
            builder = builder.json(body);
        }

        builder
    }

    async fn request_json<T, B>(
        &self,
        method: Method,
        path: &str,
        params: Option<&HashMap<String, String>>,
        body: Option<&B>,
        extra_header: Option<(&'static str, String)>,
    ) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
        B: Serialize + ?Sized,
    {
        let response = self.send(self.request_builder(method, path, params, body, extra_header)).await?;
        let text = response.text().await?;
        Ok(serde_json::from_str(&text)?)
    }

    async fn request_empty<B>(
        &self,
        method: Method,
        path: &str,
        params: Option<&HashMap<String, String>>,
        body: Option<&B>,
        extra_header: Option<(&'static str, String)>,
    ) -> Result<()>
    where
        B: Serialize + ?Sized,
    {
        self.send(self.request_builder(method, path, params, body, extra_header))
            .await?;
        Ok(())
    }

    async fn send(&self, builder: RequestBuilder) -> Result<Response> {
        let response = builder.send().await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }

        let message = response.text().await.unwrap_or_default();
        Err(ZoteroApiError::ApiError {
            status: status.as_u16(),
            message,
        })
    }
}

fn version_header(version: i64) -> (&'static str, String) {
    ("If-Unmodified-Since-Version", version.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client(server: &MockServer) -> ZoteroClient {
        ZoteroClient::with_base_url("test-key", "12345", "user", server.uri())
    }

    fn item(key: &str, title: &str, version: i64) -> Value {
        json!({
            "key": key,
            "version": version,
            "data": {
                "itemType": "journalArticle",
                "title": title,
                "collections": []
            }
        })
    }

    fn collection(key: &str, name: &str, version: i64) -> Value {
        json!({
            "key": key,
            "version": version,
            "data": {
                "name": name
            }
        })
    }

    #[tokio::test]
    async fn get_item_returns_correct_item_and_headers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/12345/items/ITEM1"))
            .and(header("Zotero-API-Key", "test-key"))
            .and(header("Zotero-API-Version", "3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(item("ITEM1", "Test Title", 7)))
            .mount(&server)
            .await;

        let client = test_client(&server);
        let result = client.get_item("ITEM1").await.unwrap();

        assert_eq!(result.key, "ITEM1");
        assert_eq!(result.version, Some(7));
        assert_eq!(result.data.title.as_deref(), Some("Test Title"));
    }

    #[tokio::test]
    async fn get_items_paginates_across_three_pages() {
        let server = MockServer::start().await;

        for (start, count) in [(0usize, 100usize), (100, 100), (200, 50)] {
            let body = (0..count)
                .map(|index| item(&format!("ITEM{:03}", start + index), "Paged", 1))
                .collect::<Vec<_>>();

            Mock::given(method("GET"))
                .and(path("/users/12345/items"))
                .and(query_param("start", start.to_string()))
                .and(query_param("limit", "100"))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }

        let client = test_client(&server);
        let results = client
            .paginate(|params| client.get_items(params), HashMap::new(), None)
            .await
            .unwrap();

        assert_eq!(results.len(), 250);
        assert_eq!(results.first().map(|item| item.key.as_str()), Some("ITEM000"));
        assert_eq!(results.last().map(|item| item.key.as_str()), Some("ITEM249"));
    }

    #[tokio::test]
    async fn update_item_sends_if_unmodified_since_version_header() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/users/12345/items/ITEM1"))
            .and(header("If-Unmodified-Since-Version", "9"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = test_client(&server);
        let data = ZoteroItemData {
            item_type: "journalArticle".to_string(),
            title: Some("Updated".to_string()),
            ..Default::default()
        };

        client.update_item("ITEM1", &data, Some(9)).await.unwrap();
    }

    #[tokio::test]
    async fn delete_item_sends_version_header_and_returns_true_on_204() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/users/12345/items/ITEM1"))
            .and(header("If-Unmodified-Since-Version", "4"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&server)
            .await;

        let client = test_client(&server);
        let deleted = client.delete_item("ITEM1", Some(4)).await.unwrap();

        assert!(deleted);
    }

    #[tokio::test]
    async fn non_success_status_returns_api_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/12345/items/MISSING"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Not found"))
            .mount(&server)
            .await;

        let client = test_client(&server);
        let error = client.get_item("MISSING").await.unwrap_err();

        match error {
            ZoteroApiError::ApiError { status, message } => {
                assert_eq!(status, 404);
                assert_eq!(message, "Not found");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_collections_returns_collection_list() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/12345/collections"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![collection("COL1", "One", 1)]))
            .mount(&server)
            .await;

        let client = test_client(&server);
        let collections = client.get_collections(HashMap::new()).await.unwrap();

        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].key, "COL1");
        assert_eq!(collections[0].data.name, "One");
    }

    #[tokio::test]
    async fn empty_page_terminates_pagination() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/12345/items"))
            .and(query_param("start", "0"))
            .and(query_param("limit", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![item("ITEM001", "Only", 1)]))
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/users/12345/items"))
            .and(query_param("start", "100"))
            .and(query_param("limit", "100"))
            .respond_with(ResponseTemplate::new(200).set_body_json(Vec::<Value>::new()))
            .mount(&server)
            .await;

        let client = test_client(&server);
        let results = client
            .paginate(|params| client.get_items(params), HashMap::new(), None)
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "ITEM001");
    }
}
