use std::collections::HashMap;

use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

use crate::clients::webdav::WebDavClient;
use crate::clients::zotero::ZoteroClient;
use crate::services::identifiers::{normalize_doi, resolve_collection_names};
use crate::shared::formatters::{
    clean_html, format_item_metadata, format_item_result, generate_bibtex,
};
use crate::shared::types::{ZoteroItem, ZoteroTag};
use crate::shared::validators::{
    StringOrList, dedupe_strings, is_collection_key, normalize_limit, parse_creator_names,
    parse_str_list,
};

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

fn default_include() -> Vec<String> {
    vec!["metadata".to_string()]
}

fn default_update_limit() -> i64 {
    50
}

/// Parameters for the zotero_get_item tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ItemGetArgs {
    /// The Zotero item key to retrieve.
    pub item_key: String,
    /// Sections to include: "metadata", "children", "fulltext", "bibtex".
    #[serde(default = "default_include")]
    pub include: Vec<String>,
}

/// Parameters for the zotero_update_item tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ItemUpdateArgs {
    /// Single item key to update.
    pub item_key: Option<String>,
    /// Batch selector: list of item keys.
    pub item_keys: Option<Vec<String>>,
    /// Batch selector: free-text query.
    #[serde(default)]
    pub query: String,
    /// Batch selector: filter by tag(s).
    pub tag: Option<StringOrList>,
    /// Maximum items for batch operations.
    #[serde(default = "default_update_limit")]
    pub limit: i64,
    /// New title.
    pub title: Option<String>,
    /// New creators (comma-separated or array).
    pub creators: Option<StringOrList>,
    /// New date.
    pub date: Option<String>,
    /// New publication title.
    pub publication_title: Option<String>,
    /// New abstract note.
    pub abstract_note: Option<String>,
    /// Replace all tags.
    pub tags: Option<StringOrList>,
    /// Add tags (preserves existing).
    pub add_tags: Option<StringOrList>,
    /// Remove tags.
    pub remove_tags: Option<StringOrList>,
    /// Add to collections (keys or names).
    pub collections: Option<StringOrList>,
    /// Add to collections by name.
    pub collection_names: Option<StringOrList>,
    /// New DOI.
    pub doi: Option<String>,
    /// New URL.
    pub url: Option<String>,
    /// New extra field.
    pub extra: Option<String>,
}

/// Parameters for the zotero_delete_item tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ItemDeleteArgs {
    /// Item key(s) to delete.
    pub item_keys: StringOrList,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Apply tag add/remove operations to a current tag list.
fn apply_tag_ops(current: &[ZoteroTag], add: &[String], remove: &[String]) -> Vec<ZoteroTag> {
    let mut by_lower: HashMap<String, String> = current
        .iter()
        .filter(|t| !t.tag.trim().is_empty())
        .map(|t| (t.tag.to_lowercase(), t.tag.clone()))
        .collect();
    for tag in add {
        by_lower.insert(tag.to_lowercase(), tag.clone());
    }
    for tag in remove {
        by_lower.remove(&tag.to_lowercase());
    }
    by_lower
        .into_values()
        .map(|tag| ZoteroTag {
            tag,
            tag_type: None,
        })
        .collect()
}

/// Format a group of children items as Markdown lines.
fn format_children_group(items: &[ZoteroItem], kind: &str) -> Vec<String> {
    if items.is_empty() {
        return vec!["- none".to_string()];
    }

    match kind {
        "attachment" => items
            .iter()
            .map(|item| {
                let title = item
                    .data
                    .title
                    .as_deref()
                    .or(item.data.filename.as_deref())
                    .unwrap_or("Attachment");
                let content_type = item.data.content_type.as_deref().unwrap_or("unknown");
                format!("- {} ({}) [{}]", title, item.key, content_type)
            })
            .collect(),
        "note" => items
            .iter()
            .map(|item| {
                let raw = item.data.note.as_deref().unwrap_or("");
                let preview = clean_html(raw, true)
                    .chars()
                    .take(160)
                    .collect::<String>()
                    .trim()
                    .to_string();
                let preview = if preview.is_empty() {
                    "(empty note)".to_string()
                } else {
                    preview
                };
                format!("- {} ({})", preview, item.key)
            })
            .collect(),
        _ => items
            .iter()
            .map(|item| {
                let title = item.data.title.as_deref().unwrap_or("(untitled)");
                format!("- {}: {} ({})", item.data.item_type, title, item.key)
            })
            .collect(),
    }
}

/// Resolve a mix of collection keys and names to keys.
async fn resolve_collection_keys(client: &ZoteroClient, refs: &[String]) -> Vec<String> {
    if refs.is_empty() {
        return vec![];
    }
    let mut keys: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for r in refs {
        let trimmed = r.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_collection_key(trimmed) {
            keys.push(trimmed.to_string());
        } else {
            names.push(trimmed.to_string());
        }
    }
    if !names.is_empty() {
        let resolved = resolve_collection_names(client, &names).await;
        keys.extend(resolved);
    }
    dedupe_strings(keys)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Handle the zotero_get_item tool.
pub async fn handle_zotero_get_item(client: &ZoteroClient, args: ItemGetArgs) -> String {
    match handle_zotero_get_item_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_get_item_inner(
    client: &ZoteroClient,
    args: ItemGetArgs,
) -> anyhow::Result<String> {
    let include = if args.include.is_empty() {
        default_include()
    } else {
        args.include
    };

    let mut sections: Vec<String> = Vec::new();

    if include.iter().any(|s| s == "metadata") {
        let item = client.get_item(&args.item_key).await?;
        sections.push(format_item_metadata(&item, true));
    }

    if include.iter().any(|s| s == "children") {
        let children = client.get_item_children(&args.item_key).await?;
        if children.is_empty() {
            sections.push("## Children\n\nThis item has no children.".to_string());
        } else {
            let attachments: Vec<_> = children
                .iter()
                .filter(|c| c.data.item_type == "attachment")
                .cloned()
                .collect();
            let notes: Vec<_> = children
                .iter()
                .filter(|c| c.data.item_type == "note")
                .cloned()
                .collect();
            let others: Vec<_> = children
                .iter()
                .filter(|c| c.data.item_type != "attachment" && c.data.item_type != "note")
                .cloned()
                .collect();

            let mut lines = Vec::new();
            lines.push("## Children".to_string());
            lines.push(String::new());
            lines.push("### Attachments".to_string());
            lines.extend(format_children_group(&attachments, "attachment"));
            lines.push(String::new());
            lines.push("### Notes".to_string());
            lines.extend(format_children_group(&notes, "note"));
            lines.push(String::new());
            lines.push("### Others".to_string());
            lines.extend(format_children_group(&others, "other"));
            sections.push(lines.join("\n"));
        }
    }

    if include.iter().any(|s| s == "fulltext") {
        let mut fulltext_content = String::new();
        let mut indexed_pages: Option<i64> = None;
        let mut total_pages: Option<i64> = None;

        // Try item key first, then try best attachment key
        let mut candidate_keys = vec![args.item_key.clone()];
        if let Ok(children) = client.get_item_children(&args.item_key).await
            && let Some(best) = crate::shared::formatters::find_best_attachment(&children)
        {
            candidate_keys.insert(0, best.key);
        }

        for key in &candidate_keys {
            match client.get_item_fulltext(key).await {
                Ok(ft) => {
                    if let Some(content) = &ft.content {
                        let trimmed = content.trim();
                        if !trimmed.is_empty() {
                            fulltext_content = trimmed.to_string();
                            indexed_pages = ft.indexed_pages;
                            total_pages = ft.total_pages;
                            break;
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        let mut lines = vec!["## Full Text".to_string(), String::new()];
        if fulltext_content.is_empty() {
            lines.push("No indexed full text content available.".to_string());
        } else {
            if indexed_pages.is_some() || total_pages.is_some() {
                let ip = indexed_pages
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let tp = total_pages
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "?".to_string());
                lines.push(format!("Indexed pages: {}/{}", ip, tp));
                lines.push(String::new());
            }
            lines.push(fulltext_content);
        }
        sections.push(lines.join("\n"));
    }

    if include.iter().any(|s| s == "bibtex") {
        let item = client.get_item(&args.item_key).await?;
        sections.push(format!("## BibTeX\n\n{}", generate_bibtex(&item)));
    }

    Ok(sections.join("\n\n"))
}

/// Handle the zotero_update_item tool.
pub async fn handle_zotero_update_item(client: &ZoteroClient, args: ItemUpdateArgs) -> String {
    match handle_zotero_update_item_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_update_item_inner(
    client: &ZoteroClient,
    args: ItemUpdateArgs,
) -> anyhow::Result<String> {
    let explicit_item_keys = dedupe_strings(args.item_keys.clone().unwrap_or_default());
    let query = args.query.trim().to_string();
    let required_tags = dedupe_strings(parse_str_list(args.tag.clone()));
    let add_tags = dedupe_strings(parse_str_list(args.add_tags.clone()));
    let remove_tags = dedupe_strings(parse_str_list(args.remove_tags.clone()));
    let has_batch_selector =
        !explicit_item_keys.is_empty() || !query.is_empty() || !required_tags.is_empty();
    let has_single_update_target = args.item_key.as_ref().is_some_and(|k| !k.trim().is_empty());

    // Batch tag update mode
    if has_batch_selector && !has_single_update_target {
        if add_tags.is_empty() && remove_tags.is_empty() {
            return Ok("No tag operations requested.".to_string());
        }

        let limit = normalize_limit(Some(args.limit), 50, 500) as usize;
        let selected: Vec<crate::shared::types::ZoteroItem>;

        if !explicit_item_keys.is_empty() {
            let mut loaded = Vec::new();
            for key in &explicit_item_keys {
                loaded.push(client.get_item(key).await?);
            }
            selected = loaded;
        } else {
            let mut params = HashMap::new();
            params.insert("q".to_string(), query.clone());
            params.insert("qmode".to_string(), "everything".to_string());
            params.insert("itemType".to_string(), "-attachment".to_string());
            params.insert("limit".to_string(), (limit * 3).min(500).to_string());
            params.insert("sort".to_string(), "dateModified".to_string());
            params.insert("direction".to_string(), "desc".to_string());

            let mut candidates = client.get_items(params).await?;

            if query.is_empty() && !required_tags.is_empty() {
                let mut paginate_params = HashMap::new();
                paginate_params.insert("itemType".to_string(), "-attachment".to_string());
                candidates = client
                    .paginate(|p| client.get_items(p), paginate_params, Some(5000))
                    .await?;
            }

            let required_lower: Vec<String> =
                required_tags.iter().map(|t| t.to_lowercase()).collect();
            selected = candidates
                .into_iter()
                .filter(|item| {
                    if required_lower.is_empty() {
                        return true;
                    }
                    let item_tags: std::collections::HashSet<String> = item
                        .data
                        .tags
                        .as_ref()
                        .map(|tags| tags.iter().map(|t| t.tag.to_lowercase()).collect())
                        .unwrap_or_default();
                    required_lower.iter().all(|tag| item_tags.contains(tag))
                })
                .take(limit)
                .collect();
        }

        if selected.is_empty() {
            return Ok("No matching items found.".to_string());
        }

        let mut updated = 0usize;
        let mut skipped: Vec<String> = Vec::new();

        for item in &selected {
            let current_tags = item.data.tags.as_deref().unwrap_or(&[]);
            let next_tags = apply_tag_ops(current_tags, &add_tags, &remove_tags);

            let before: Vec<String> =
                dedupe_strings(current_tags.iter().map(|t| t.tag.clone()).collect());
            let mut before_sorted = before.clone();
            before_sorted.sort();

            let after: Vec<String> =
                dedupe_strings(next_tags.iter().map(|t| t.tag.clone()).collect());
            let mut after_sorted = after.clone();
            after_sorted.sort();

            if before_sorted == after_sorted {
                skipped.push(item.key.clone());
                continue;
            }

            let mut next_data = item.data.clone();
            next_data.tags = Some(next_tags);
            client
                .update_item(&item.key, &next_data, item.version)
                .await?;
            updated += 1;
        }

        return Ok(format!(
            "Processed: {}\nUpdated: {}\nSkipped (no changes): {}",
            selected.len(),
            updated,
            skipped.len()
        ));
    }

    // Single item update mode
    if !has_single_update_target {
        return Err(anyhow::anyhow!(
            "item_key is required for single-item updates"
        ));
    }

    let item_key = args.item_key.as_ref().unwrap().trim().to_string();
    let item = client.get_item(&item_key).await?;
    let mut next = item.data.clone();

    if let Some(title) = &args.title {
        next.title = Some(title.clone());
    }
    if let Some(date) = &args.date {
        next.date = Some(date.clone());
    }
    if let Some(publication_title) = &args.publication_title {
        next.publication_title = Some(publication_title.clone());
    }
    if let Some(abstract_note) = &args.abstract_note {
        next.abstract_note = Some(clean_html(abstract_note, false));
    }
    if let Some(url) = &args.url {
        next.url = Some(url.clone());
    }
    if let Some(extra) = &args.extra {
        next.extra = Some(extra.clone());
    }
    if let Some(doi) = &args.doi {
        next.doi = if doi.trim().is_empty() {
            Some(String::new())
        } else {
            Some(normalize_doi(doi)?)
        };
    }

    if let Some(creators) = &args.creators {
        let creator_values = parse_str_list(Some(creators.clone()));
        next.creators = Some(parse_creator_names(creator_values));
    }

    if let Some(tags) = &args.tags {
        let replaced = dedupe_strings(parse_str_list(Some(tags.clone())));
        next.tags = Some(
            replaced
                .into_iter()
                .map(|tag| ZoteroTag {
                    tag,
                    tag_type: None,
                })
                .collect(),
        );
    } else if !add_tags.is_empty() || !remove_tags.is_empty() {
        let current_tags = next.tags.as_deref().unwrap_or(&[]);
        next.tags = Some(apply_tag_ops(current_tags, &add_tags, &remove_tags));
    }

    let collection_refs: Vec<String> = [
        parse_str_list(args.collections.clone()),
        parse_str_list(args.collection_names.clone()),
    ]
    .concat();
    if !collection_refs.is_empty() {
        let add_collection_keys = resolve_collection_keys(client, &collection_refs).await;
        let existing = next.collections.get_or_insert_with(Vec::new);
        for key in add_collection_keys {
            if !existing.contains(&key) {
                existing.push(key);
            }
        }
    }

    client.update_item(&item.key, &next, item.version).await?;
    let updated = client.get_item(&item.key).await?;
    Ok(format_item_result(&updated, None, true))
}

/// Handle the zotero_delete_item tool.
pub async fn handle_zotero_delete_item(
    client: &ZoteroClient,
    webdav: &Option<WebDavClient>,
    args: ItemDeleteArgs,
) -> String {
    match handle_zotero_delete_item_inner(client, webdav, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_delete_item_inner(
    client: &ZoteroClient,
    webdav: &Option<WebDavClient>,
    args: ItemDeleteArgs,
) -> anyhow::Result<String> {
    let item_keys = dedupe_strings(args.item_keys.into_vec());
    if item_keys.is_empty() {
        return Err(anyhow::anyhow!("item_keys requires at least one value"));
    }

    let mut deleted = 0usize;
    let mut failed = 0usize;
    let mut webdav_deleted = 0usize;

    for key in &item_keys {
        // First, get the item to retrieve its version and children
        match client.get_item(key).await {
            Ok(item) => {
                // If WebDAV is configured, delete attachment files first
                if let Some(webdav_client) = webdav {
                    // Get children (attachments and notes)
                    match client.get_item_children(key).await {
                        Ok(children) => {
                            for child in &children {
                                // Delete WebDAV files for attachments
                                if child.data.item_type == "attachment" {
                                    if let Err(e) = webdav_client.delete_file(&child.key).await {
                                        // Log but don't fail the whole operation
                                        tracing::warn!("Failed to delete WebDAV file for {}: {}", child.key, e);
                                    } else {
                                        webdav_deleted += 1;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            // Log but continue with item deletion
                            tracing::warn!("Failed to get children for {}: {}", key, e);
                        }
                    }
                }

                // Then delete the item from Zotero
                match client.delete_item(key, item.version).await {
                    Ok(true) => deleted += 1,
                    _ => failed += 1,
                }
            }
            Err(_) => failed += 1,
        }
    }

    let mut result = format!("Requested: {}\nDeleted: {}\nFailed: {}", item_keys.len(), deleted, failed);
    if webdav_deleted > 0 {
        result.push_str(&format!("\nWebDAV files deleted: {}", webdav_deleted));
    }
    Ok(result)
}
