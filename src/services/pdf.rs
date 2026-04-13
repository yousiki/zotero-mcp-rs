#![allow(dead_code)]

use anyhow::{Context, Result, anyhow};
use serde_json::Value;

use crate::clients::webdav::WebDavClient;
use crate::clients::zotero::ZoteroClient;
use crate::shared::template_engine::build_renamed_filename;
use crate::shared::validators::handle_write_response;

/// Extract the attachment key from a Zotero write response Value.
/// Checks `success` map (string values) first, then `successful` map (item objects with `.key`).
fn extract_attachment_key(data: &Value) -> Result<String> {
    if let Some(success) = data.get("success").and_then(|v| v.as_object()) {
        for v in success.values() {
            if let Some(s) = v.as_str() {
                return Ok(s.to_string());
            }
        }
    }

    if let Some(successful) = data.get("successful").and_then(|v| v.as_object()) {
        for v in successful.values() {
            if let Some(key) = v.get("key").and_then(|k| k.as_str()) {
                return Ok(key.to_string());
            }
        }
    }

    Err(anyhow!("No attachment key found in write response"))
}

/// Download PDF from URL, upload to WebDAV, create Zotero attachment item.
/// Returns the attachment key.
pub(crate) async fn attach_pdf_bytes(
    client: &ZoteroClient,
    parent_key: &str,
    webdav: &WebDavClient,
    filename: &str,
    content_type: &str,
    data: &[u8],
) -> Result<String> {
    let mut attach_data = client
        .get_item_template("attachment", Some("imported_file"))
        .await
        .map_err(|e| anyhow!("Failed to get item template: {e}"))?;
    attach_data.item_type = "attachment".to_string();
    attach_data.parent_item = Some(parent_key.to_string());
    attach_data.link_mode = Some("imported_file".to_string());
    attach_data.title = Some(filename.to_string());
    attach_data.filename = Some(filename.to_string());
    attach_data.content_type = Some(content_type.to_string());

    let create_resp = client
        .create_items(&[attach_data.clone()])
        .await
        .map_err(|e| anyhow!("Failed to create attachment item: {e}"))?;
    let create_value = serde_json::to_value(&create_resp)?;
    let status = handle_write_response(&create_value);
    if !status.ok {
        return Err(anyhow!("Failed to create attachment: {}", status.message));
    }
    let attachment_key = extract_attachment_key(status.data.as_ref().unwrap_or(&Value::Null))?;

    let upload_result = webdav.upload_file(&attachment_key, filename, data).await?;

    let mut updated_data = attach_data;
    updated_data.md5 = Some(upload_result.md5);
    updated_data.mtime = Some(upload_result.mtime as i64);
    let attach_item = client
        .get_item(&attachment_key)
        .await
        .map_err(|e| anyhow!("Failed to get attachment item: {e}"))?;
    client
        .update_item(&attachment_key, &updated_data, attach_item.version)
        .await
        .map_err(|e| anyhow!("Failed to update attachment: {e}"))?;

    Ok(attachment_key)
}

pub async fn download_and_attach_pdf(
    client: &ZoteroClient,
    parent_key: &str,
    pdf_url: &str,
    identifier: &str,
    webdav: &WebDavClient,
    filename: &str,
) -> Result<String> {
    // 1. Download with 60s timeout
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    let response = http
        .get(pdf_url)
        .send()
        .await
        .context("Failed to download PDF")?;
    if !response.status().is_success() {
        return Err(anyhow!("Failed to download PDF ({})", response.status()));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/pdf")
        .to_string();
    let data = response.bytes().await?.to_vec();

    // 2. Generate filename
    let actual_filename = if !filename.is_empty() {
        filename.to_string()
    } else {
        let safe_id: String = identifier
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
            .trim_matches('_')
            .to_string();
        format!(
            "{}.pdf",
            if safe_id.is_empty() {
                "paper".to_string()
            } else {
                safe_id
            }
        )
    };

    attach_pdf_bytes(
        client,
        parent_key,
        webdav,
        &actual_filename,
        &content_type,
        &data,
    )
    .await
}

/// Create a linked URL attachment on a Zotero item.
/// Returns the attachment key or None on failure.
pub async fn attach_linked_url(
    client: &ZoteroClient,
    parent_key: &str,
    url: &str,
    title: &str,
) -> Result<Option<String>> {
    let mut attach_data = client
        .get_item_template("attachment", Some("linked_url"))
        .await
        .map_err(|e| anyhow!("Failed to get linked_url template: {e}"))?;
    attach_data.item_type = "attachment".to_string();
    attach_data.parent_item = Some(parent_key.to_string());
    attach_data.link_mode = Some("linked_url".to_string());
    attach_data.title = Some(title.to_string());
    attach_data.url = Some(url.to_string());

    let create_resp = client
        .create_items(&[attach_data])
        .await
        .map_err(|e| anyhow!("Failed to create linked URL attachment: {e}"))?;
    let create_value = serde_json::to_value(&create_resp)?;
    let status = handle_write_response(&create_value);
    if !status.ok {
        return Ok(None);
    }

    let key = extract_attachment_key(status.data.as_ref().unwrap_or(&Value::Null)).ok();
    Ok(key)
}

/// Rename all PDF attachments of an item using the default template.
pub async fn rename_pdf_attachments(client: &ZoteroClient, parent_key: &str) -> Result<()> {
    let parent = client
        .get_item(parent_key)
        .await
        .map_err(|e| anyhow!("Failed to get parent item: {e}"))?;

    let children = client
        .get_item_children(parent_key)
        .await
        .map_err(|e| anyhow!("Failed to get item children: {e}"))?;

    for child in children {
        let is_attachment = child.data.item_type == "attachment";
        let is_pdf = child
            .data
            .content_type
            .as_deref()
            .map(|ct| ct.contains("pdf"))
            .unwrap_or(false);
        let is_imported = child
            .data
            .link_mode
            .as_deref()
            .map(|lm| lm == "imported_file" || lm == "imported_url")
            .unwrap_or(false);

        if !is_attachment || !is_pdf || !is_imported {
            continue;
        }

        let new_filename = build_renamed_filename(&parent.data, "pdf");
        if new_filename.is_empty() {
            continue;
        }

        let current_filename = child.data.filename.as_deref().unwrap_or("");
        if current_filename == new_filename {
            continue;
        }

        let mut updated_data = child.data.clone();
        updated_data.filename = Some(new_filename.clone());
        updated_data.title = Some(new_filename);

        client
            .update_item(&child.key, &updated_data, child.version)
            .await
            .map_err(|e| anyhow!("Failed to rename attachment {}: {e}", child.key))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use md5::{Digest, Md5};
    use serde_json::json;
    use std::io::{Cursor, Read};
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn zotero_client(server: &MockServer) -> ZoteroClient {
        ZoteroClient::with_base_url("test-key", "12345", "user", server.uri())
    }

    fn webdav_client(server: &MockServer) -> WebDavClient {
        WebDavClient::new(&server.uri(), "user", "pass")
    }

    fn md5_hex(data: &[u8]) -> String {
        let mut hasher = Md5::new();
        hasher.update(data);
        hasher
            .finalize()
            .into_iter()
            .map(|b| format!("{:02x}", b))
            .collect()
    }

    #[test]
    fn test_extract_attachment_key_from_success() {
        let data = json!({"success": {"0": "ABCD1234"}, "failed": {}});
        let key = extract_attachment_key(&data).unwrap();
        assert_eq!(key, "ABCD1234");
    }

    #[test]
    fn test_extract_attachment_key_from_successful() {
        let data = json!({
            "successful": {"0": {"key": "EFGH5678", "data": {}}},
            "failed": {}
        });
        let key = extract_attachment_key(&data).unwrap();
        assert_eq!(key, "EFGH5678");
    }

    #[test]
    fn test_extract_attachment_key_missing() {
        let data = json!({"failed": {"0": {"code": 400, "message": "Bad request"}}});
        let result = extract_attachment_key(&data);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No attachment key")
        );
    }

    #[tokio::test]
    async fn test_attach_local_pdf_uploads_and_updates_attachment() {
        unsafe {
            std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
            std::env::set_var("no_proxy", "127.0.0.1,localhost");
        }

        let zotero_server = MockServer::start().await;
        let webdav_server = MockServer::start().await;
        let client = zotero_client(&zotero_server);
        let webdav = webdav_client(&webdav_server);

        let uploaded_mtime: Arc<Mutex<Option<i64>>> = Arc::new(Mutex::new(None));
        let expected_filename = "paper.pdf".to_string();
        let expected_bytes = b"hello local pdf".to_vec();
        let expected_md5 = md5_hex(&expected_bytes);

        Mock::given(method("GET"))
            .and(path("/items/new"))
            .and(query_param("itemType", "attachment"))
            .and(query_param("linkMode", "imported_file"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/users/12345/items"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "successful": {
                    "0": {"key": "ATTACH1", "data": {}}
                }
            })))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/users/12345/items/ATTACH1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "key": "ATTACH1",
                "version": 7,
                "data": {"itemType": "attachment"}
            })))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/users/12345/items/ATTACH1"))
            .respond_with({
                let uploaded_mtime = Arc::clone(&uploaded_mtime);
                let expected_filename = expected_filename.clone();
                let expected_md5 = expected_md5.clone();
                move |request: &wiremock::Request| {
                    let body: serde_json::Value = request.body_json().expect("json body");
                    assert_eq!(body["itemType"], json!("attachment"));
                    assert_eq!(body["parentItem"], json!("PARENT1"));
                    assert_eq!(body["linkMode"], json!("imported_file"));
                    assert_eq!(body["title"], json!(expected_filename));
                    assert_eq!(body["filename"], json!(expected_filename));
                    assert_eq!(body["contentType"], json!("application/pdf"));
                    assert_eq!(body["md5"], json!(expected_md5));
                    let mtime = body["mtime"].as_i64().expect("mtime");
                    assert_eq!(*uploaded_mtime.lock().unwrap(), Some(mtime));
                    ResponseTemplate::new(204)
                }
            })
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/zotero/ATTACH1.zip"))
            .respond_with({
                let expected_filename = expected_filename.clone();
                let expected_bytes = expected_bytes.clone();
                move |request: &wiremock::Request| {
                    let cursor = Cursor::new(request.body.clone());
                    let mut archive = zip::ZipArchive::new(cursor).expect("zip archive");
                    assert_eq!(archive.len(), 1);
                    let mut file = archive.by_index(0).expect("zip entry");
                    assert_eq!(file.name(), expected_filename);
                    let mut contents = Vec::new();
                    file.read_to_end(&mut contents).expect("zip contents");
                    assert_eq!(contents, expected_bytes);
                    ResponseTemplate::new(201)
                }
            })
            .expect(1)
            .mount(&webdav_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/zotero/ATTACH1.prop"))
            .respond_with({
                let expected_md5 = expected_md5.clone();
                let uploaded_mtime = Arc::clone(&uploaded_mtime);
                move |request: &wiremock::Request| {
                    let body = String::from_utf8(request.body.clone()).expect("prop xml");
                    let mtime = body
                        .split("<mtime>")
                        .nth(1)
                        .and_then(|part| part.split("</mtime>").next())
                        .and_then(|value| value.parse::<i64>().ok())
                        .expect("mtime value");
                    assert!(body.contains(&format!("<hash>{}</hash>", expected_md5)));
                    *uploaded_mtime.lock().unwrap() = Some(mtime);
                    ResponseTemplate::new(201)
                }
            })
            .expect(1)
            .mount(&webdav_server)
            .await;

        let attachment_key = attach_pdf_bytes(
            &client,
            "PARENT1",
            &webdav,
            &expected_filename,
            "application/pdf",
            &expected_bytes,
        )
        .await
        .unwrap();

        assert_eq!(attachment_key, "ATTACH1");
    }
}
