#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
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

    // 3. Get item template for imported_file attachment
    let mut attach_data = client
        .get_item_template("attachment", Some("imported_file"))
        .await
        .map_err(|e| anyhow!("Failed to get item template: {e}"))?;
    attach_data.item_type = "attachment".to_string();
    attach_data.parent_item = Some(parent_key.to_string());
    attach_data.link_mode = Some("imported_file".to_string());
    attach_data.title = Some(actual_filename.clone());
    attach_data.filename = Some(actual_filename.clone());
    attach_data.content_type = Some(content_type);

    // 4. Create attachment item in Zotero
    let create_resp = client
        .create_items(&[attach_data.clone()])
        .await
        .map_err(|e| anyhow!("Failed to create attachment item: {e}"))?;
    let create_value = serde_json::to_value(&create_resp)?;
    let status = handle_write_response(&create_value);
    if !status.ok {
        return Err(anyhow!("Failed to create attachment: {}", status.message));
    }
    let attachment_key =
        extract_attachment_key(status.data.as_ref().unwrap_or(&Value::Null))?;

    // 5. Upload to WebDAV
    let upload_result = webdav
        .upload_file(&attachment_key, &actual_filename, &data)
        .await?;

    // 6. Update attachment with md5 + mtime
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

    let key =
        extract_attachment_key(status.data.as_ref().unwrap_or(&Value::Null)).ok();
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
    use serde_json::json;

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
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No attachment key"));
    }
}
