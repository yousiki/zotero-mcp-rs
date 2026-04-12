use std::collections::HashMap;

use regex::Regex;
use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;
use serde_json::json;

use crate::clients::zotero::ZoteroClient;
use crate::services::identifiers::{InputType, detect_input_type, normalize_doi};
use crate::shared::formatters::{clean_html, format_item_result};
use crate::shared::types::ZoteroItem;
use crate::shared::validators::{StringOrList, dedupe_strings, normalize_limit, parse_str_list};

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

fn default_recent_limit() -> i64 {
    10
}

fn default_tags_limit() -> i64 {
    500
}

fn default_dedup_method() -> String {
    "both".to_string()
}

fn default_dedup_limit() -> i64 {
    50
}

/// Parameters for the zotero_get_recent tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetRecentArgs {
    /// Maximum number of recent items to return.
    #[serde(default = "default_recent_limit")]
    pub limit: i64,
}

/// Parameters for the zotero_list_tags tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTagsArgs {
    /// Maximum number of tags to return.
    #[serde(default = "default_tags_limit")]
    pub limit: i64,
}

/// Parameters for the zotero_deduplicate tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeduplicateArgs {
    /// Action: "find" or "merge".
    pub action: String,
    /// Method: "title", "doi", or "both".
    #[serde(default = "default_dedup_method")]
    pub method: String,
    /// Optional collection key to scope the search.
    pub collection_key: Option<String>,
    /// Maximum duplicate groups to report (find mode).
    #[serde(default = "default_dedup_limit")]
    pub limit: i64,
    /// Keeper item key (merge mode).
    pub keeper_key: Option<String>,
    /// Duplicate item keys (merge mode).
    pub duplicate_keys: Option<StringOrList>,
    /// Confirm merge execution (default: dry-run).
    #[serde(default)]
    pub confirm: bool,
}

/// Parameters for the fetch tool.
/// Accepts: Zotero item key, DOI, or arXiv ID.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FetchArgs {
    /// Zotero item key, DOI, or arXiv ID.
    pub id: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Normalize a title for deduplication comparison.
fn normalize_title(value: &str) -> String {
    let re_tag = Regex::new(r"<[^>]+>").unwrap();
    let re_punct = Regex::new(r"[\s\p{P}]+").unwrap();
    let lower = value.to_lowercase();
    let no_tags = re_tag.replace_all(&lower, " ");
    re_punct.replace_all(&no_tags, " ").trim().to_string()
}

/// Safely normalize a DOI, returning empty string on failure.
fn safe_normalize_doi(value: Option<&str>) -> String {
    match value {
        None => String::new(),
        Some(v) if v.trim().is_empty() => String::new(),
        Some(v) => normalize_doi(v)
            .map(|d| d.to_lowercase())
            .unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Handle the zotero_get_recent tool.
pub async fn handle_zotero_get_recent(client: &ZoteroClient, args: GetRecentArgs) -> String {
    match handle_zotero_get_recent_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_get_recent_inner(
    client: &ZoteroClient,
    args: GetRecentArgs,
) -> anyhow::Result<String> {
    let limit = normalize_limit(Some(args.limit), 10, 200);
    let mut params = HashMap::new();
    params.insert("sort".to_string(), "dateAdded".to_string());
    params.insert("direction".to_string(), "desc".to_string());
    params.insert("limit".to_string(), limit.to_string());
    params.insert("itemType".to_string(), "-attachment".to_string());

    let items = client.get_items(params).await?;

    if items.is_empty() {
        return Ok("No recent items found.".to_string());
    }

    let result = items
        .iter()
        .enumerate()
        .map(|(i, item)| format_item_result(item, Some(i + 1), true))
        .collect::<Vec<_>>()
        .join("\n\n");

    Ok(result)
}

/// Handle the zotero_list_tags tool.
pub async fn handle_zotero_list_tags(client: &ZoteroClient, args: ListTagsArgs) -> String {
    match handle_zotero_list_tags_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_list_tags_inner(
    client: &ZoteroClient,
    args: ListTagsArgs,
) -> anyhow::Result<String> {
    let limit = normalize_limit(Some(args.limit), 500, 10000) as usize;

    let paginate_params = HashMap::new();
    let tag_entries = client
        .paginate(|p| client.get_tags(p), paginate_params, Some(limit))
        .await?;

    let mut tags: Vec<String> = tag_entries
        .into_iter()
        .map(|entry| entry.tag)
        .filter(|tag| !tag.trim().is_empty())
        .collect();
    tags.sort_by_key(|a| a.to_lowercase());

    if tags.is_empty() {
        return Ok("No tags found.".to_string());
    }

    // Group by first letter
    let mut grouped: HashMap<String, Vec<String>> = HashMap::new();
    for tag in &tags {
        let initial = tag.chars().next().unwrap_or('#').to_uppercase().to_string();
        let bucket = if initial.len() == 1 && initial.chars().next().unwrap().is_ascii_alphabetic()
        {
            initial
        } else {
            "#".to_string()
        };
        grouped.entry(bucket).or_default().push(tag.clone());
    }

    let mut sorted_keys: Vec<String> = grouped.keys().cloned().collect();
    sorted_keys.sort();

    let mut sections: Vec<String> = Vec::new();
    for letter in sorted_keys {
        let entries = grouped.get(&letter).unwrap();
        sections.push(format!("## {}", letter));
        for entry in entries {
            sections.push(format!("- {}", entry));
        }
        sections.push(String::new());
    }

    Ok(sections.join("\n").trim().to_string())
}

/// Handle the zotero_deduplicate tool.
pub async fn handle_zotero_deduplicate(client: &ZoteroClient, args: DeduplicateArgs) -> String {
    match handle_zotero_deduplicate_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_deduplicate_inner(
    client: &ZoteroClient,
    args: DeduplicateArgs,
) -> anyhow::Result<String> {
    if args.action == "find" {
        let limit = normalize_limit(Some(args.limit), 50, 500) as usize;

        let items: Vec<ZoteroItem> = if let Some(ref collection_key) = args.collection_key {
            let mut params = HashMap::new();
            params.insert("itemType".to_string(), "-attachment".to_string());
            client
                .paginate(
                    |p| client.get_collection_items(collection_key, p),
                    params,
                    Some(5000),
                )
                .await?
        } else {
            let mut params = HashMap::new();
            params.insert("itemType".to_string(), "-attachment".to_string());
            client
                .paginate(|p| client.get_items(p), params, Some(5000))
                .await?
        };

        let mut title_groups: HashMap<String, Vec<(String, String, String)>> = HashMap::new();
        let mut doi_groups: HashMap<String, Vec<(String, String, String)>> = HashMap::new();

        for item in &items {
            let title = item.data.title.as_deref().unwrap_or("").trim().to_string();
            let doi = safe_normalize_doi(item.data.doi.as_deref());
            let row = (item.key.clone(), title.clone(), doi.clone());
            let normalized_title = normalize_title(&title);
            if !normalized_title.is_empty() {
                title_groups
                    .entry(normalized_title)
                    .or_default()
                    .push(row.clone());
            }
            if !doi.is_empty() {
                doi_groups.entry(doi).or_default().push(row);
            }
        }

        let mut lines: Vec<String> = Vec::new();

        let emit_groups = |label: &str,
                           groups: &HashMap<String, Vec<(String, String, String)>>,
                           lines: &mut Vec<String>,
                           limit: usize| {
            let duplicate_groups: Vec<_> = groups.values().filter(|g| g.len() >= 2).collect();
            if duplicate_groups.is_empty() {
                return;
            }
            lines.push(format!("## {}", label));
            let mut idx = 0usize;
            for group in duplicate_groups.iter().take(limit) {
                idx += 1;
                let title = &group[0].1;
                let doi = &group[0].2;
                let title_display = if title.is_empty() {
                    "(untitled)"
                } else {
                    title.as_str()
                };
                let doi_suffix = if doi.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", doi)
                };
                lines.push(format!("{}. {}{}", idx, title_display, doi_suffix));
                for entry in group.iter() {
                    lines.push(format!("  - {}", entry.0));
                }
            }
            lines.push(String::new());
        };

        if args.method == "title" || args.method == "both" {
            emit_groups("Title duplicates", &title_groups, &mut lines, limit);
        }
        if args.method == "doi" || args.method == "both" {
            emit_groups("DOI duplicates", &doi_groups, &mut lines, limit);
        }

        if lines.is_empty() {
            return Ok("No duplicates found.".to_string());
        }

        return Ok(lines.join("\n").trim().to_string());
    }

    // Validate action
    if args.action != "merge" {
        return Err(anyhow::anyhow!(
            "Invalid action: '{}'. Must be one of: find, merge",
            args.action
        ));
    }

    // Merge action
    let keeper_key = args
        .keeper_key
        .ok_or_else(|| anyhow::anyhow!("keeper_key is required for action=merge"))?;

    let keeper = client.get_item(&keeper_key).await?;
    let duplicate_keys = dedupe_strings(parse_str_list(args.duplicate_keys))
        .into_iter()
        .filter(|k| k != &keeper_key)
        .collect::<Vec<_>>();

    if duplicate_keys.is_empty() {
        return Err(anyhow::anyhow!(
            "duplicate_keys must include at least one non-keeper key"
        ));
    }

    let mut duplicates: Vec<ZoteroItem> = Vec::new();
    let mut duplicate_children: Vec<Vec<ZoteroItem>> = Vec::new();
    for key in &duplicate_keys {
        duplicates.push(client.get_item(key).await?);
        duplicate_children.push(client.get_item_children(key).await?);
    }

    let mut merged_tag_names: std::collections::HashSet<String> = keeper
        .data
        .tags
        .as_ref()
        .unwrap_or(&vec![])
        .iter()
        .map(|t| t.tag.clone())
        .collect();
    let mut merged_collections: std::collections::HashSet<String> = keeper
        .data
        .collections
        .as_ref()
        .unwrap_or(&vec![])
        .iter()
        .cloned()
        .collect();
    let mut child_count = 0usize;

    for (i, duplicate) in duplicates.iter().enumerate() {
        let children = duplicate_children
            .get(i)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        child_count += children.len();
        for tag in duplicate.data.tags.as_ref().unwrap_or(&vec![]) {
            merged_tag_names.insert(tag.tag.clone());
        }
        for collection in duplicate.data.collections.as_ref().unwrap_or(&vec![]) {
            merged_collections.insert(collection.clone());
        }
    }

    if !args.confirm {
        let preview = [
            "Dry run preview:".to_string(),
            format!("Keeper: {}", keeper.key),
            format!("Duplicates: {}", duplicate_keys.join(", ")),
            format!("Merged tags: {}", merged_tag_names.len()),
            format!("Merged collections: {}", merged_collections.len()),
            format!("Children to re-parent: {}", child_count),
            "Set confirm=true to execute merge.".to_string(),
        ];
        return Ok(preview.join("\n"));
    }

    // Execute merge: update keeper with merged tags and collections
    let mut keeper_data = keeper.data.clone();
    keeper_data.tags = Some(
        merged_tag_names
            .into_iter()
            .map(|tag| crate::shared::types::ZoteroTag {
                tag,
                tag_type: None,
            })
            .collect(),
    );
    keeper_data.collections = Some(merged_collections.into_iter().collect());
    client
        .update_item(&keeper.key, &keeper_data, keeper.version)
        .await?;

    // Re-parent children to keeper
    for children in &duplicate_children {
        for child in children {
            let mut child_data = child.data.clone();
            child_data.parent_item = Some(keeper.key.clone());
            client
                .update_item(&child.key, &child_data, child.version)
                .await?;
        }
    }

    // Delete duplicates
    for duplicate in &duplicates {
        client
            .delete_item(&duplicate.key, duplicate.version)
            .await?;
    }

    let result = [
        format!("Merged into keeper: {}", keeper.key),
        format!("Duplicates trashed: {}", duplicates.len()),
        format!("Children re-parented: {}", child_count),
    ];
    Ok(result.join("\n"))
}

/// Handle the fetch tool (ChatGPT connector).
/// Supports: Zotero item key, DOI, or arXiv ID.
pub async fn handle_fetch(client: &ZoteroClient, args: FetchArgs) -> String {
    match handle_fetch_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_fetch_inner(client: &ZoteroClient, args: FetchArgs) -> anyhow::Result<String> {
    let input = args.id.trim();
    if input.is_empty() {
        return Ok(
            "Input cannot be empty. Provide a Zotero item key, DOI, or arXiv ID.".to_string(),
        );
    }

    // Detect input type and resolve to a Zotero item key
    let input_type = detect_input_type(input);
    let item_key = match input_type {
        InputType::Doi => {
            // Normalize DOI
            let doi = normalize_doi(input)?;

            // Search for existing item by DOI
            let mut params = HashMap::new();
            params.insert("q".to_string(), doi.clone());
            params.insert("qmode".to_string(), "everything".to_string());
            params.insert("limit".to_string(), "50".to_string());
            let items = client.get_items(params).await?;

            // Find exact DOI match
            let normalized_doi = doi.to_lowercase();
            let matched = items.into_iter().find(|item| {
                item.data
                    .doi
                    .as_deref()
                    .map(|d| d.to_lowercase() == normalized_doi)
                    .unwrap_or(false)
            });

            match matched {
                Some(item) => item.key,
                None => {
                    return Ok(format!(
                        "No item found with DOI: {}. Use zotero_add_paper to add this paper.",
                        doi
                    ));
                }
            }
        }
        InputType::Arxiv => {
            // Normalize arXiv ID
            let arxiv_id = crate::services::identifiers::normalize_arxiv_id(input)?;

            // Search for existing item by arXiv ID (in URL or extra field)
            let mut params = HashMap::new();
            params.insert("q".to_string(), arxiv_id.clone());
            params.insert("qmode".to_string(), "everything".to_string());
            params.insert("limit".to_string(), "50".to_string());
            let items = client.get_items(params).await?;

            // Find item with matching arXiv ID
            let matched = items.into_iter().find(|item| {
                // Check URL field
                if item
                    .data
                    .url
                    .as_deref()
                    .map(|u| u.contains(&arxiv_id))
                    .unwrap_or(false)
                {
                    return true;
                }
                // Check extra field for arXiv ID
                if item
                    .data
                    .extra
                    .as_deref()
                    .map(|e| e.to_lowercase().contains(&arxiv_id.to_lowercase()))
                    .unwrap_or(false)
                {
                    return true;
                }
                false
            });

            match matched {
                Some(item) => item.key,
                None => {
                    return Ok(format!(
                        "No item found with arXiv ID: {}. Use zotero_add_paper to add this paper.",
                        arxiv_id
                    ));
                }
            }
        }
        _ => {
            // Treat as Zotero item key
            input.to_string()
        }
    };

    // Fetch the item and its children
    let item = client.get_item(&item_key).await?;
    let children = client.get_item_children(&item_key).await?;

    let notes: Vec<&ZoteroItem> = children
        .iter()
        .filter(|c| c.data.item_type == "note")
        .collect();
    let annotations: Vec<&ZoteroItem> = children
        .iter()
        .filter(|c| c.data.item_type == "annotation")
        .collect();

    let mut text_parts: Vec<String> = Vec::new();
    text_parts.push(item.data.title.as_deref().unwrap_or("").to_string());

    if let Some(abstract_note) = &item.data.abstract_note
        && !abstract_note.is_empty()
    {
        text_parts.push(clean_html(abstract_note, false));
    }

    if let Some(extra) = &item.data.extra
        && !extra.is_empty()
    {
        text_parts.push(extra.clone());
    }

    for note in &notes {
        let raw = note.data.note.as_deref().unwrap_or("");
        text_parts.push(clean_html(raw, false));
    }

    for annotation in &annotations {
        let mut segments: Vec<String> = Vec::new();
        if let Some(text) = &annotation.data.annotation_text
            && !text.is_empty()
        {
            segments.push(text.clone());
        }
        if let Some(comment) = &annotation.data.annotation_comment
            && !comment.is_empty()
        {
            segments.push(comment.clone());
        }
        let segment = segments.join("\n");
        if !segment.is_empty() {
            text_parts.push(segment);
        }
    }

    let payload = json!({
        "id": item.key,
        "title": item.data.title.as_deref().unwrap_or("(untitled)"),
        "text": text_parts.into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n"),
        "url": item.data.url,
        "metadata": {
            "itemType": item.data.item_type,
            "date": item.data.date,
            "doi": item.data.doi,
            "tags": item.data.tags.as_ref().unwrap_or(&vec![]).iter().map(|t| &t.tag).collect::<Vec<_>>(),
        }
    });

    Ok(serde_json::to_string_pretty(&payload)?)
}
