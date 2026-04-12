use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

use crate::clients::webdav::WebDavClient;
use crate::clients::zotero::{ZoteroApiError, ZoteroClient};
use crate::services::arxiv::add_via_arxiv;
use crate::services::crossref::add_via_crossref;
use crate::services::identifiers::{
    InputType, detect_input_type, find_existing_by_arxiv_id, find_existing_by_doi,
    normalize_arxiv_id, normalize_doi, resolve_collection_names,
};
use crate::services::pdf::{attach_pdf_bytes, rename_pdf_attachments};
use crate::shared::formatters::format_item_result;
use crate::shared::types::{ZoteroItemData, ZoteroTag};
use crate::shared::validators::{
    StringOrList, dedupe_strings, extract_created_key, handle_write_response, is_collection_key,
    parse_str_list,
};
use std::path::{Path, PathBuf};

/// Parameters for the zotero_add_paper tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddPaperArgs {
    /// URL, DOI, arXiv ID/URL, ISBN, PMID, or local file path.
    pub input: String,
    /// Collection keys or names.
    pub collections: Option<StringOrList>,
    /// Tags to add.
    pub tags: Option<StringOrList>,
}

/// Execute the zotero_add_paper tool, returning formatted Markdown results.
pub async fn handle_zotero_add_paper(
    client: &ZoteroClient,
    webdav: &WebDavClient,
    args: AddPaperArgs,
) -> String {
    match add_paper_inner(client, webdav, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn add_paper_inner(
    client: &ZoteroClient,
    webdav: &WebDavClient,
    args: AddPaperArgs,
) -> anyhow::Result<String> {
    let input = args.input.trim();
    if input.is_empty() {
        return Ok("input cannot be empty".to_string());
    }

    let input_type = detect_input_type(input);

    match input_type {
        InputType::File => {
            let path = resolve_local_file_path(input)?;
            let bytes = validate_local_pdf(&path).await?;
            let (filename, title) = file_name_and_title(&path)?;
            let collection_refs = parse_str_list(args.collections);
            let collection_keys = resolve_collections(client, &collection_refs).await?;
            let tags = build_tags(args.tags);
            let template = get_file_parent_template(client).await?;

            let mut item_data = template.clone();
            item_data.title = Some(title);
            item_data.collections = Some(collection_keys);
            item_data.tags = Some(tags);

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

            if let Err(err) = attach_pdf_bytes(
                client,
                &created_key,
                webdav,
                &filename,
                "application/pdf",
                &bytes,
            )
            .await
            {
                let attach_error = err.to_string();
                match client.delete_item(&created_key, None).await {
                    Ok(true) => return Err(anyhow::anyhow!(attach_error)),
                    Ok(false) => {
                        return Err(anyhow::anyhow!(
                            "{}; rollback delete returned non-204",
                            attach_error
                        ));
                    }
                    Err(rollback_err) => {
                        return Err(anyhow::anyhow!(
                            "{}; rollback failed: {}",
                            attach_error,
                            rollback_err
                        ));
                    }
                }
            }

            let created_item = client.get_item(&created_key).await?;
            Ok(format_item_result(&created_item, None, true))
        }
        InputType::Doi => {
            // DOI flow: CrossRef metadata + OA PDF
            let doi = normalize_doi(input)?;
            let collection_refs = parse_str_list(args.collections);
            let collection_keys = resolve_collections(client, &collection_refs).await?;
            let tags = build_tags(args.tags);

            // Check for existing item
            if let Some(existing) = find_existing_by_doi(&doi).await {
                return Ok(format!(
                    "Item already exists: {}",
                    format_item_result(&existing, None, true)
                ));
            }

            // Call add_via_crossref
            let result = add_via_crossref(client, &doi, collection_keys, tags, webdav).await?;

            // Rename PDF attachments
            rename_pdf_attachments(client, &result.key).await?;

            Ok(result.result)
        }
        InputType::Arxiv => {
            // arXiv flow: arXiv Atom API + direct PDF
            let arxiv_id = normalize_arxiv_id(input)?;
            let collection_refs = parse_str_list(args.collections);
            let collection_keys = resolve_collections(client, &collection_refs).await?;
            let tags = build_tags(args.tags);

            // Check for existing item
            if let Some(existing) = find_existing_by_arxiv_id(&arxiv_id).await {
                return Ok(format!(
                    "Item already exists: {}",
                    format_item_result(&existing, None, true)
                ));
            }

            // Call add_via_arxiv
            let result = add_via_arxiv(client, webdav, &arxiv_id, collection_keys, tags).await?;

            Ok(result.result)
        }
        InputType::Isbn => {
            let collection_refs = parse_str_list(args.collections);
            let _collection_keys = resolve_collections(client, &collection_refs).await?;
            let _tags = build_tags(args.tags);
            Ok(
                "ISBN lookup is not supported in this version. Use DOI or arXiv ID instead."
                    .to_string(),
            )
        }
        InputType::Url => {
            let collection_refs = parse_str_list(args.collections);
            let _collection_keys = resolve_collections(client, &collection_refs).await?;
            let _tags = build_tags(args.tags);
            Ok("URL metadata extraction is not supported in this version. Use DOI or arXiv ID instead."
                .to_string())
        }
    }
}

fn build_tags(tags: Option<StringOrList>) -> Vec<ZoteroTag> {
    dedupe_strings(parse_str_list(tags))
        .into_iter()
        .map(|tag| ZoteroTag {
            tag,
            tag_type: None,
        })
        .collect()
}

fn file_name_and_title(path: &Path) -> anyhow::Result<(String, String)> {
    let filename = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow::anyhow!("{} has no file name", path.display()))?;

    let title = Path::new(&filename)
        .file_stem()
        .map(|stem| {
            stem.to_string_lossy()
                .replace(['_', '-'], " ")
                .trim()
                .to_string()
        })
        .filter(|stem| !stem.is_empty())
        .unwrap_or_else(|| filename.clone());

    Ok((filename, title))
}

async fn get_file_parent_template(client: &ZoteroClient) -> anyhow::Result<ZoteroItemData> {
    match client.get_item_template("document", None).await {
        Ok(template) => Ok(template),
        Err(ZoteroApiError::ApiError { status: 404, .. }) => client
            .get_item_template("report", None)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get item template: {e}")),
        Err(err) => Err(anyhow::anyhow!("Failed to get item template: {err}")),
    }
}

fn resolve_local_file_path(input: &str) -> anyhow::Result<PathBuf> {
    if let Some(rest) = input.strip_prefix("~/") {
        let home = std::env::var("HOME").map_err(|_| anyhow::anyhow!("HOME is not set"))?;
        return Ok(PathBuf::from(home).join(rest));
    }

    if input.starts_with('/') || input.starts_with("./") {
        return Ok(PathBuf::from(input));
    }

    Err(anyhow::anyhow!("unsupported local file path: {}", input))
}

async fn validate_local_pdf(path: &Path) -> anyhow::Result<Vec<u8>> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))?;

    if !metadata.is_file() {
        return Err(anyhow::anyhow!("{} is not a file", path.display()));
    }

    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| anyhow::anyhow!("{}: {}", path.display(), e))?;

    if bytes.is_empty() {
        return Err(anyhow::anyhow!("{} is empty", path.display()));
    }

    if !bytes.starts_with(b"%PDF-") {
        return Err(anyhow::anyhow!("{} is not a PDF", path.display()));
    }

    Ok(bytes)
}

async fn resolve_collections(
    client: &ZoteroClient,
    refs: &[String],
) -> anyhow::Result<Vec<String>> {
    let mut keys = Vec::new();
    let mut names = Vec::new();
    for ref_ in refs {
        let trimmed = ref_.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_collection_key(trimmed) {
            keys.push(trimmed.to_string());
        } else {
            names.push(trimmed.to_string());
        }
    }
    let resolved = resolve_collection_names(client, &names).await;
    keys.extend(resolved);
    Ok(dedupe_strings(keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use wiremock::matchers::{body_string_contains, method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn test_clients() -> (ZoteroClient, WebDavClient, MockServer, MockServer) {
        let zotero_server = MockServer::start().await;

        let webdav_server = MockServer::start().await;

        let client = ZoteroClient::with_base_url("key", "12345", "user", zotero_server.uri());
        let webdav = WebDavClient::new(&webdav_server.uri(), "user", "pass");

        (client, webdav, zotero_server, webdav_server)
    }

    async fn mock_collection_lookup(zotero_server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/users/12345/collections"))
            .respond_with(ResponseTemplate::new(200).set_body_json(Vec::<serde_json::Value>::new()))
            .expect(1)
            .mount(zotero_server)
            .await;
    }

    async fn mock_no_local_file_side_effects(
        zotero_server: &MockServer,
        webdav_server: &MockServer,
    ) {
        Mock::given(method("GET"))
            .and(path_regex(r".*/items/new.*"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(zotero_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/users/12345/items"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(zotero_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path_regex(r".*/items/.*"))
            .respond_with(ResponseTemplate::new(204))
            .expect(0)
            .mount(zotero_server)
            .await;

        Mock::given(method("PUT"))
            .and(path_regex(".*"))
            .respond_with(ResponseTemplate::new(201))
            .expect(0)
            .mount(webdav_server)
            .await;
    }

    fn unique_temp_path(name: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "zotero-mcp-rs-{name}-{}-{stamp}",
            std::process::id()
        ))
    }

    #[tokio::test]
    async fn test_add_paper_local_pdf_missing_file() {
        let (client, webdav, zs, ws) = test_clients().await;
        mock_no_local_file_side_effects(&zs, &ws).await;
        let input = format!(
            "./missing-{}-{}.pdf",
            std::process::id(),
            unique_temp_path("missing").display()
        );

        let result = handle_zotero_add_paper(
            &client,
            &webdav,
            AddPaperArgs {
                input,
                collections: None,
                tags: None,
            },
        )
        .await;

        assert!(result.starts_with("Error: "));
        assert!(result.contains("No such file") || result.contains("not found"));
    }

    #[tokio::test]
    async fn test_add_paper_local_pdf_directory_path() {
        let (client, webdav, zs, ws) = test_clients().await;
        mock_no_local_file_side_effects(&zs, &ws).await;
        let dir = unique_temp_path("dir");
        fs::create_dir_all(&dir).unwrap();

        let result = handle_zotero_add_paper(
            &client,
            &webdav,
            AddPaperArgs {
                input: dir.to_string_lossy().into_owned(),
                collections: None,
                tags: None,
            },
        )
        .await;

        let _ = fs::remove_dir_all(&dir);

        assert!(result.starts_with("Error: "));
        assert!(result.contains("is not a file"));
    }

    #[tokio::test]
    async fn test_add_paper_local_pdf_non_pdf() {
        let (client, webdav, zs, ws) = test_clients().await;
        mock_no_local_file_side_effects(&zs, &ws).await;
        let file = unique_temp_path("text");
        fs::write(&file, b"hello world").unwrap();

        let result = handle_zotero_add_paper(
            &client,
            &webdav,
            AddPaperArgs {
                input: file.to_string_lossy().into_owned(),
                collections: None,
                tags: None,
            },
        )
        .await;

        let _ = fs::remove_file(&file);

        assert!(result.starts_with("Error: "));
        assert!(result.contains("is not a PDF"));
    }

    #[tokio::test]
    async fn test_add_paper_local_pdf_success() {
        unsafe {
            std::env::set_var("NO_PROXY", "127.0.0.1,localhost");
            std::env::set_var("no_proxy", "127.0.0.1,localhost");
        }

        let zotero_server = MockServer::start().await;
        let webdav_server = MockServer::start().await;
        let client = ZoteroClient::with_base_url("key", "12345", "user", zotero_server.uri());
        let webdav = WebDavClient::new(&webdav_server.uri(), "user", "pass");

        mock_collection_lookup(&zotero_server).await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/items/new.*"))
            .and(query_param("itemType", "document"))
            .respond_with(ResponseTemplate::new(404).set_body_string("template missing"))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/items/new.*"))
            .and(query_param("itemType", "report"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"itemType":"report","title":""})),
            )
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/users/12345/items"))
            .and(body_string_contains("\"itemType\":\"report\""))
            .and(body_string_contains("\"title\":\"my local paper\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "successful": {"0": {"key": "PARENT1", "data": {}}},
                "failed": {}
            })))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/items/new.*"))
            .and(query_param("itemType", "attachment"))
            .and(query_param("linkMode", "imported_file"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"itemType":"attachment"})),
            )
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/users/12345/items"))
            .and(body_string_contains("\"itemType\":\"attachment\""))
            .and(body_string_contains("\"parentItem\":\"PARENT1\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "successful": {"0": {"key": "ATTACH1", "data": {}}},
                "failed": {}
            })))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/users/12345/items/ATTACH1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "key": "ATTACH1",
                "version": 1,
                "data": {"itemType": "attachment", "title": "local-paper.pdf", "filename": "local-paper.pdf", "contentType": "application/pdf", "parentItem": "PARENT1"}
            })))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/users/12345/items/PARENT1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "key": "PARENT1",
                "version": 1,
                "data": {"itemType": "report", "title": "my local paper"}
            })))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/users/12345/items/ATTACH1"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/ATTACH1.zip"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&webdav_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/ATTACH1.prop"))
            .respond_with(ResponseTemplate::new(201))
            .expect(1)
            .mount(&webdav_server)
            .await;

        let file_dir = unique_temp_path("local-paper");
        fs::create_dir_all(&file_dir).unwrap();
        let file = file_dir.join("my-local_paper.pdf");
        fs::write(&file, b"%PDF-1.4\nlocal paper\n%%EOF").unwrap();

        let result = handle_zotero_add_paper(
            &client,
            &webdav,
            AddPaperArgs {
                input: file.to_string_lossy().into_owned(),
                collections: None,
                tags: None,
            },
        )
        .await;

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(&file_dir);

        assert!(result.contains("**my local paper**"), "{}", result);
        assert!(result.contains("Key: PARENT1"), "{}", result);
        assert!(result.contains("Type: report"), "{}", result);
    }

    #[tokio::test]
    async fn test_add_paper_document_template_error_does_not_fallback() {
        let (client, webdav, zotero_server, _webdav_server) = test_clients().await;
        mock_collection_lookup(&zotero_server).await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/items/new.*"))
            .and(query_param("itemType", "document"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/items/new.*"))
            .and(query_param("itemType", "report"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&zotero_server)
            .await;

        let file_dir = unique_temp_path("local-paper-template-error");
        fs::create_dir_all(&file_dir).unwrap();
        let file = file_dir.join("my-local_paper.pdf");
        fs::write(&file, b"%PDF-1.4\nlocal paper\n%%EOF").unwrap();

        let result = handle_zotero_add_paper(
            &client,
            &webdav,
            AddPaperArgs {
                input: file.to_string_lossy().into_owned(),
                collections: None,
                tags: None,
            },
        )
        .await;

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(&file_dir);

        assert!(result.starts_with("Error: "), "{}", result);
        assert!(result.contains("500"), "{}", result);
    }

    #[tokio::test]
    async fn test_add_paper_local_pdf_rolls_back_parent_on_attach_failure() {
        let (client, webdav, zotero_server, webdav_server) = test_clients().await;
        mock_collection_lookup(&zotero_server).await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/items/new.*"))
            .and(query_param("itemType", "document"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"itemType":"document","title":""})),
            )
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/items/new.*"))
            .and(query_param("itemType", "report"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&zotero_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/users/12345/items"))
            .and(body_string_contains("\"itemType\":\"document\""))
            .and(body_string_contains("\"title\":\"my local paper\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "successful": {"0": {"key": "PARENT1", "data": {}}},
                "failed": {}
            })))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/users/12345/items"))
            .and(body_string_contains("\"itemType\":\"attachment\""))
            .respond_with(ResponseTemplate::new(500).set_body_string("attach failed"))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(r".*/items/new.*"))
            .and(query_param("itemType", "attachment"))
            .and(query_param("linkMode", "imported_file"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"itemType":"attachment"})),
            )
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/users/12345/items/PARENT1"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&zotero_server)
            .await;

        Mock::given(method("PUT"))
            .and(path_regex(".*"))
            .respond_with(ResponseTemplate::new(201))
            .expect(0)
            .mount(&webdav_server)
            .await;

        let file_dir = unique_temp_path("local-paper-rollback");
        fs::create_dir_all(&file_dir).unwrap();
        let file = file_dir.join("my-local_paper.pdf");
        fs::write(&file, b"%PDF-1.4\nlocal paper\n%%EOF").unwrap();

        let result = handle_zotero_add_paper(
            &client,
            &webdav,
            AddPaperArgs {
                input: file.to_string_lossy().into_owned(),
                collections: None,
                tags: None,
            },
        )
        .await;

        let _ = fs::remove_file(&file);
        let _ = fs::remove_dir_all(&file_dir);

        assert!(result.starts_with("Error: "), "{}", result);
        assert!(result.contains("attach failed"), "{}", result);
    }
}
