use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

use crate::clients::webdav::WebDavClient;
use crate::clients::zotero::ZoteroClient;
use crate::services::arxiv::add_via_arxiv;
use crate::services::crossref::add_via_crossref;
use crate::services::identifiers::{
    detect_input_type, find_existing_by_arxiv_id, find_existing_by_doi, normalize_arxiv_id,
    normalize_doi, resolve_collection_names, InputType,
};
use crate::services::pdf::rename_pdf_attachments;
use crate::shared::formatters::format_item_result;
use crate::shared::types::ZoteroTag;
use crate::shared::validators::{dedupe_strings, is_collection_key, parse_str_list, StringOrList};

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
    let collection_refs = parse_str_list(args.collections);
    let collection_keys = resolve_collections(client, &collection_refs).await?;
    let tags: Vec<ZoteroTag> = dedupe_strings(parse_str_list(args.tags))
        .into_iter()
        .map(|tag| ZoteroTag { tag, tag_type: None })
        .collect();

    match input_type {
        InputType::File => {
            // File upload flow — requires WebDAV
            // (stub for now — file upload not fully implemented)
            Ok("File upload not yet supported".to_string())
        }
        InputType::Doi => {
            // DOI flow: CrossRef metadata + OA PDF
            let doi = normalize_doi(input)?;

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
            Ok("ISBN lookup is not supported in this version. Use DOI or arXiv ID instead."
                .to_string())
        }
        InputType::Url => {
            Ok("URL metadata extraction is not supported in this version. Use DOI or arXiv ID instead."
                .to_string())
        }
    }
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
