use std::collections::HashMap;

use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

use crate::clients::zotero::ZoteroClient;
use crate::shared::formatters::{clean_html, escape_html, truncate};
use crate::shared::types::{ZoteroItem, ZoteroTag};
use crate::shared::validators::{handle_write_response, normalize_limit, parse_str_list};

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

fn default_note_limit() -> i64 {
    20
}

fn default_truncate() -> bool {
    true
}

fn default_color() -> String {
    "#ffd400".to_string()
}

/// Parameters for the zotero_get_annotations tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAnnotationsArgs {
    /// Optional item key to get annotations for a specific item.
    pub item_key: Option<String>,
    /// Maximum number of annotations to return.
    pub limit: Option<i64>,
}

/// Parameters for the zotero_get_notes tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetNotesArgs {
    /// Optional item key to get notes for a specific item.
    pub item_key: Option<String>,
    /// Optional query to search notes and annotations.
    pub query: Option<String>,
    /// Maximum number of notes to return.
    #[serde(default = "default_note_limit")]
    pub limit: i64,
    /// Whether to truncate note content.
    #[serde(default = "default_truncate")]
    pub truncate: bool,
}

/// Parameters for the zotero_add_note tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddNoteArgs {
    /// Type of item to create: "note" or "annotation".
    #[serde(rename = "type")]
    pub note_type: String,
    /// Parent item key (required for type=note).
    pub item_key: Option<String>,
    /// Note title (required for type=note).
    pub note_title: Option<String>,
    /// Note text content (required for type=note).
    pub note_text: Option<String>,
    /// Optional tags for the note.
    pub tags: Option<Vec<String>>,
    /// Parent attachment key (required for type=annotation).
    pub attachment_key: Option<String>,
    /// Page number (required for type=annotation).
    pub page: Option<i64>,
    /// Annotation text (required for type=annotation).
    pub text: Option<String>,
    /// Optional comment for the annotation.
    pub comment: Option<String>,
    /// Annotation color (default: #ffd400).
    #[serde(default = "default_color")]
    pub color: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

struct AnnotationInfo {
    key: String,
    annotation_type: String,
    color: String,
    page_label: String,
    position: String,
    text: String,
    comment: String,
    tags: Vec<String>,
    parent_title: Option<String>,
}

fn format_annotation_block(ann: &AnnotationInfo, index: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push(format!(
        "{}. Type: {}",
        index,
        if ann.annotation_type.is_empty() {
            "annotation"
        } else {
            &ann.annotation_type
        }
    ));
    lines.push(format!(
        "   - Key: {}",
        if ann.key.is_empty() {
            "(none)"
        } else {
            &ann.key
        }
    ));
    if let Some(parent_title) = &ann.parent_title {
        lines.push(format!("   - Parent: {}", parent_title));
    }
    lines.push(format!(
        "   - Color: {}",
        if ann.color.is_empty() {
            "(none)"
        } else {
            &ann.color
        }
    ));
    lines.push(format!(
        "   - Page: {}",
        if ann.page_label.is_empty() {
            "(none)"
        } else {
            &ann.page_label
        }
    ));
    lines.push(format!(
        "   - Position: {}",
        if ann.position.is_empty() {
            "(none)"
        } else {
            &ann.position
        }
    ));
    lines.push(format!(
        "   - Text: {}",
        if ann.text.is_empty() {
            "(none)"
        } else {
            &ann.text
        }
    ));
    lines.push(format!(
        "   - Comment: {}",
        if ann.comment.is_empty() {
            "(none)"
        } else {
            &ann.comment
        }
    ));
    lines.push(format!(
        "   - Tags: {}",
        if ann.tags.is_empty() {
            "(none)".to_string()
        } else {
            ann.tags.join(", ")
        }
    ));
    lines.join("\n")
}

fn build_note_html(title: &str, text: &str) -> String {
    let clean_title = escape_html(title.trim());
    let blocks: Vec<String> = text
        .trim()
        .split("\n\n")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|segment| format!("<p>{}</p>", escape_html(&segment).replace('\n', "<br />")))
        .collect();
    format!("<h1>{}</h1>\n{}", clean_title, blocks.join("\n"))
}

fn annotation_info_from_item(item: &ZoteroItem, parent_title: Option<String>) -> AnnotationInfo {
    let tags = item
        .data
        .tags
        .as_ref()
        .map(|tags| {
            tags.iter()
                .map(|t| t.tag.clone())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();

    AnnotationInfo {
        key: item.key.clone(),
        annotation_type: item.data.annotation_type.clone().unwrap_or_default(),
        color: item.data.annotation_color.clone().unwrap_or_default(),
        page_label: item.data.annotation_page_label.clone().unwrap_or_default(),
        position: item.data.annotation_position.clone().unwrap_or_default(),
        text: item.data.annotation_text.clone().unwrap_or_default(),
        comment: item.data.annotation_comment.clone().unwrap_or_default(),
        tags,
        parent_title,
    }
}

async fn safe_get_item(client: &ZoteroClient, key: &str) -> Option<ZoteroItem> {
    client.get_item(key).await.ok()
}

async fn resolve_annotation_parent_titles(
    client: &ZoteroClient,
    annotations: &[ZoteroItem],
) -> HashMap<String, String> {
    let mut attachment_keys: Vec<String> = Vec::new();
    for annotation in annotations {
        if let Some(parent) = &annotation.data.parent_item
            && !parent.trim().is_empty()
        {
            attachment_keys.push(parent.clone());
        }
    }

    let mut attachment_to_paper: HashMap<String, String> = HashMap::new();
    let mut paper_keys: Vec<String> = Vec::new();

    for key in &attachment_keys {
        if let Some(attachment) = safe_get_item(client, key).await
            && let Some(paper_key) = &attachment.data.parent_item
            && !paper_key.trim().is_empty()
        {
            attachment_to_paper.insert(attachment.key.clone(), paper_key.clone());
            paper_keys.push(paper_key.clone());
        }
    }

    let mut paper_title_map: HashMap<String, String> = HashMap::new();
    for key in &paper_keys {
        if let Some(paper) = safe_get_item(client, key).await {
            paper_title_map.insert(
                paper.key.clone(),
                paper.data.title.unwrap_or_else(|| "(untitled)".to_string()),
            );
        }
    }

    let mut result: HashMap<String, String> = HashMap::new();
    for (attachment_key, paper_key) in &attachment_to_paper {
        if let Some(title) = paper_title_map.get(paper_key) {
            result.insert(attachment_key.clone(), title.clone());
        }
    }

    result
}

async fn resolve_note_parent_titles(
    client: &ZoteroClient,
    notes: &[ZoteroItem],
) -> HashMap<String, String> {
    let mut parent_keys: Vec<String> = Vec::new();
    for note in notes {
        if let Some(parent) = &note.data.parent_item
            && !parent.trim().is_empty()
        {
            parent_keys.push(parent.clone());
        }
    }

    let mut parent_map: HashMap<String, String> = HashMap::new();
    for key in &parent_keys {
        if let Some(parent) = safe_get_item(client, key).await {
            parent_map.insert(
                parent.key.clone(),
                parent
                    .data
                    .title
                    .unwrap_or_else(|| "(untitled)".to_string()),
            );
        }
    }

    parent_map
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Handle the zotero_get_annotations tool.
pub async fn handle_zotero_get_annotations(
    client: &ZoteroClient,
    args: GetAnnotationsArgs,
) -> String {
    match handle_zotero_get_annotations_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_get_annotations_inner(
    client: &ZoteroClient,
    args: GetAnnotationsArgs,
) -> anyhow::Result<String> {
    let limit = normalize_limit(args.limit, 20, 500) as usize;

    if let Some(item_key) = &args.item_key {
        let parent = client.get_item(item_key).await?;
        let parent_title = parent
            .data
            .title
            .unwrap_or_else(|| "(untitled)".to_string());

        let children = client.get_item_children(item_key).await?;
        let mut annotations: Vec<ZoteroItem> = children
            .into_iter()
            .filter(|item| item.data.item_type == "annotation")
            .collect();

        // If no direct annotations found, check if children are PDF attachments and traverse them
        if annotations.is_empty() {
            let pdf_attachments: Vec<ZoteroItem> = client
                .get_item_children(item_key)
                .await?
                .into_iter()
                .filter(|item| {
                    item.data.item_type == "attachment"
                        && item
                            .data
                            .content_type
                            .as_deref()
                            .unwrap_or("")
                            .contains("pdf")
                })
                .collect();

            for attachment in pdf_attachments {
                let attachment_children = client.get_item_children(&attachment.key).await?;
                let attachment_annotations: Vec<ZoteroItem> = attachment_children
                    .into_iter()
                    .filter(|item| item.data.item_type == "annotation")
                    .collect();
                annotations.extend(attachment_annotations);
            }
        }

        annotations.truncate(limit);
        if annotations.is_empty() {
            return Ok("No annotations found for this item.".to_string());
        }

        let blocks: Vec<String> = annotations
            .iter()
            .enumerate()
            .map(|(idx, annotation)| {
                let info = annotation_info_from_item(annotation, Some(parent_title.clone()));
                format_annotation_block(&info, idx + 1)
            })
            .collect();

        return Ok(blocks.join("\n\n"));
    }

    // No item_key: get all annotations from library
    let mut params = HashMap::new();
    params.insert("itemType".to_string(), "annotation".to_string());
    params.insert("limit".to_string(), limit.to_string());
    let annotations = client.get_items(params).await?;

    if annotations.is_empty() {
        return Ok("No annotations found in library.".to_string());
    }

    let parent_title_by_attachment = resolve_annotation_parent_titles(client, &annotations).await;
    let blocks: Vec<String> = annotations
        .iter()
        .enumerate()
        .map(|(idx, annotation)| {
            let parent_attachment = annotation.data.parent_item.clone().unwrap_or_default();
            let parent_title = parent_title_by_attachment.get(&parent_attachment).cloned();
            let info = annotation_info_from_item(annotation, parent_title);
            format_annotation_block(&info, idx + 1)
        })
        .collect();

    Ok(blocks.join("\n\n"))
}

/// Handle the zotero_get_notes tool.
pub async fn handle_zotero_get_notes(client: &ZoteroClient, args: GetNotesArgs) -> String {
    match handle_zotero_get_notes_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_get_notes_inner(
    client: &ZoteroClient,
    args: GetNotesArgs,
) -> anyhow::Result<String> {
    let limit = normalize_limit(Some(args.limit), 20, 500) as usize;

    if let Some(query) = &args.query {
        let query = query.trim();
        if !query.is_empty() {
            let query_lower = query.to_lowercase();

            // Get note candidates
            let note_candidates: Vec<ZoteroItem> = if let Some(item_key) = &args.item_key {
                client
                    .get_item_children(item_key)
                    .await?
                    .into_iter()
                    .filter(|item| item.data.item_type == "note")
                    .take(2000)
                    .collect()
            } else {
                let mut params = HashMap::new();
                params.insert("q".to_string(), query.to_string());
                params.insert("qmode".to_string(), "everything".to_string());
                params.insert("itemType".to_string(), "note".to_string());
                params.insert("limit".to_string(), limit.to_string());
                client.get_items(params).await?
            };

            let matched_notes: Vec<ZoteroItem> = note_candidates
                .into_iter()
                .filter(|note| {
                    let content = clean_html(note.data.note.as_deref().unwrap_or(""), true);
                    content.to_lowercase().contains(&query_lower)
                })
                .collect();

            // Get annotation candidates
            let annotation_candidates: Vec<ZoteroItem> = if let Some(item_key) = &args.item_key {
                client
                    .get_item_children(item_key)
                    .await?
                    .into_iter()
                    .filter(|item| item.data.item_type == "annotation")
                    .take(5000)
                    .collect()
            } else {
                let mut params = HashMap::new();
                params.insert("itemType".to_string(), "annotation".to_string());
                client
                    .paginate(|p| client.get_items(p), params, Some(5000))
                    .await?
            };

            let matched_annotations: Vec<ZoteroItem> = annotation_candidates
                .into_iter()
                .filter(|annotation| {
                    let text = annotation
                        .data
                        .annotation_text
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase();
                    let comment = annotation
                        .data
                        .annotation_comment
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase();
                    text.contains(&query_lower) || comment.contains(&query_lower)
                })
                .collect();

            let total_matches = matched_notes.len() + matched_annotations.len();
            if total_matches == 0 {
                return Ok("No matching notes or annotations found.".to_string());
            }

            let note_parent_titles = resolve_note_parent_titles(client, &matched_notes).await;
            let annotation_parent_titles =
                resolve_annotation_parent_titles(client, &matched_annotations).await;

            let note_blocks: Vec<String> = matched_notes
                .iter()
                .take(limit)
                .enumerate()
                .map(|(idx, note)| {
                    let parent_key = note.data.parent_item.clone().unwrap_or_default();
                    let parent_title = note_parent_titles.get(&parent_key).cloned();
                    let content = truncate(
                        &clean_html(note.data.note.as_deref().unwrap_or(""), true),
                        500,
                    );

                    let mut lines: Vec<String> = Vec::new();
                    lines.push(format!("{}. [Note] {}", idx + 1, note.key));
                    if let Some(pt) = parent_title {
                        lines.push(format!("   - Parent: {}", pt));
                    }
                    lines.push(format!(
                        "   - Content: {}",
                        if content.is_empty() {
                            "(empty)"
                        } else {
                            &content
                        }
                    ));
                    lines.join("\n")
                })
                .collect();

            let remaining = limit.saturating_sub(note_blocks.len());
            let annotation_blocks: Vec<String> = matched_annotations
                .iter()
                .take(remaining)
                .enumerate()
                .map(|(idx, annotation)| {
                    let parent_attachment = annotation.data.parent_item.clone().unwrap_or_default();
                    let parent_title = annotation_parent_titles.get(&parent_attachment).cloned();
                    let info = annotation_info_from_item(annotation, parent_title);
                    format_annotation_block(&info, note_blocks.len() + idx + 1)
                })
                .collect();

            let mut output: Vec<String> = Vec::new();
            if !note_blocks.is_empty() {
                output.push("## Notes".to_string());
                output.push(note_blocks.join("\n\n"));
            }
            if !annotation_blocks.is_empty() {
                output.push("## Annotations".to_string());
                output.push(annotation_blocks.join("\n\n"));
            }

            return Ok(output.join("\n\n").trim().to_string());
        }
    }

    // No query: list notes for item or library
    let notes: Vec<ZoteroItem> = if let Some(item_key) = &args.item_key {
        client
            .get_item_children(item_key)
            .await?
            .into_iter()
            .filter(|item| item.data.item_type == "note")
            .take(limit)
            .collect()
    } else {
        let mut params = HashMap::new();
        params.insert("itemType".to_string(), "note".to_string());
        params.insert("limit".to_string(), limit.to_string());
        client.get_items(params).await?
    };

    if notes.is_empty() {
        return Ok("No notes found.".to_string());
    }

    let parent_titles = resolve_note_parent_titles(client, &notes).await;
    let blocks: Vec<String> = notes
        .iter()
        .enumerate()
        .map(|(idx, note)| {
            let cleaned = clean_html(note.data.note.as_deref().unwrap_or(""), true);
            let text = if args.truncate {
                truncate(&cleaned, 500)
            } else {
                cleaned
            };
            let parent_key = note.data.parent_item.clone().unwrap_or_default();
            let parent_title = parent_titles.get(&parent_key).cloned();
            let tags: Vec<String> = note
                .data
                .tags
                .as_ref()
                .map(|tags| {
                    tags.iter()
                        .map(|t| t.tag.clone())
                        .filter(|t| !t.is_empty())
                        .collect()
                })
                .unwrap_or_default();

            let mut lines: Vec<String> = Vec::new();
            lines.push(format!("{}. Key: {}", idx + 1, note.key));
            if let Some(pt) = parent_title {
                lines.push(format!("   - Parent: {}", pt));
            }
            lines.push(format!(
                "   - Tags: {}",
                if tags.is_empty() {
                    "(none)".to_string()
                } else {
                    tags.join(", ")
                }
            ));
            lines.push(format!(
                "   - Content: {}",
                if text.is_empty() { "(empty)" } else { &text }
            ));
            lines.join("\n")
        })
        .collect();

    Ok(blocks.join("\n\n"))
}

/// Handle the zotero_add_note tool.
pub async fn handle_zotero_add_note(client: &ZoteroClient, args: AddNoteArgs) -> String {
    match handle_zotero_add_note_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_add_note_inner(
    client: &ZoteroClient,
    args: AddNoteArgs,
) -> anyhow::Result<String> {
    if args.note_type == "note" {
        let item_key = args
            .item_key
            .ok_or_else(|| anyhow::anyhow!("item_key is required for type=note"))?;
        let note_title = args
            .note_title
            .ok_or_else(|| anyhow::anyhow!("note_title is required for type=note"))?;
        let note_text = args
            .note_text
            .ok_or_else(|| anyhow::anyhow!("note_text is required for type=note"))?;

        // Verify parent item exists
        client.get_item(&item_key).await?;

        let tags: Vec<ZoteroTag> =
            parse_str_list(args.tags.map(crate::shared::validators::StringOrList::List))
                .into_iter()
                .map(|tag| ZoteroTag {
                    tag,
                    tag_type: None,
                })
                .collect();

        let note_html = build_note_html(&note_title, &note_text);

        let note_data = crate::shared::types::ZoteroItemData {
            item_type: "note".to_string(),
            parent_item: Some(item_key),
            note: Some(note_html),
            tags: Some(tags),
            ..Default::default()
        };

        let response = client.create_items(&[note_data]).await?;
        let response_value = serde_json::to_value(&response)?;
        let status = handle_write_response(&response_value);
        if !status.ok {
            return Ok(format!("Error: {}", status.message));
        }
        return Ok(status.message);
    }

    // type=annotation
    let attachment_key = args
        .attachment_key
        .ok_or_else(|| anyhow::anyhow!("attachment_key is required for type=annotation"))?;
    let page = args
        .page
        .ok_or_else(|| anyhow::anyhow!("page is required for type=annotation"))?;
    let text = args
        .text
        .ok_or_else(|| anyhow::anyhow!("text is required for type=annotation"))?;

    // Verify attachment exists
    client.get_item(&attachment_key).await?;

    let annotation_position = serde_json::to_string(&serde_json::json!({
        "pageIndex": (page - 1).max(0),
        "rects": [[0, 0, 100, 100]]
    }))?;

    let tags = args.tags.map(|t| {
        t.into_iter()
            .filter(|s| !s.trim().is_empty())
            .map(|s| crate::shared::types::ZoteroTag {
                tag: s.trim().to_string(),
                tag_type: None,
            })
            .collect::<Vec<_>>()
    });

    let annotation_data = crate::shared::types::ZoteroItemData {
        item_type: "annotation".to_string(),
        parent_item: Some(attachment_key),
        annotation_type: Some("highlight".to_string()),
        annotation_text: Some(text),
        annotation_comment: Some(args.comment.unwrap_or_default()),
        annotation_color: Some(args.color),
        annotation_page_label: Some(page.to_string()),
        annotation_position: Some(annotation_position),
        tags,
        ..Default::default()
    };

    let response = client.create_items(&[annotation_data]).await?;
    let response_value = serde_json::to_value(&response)?;
    let status = handle_write_response(&response_value);
    if !status.ok {
        return Ok(format!("Error: {}", status.message));
    }
    Ok(status.message)
}
