use std::collections::HashMap;

use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;
use serde_json::Value;

use crate::clients::zotero::ZoteroClient;
use crate::services::identifiers::resolve_collection_names;
use crate::shared::formatters::format_item_result;
use crate::shared::types::{ZoteroCollection, ZoteroCollectionData};
use crate::shared::validators::{
    dedupe_strings, handle_write_response, is_collection_key, normalize_limit, parse_str_list,
    StringOrList,
};

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

fn default_list_limit() -> i64 {
    100
}

/// Parameters for the zotero_list_collections tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListCollectionsArgs {
    /// Search collections by name (substring match).
    pub query: Option<String>,
    /// Retrieve a specific collection by key or name.
    pub collection_key: Option<String>,
    /// Include items in the collection output.
    #[serde(default)]
    pub include_items: bool,
    /// Maximum number of collections to return.
    #[serde(default = "default_list_limit")]
    pub limit: i64,
}

/// Parameters for the zotero_manage_collections tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ManageCollectionsArgs {
    /// Action to perform: "create", "add_items", "remove_items", "delete".
    pub action: String,
    /// Collection name (required for create).
    pub name: Option<String>,
    /// Parent collection key or name (optional for create).
    pub parent_collection: Option<String>,
    /// Collection key (required for delete, add_items, remove_items).
    pub collection_key: Option<String>,
    /// Item keys to add/remove (for add_items/remove_items).
    pub item_keys: Option<StringOrList>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve a collection reference (key or name) to a collection key.
async fn resolve_collection_key(client: &ZoteroClient, ref_: &str) -> anyhow::Result<String> {
    let trimmed = ref_.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!("collection reference cannot be empty"));
    }
    if is_collection_key(trimmed) {
        return Ok(trimmed.to_string());
    }
    let keys = resolve_collection_names(client, &[trimmed.to_string()]).await;
    if keys.is_empty() {
        return Err(anyhow::anyhow!("Collection not found: {}", trimmed));
    }
    Ok(keys[0].clone())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Handle the zotero_list_collections tool.
pub async fn handle_zotero_list_collections(client: &ZoteroClient, args: ListCollectionsArgs) -> String {
    match handle_zotero_list_collections_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_list_collections_inner(
    client: &ZoteroClient,
    args: ListCollectionsArgs,
) -> anyhow::Result<String> {
    let limit = normalize_limit(Some(args.limit), 100, 5000) as usize;

    // Case 1: Specific collection requested
    if let Some(collection_key) = &args.collection_key {
        let trimmed = collection_key.trim();
        if trimmed.is_empty() {
            return Err(anyhow::anyhow!("collection_key cannot be empty"));
        }

        let key = resolve_collection_key(client, trimmed).await?;
        let collection = client.get_collection(&key).await?;

        let mut params = HashMap::new();
        params.insert("itemType".to_string(), "-attachment".to_string());
        params.insert("sort".to_string(), "dateModified".to_string());
        params.insert("direction".to_string(), "desc".to_string());
        params.insert("limit".to_string(), normalize_limit(Some(args.limit), 50, 500).to_string());

        let items = client.get_collection_items(&key, params).await?;

        if items.is_empty() {
            return Ok(format!("Collection '{}' has no items.", collection.data.name));
        }

        let mut lines = vec![
            format!("# Collection: {}", collection.data.name),
            String::new(),
        ];

        for (index, item) in items.iter().enumerate() {
            lines.push(format_item_result(item, Some(index + 1), true));
        }

        return Ok(lines.join("\n\n"));
    }

    // Case 2: Search by query
    if let Some(query) = &args.query {
        let query_lower = query.trim().to_lowercase();
        if !query_lower.is_empty() {
            let params = HashMap::new();
            let collections = client
                .paginate(|p| client.get_collections(p), params, Some(limit))
                .await?;

            let mut matches: Vec<&ZoteroCollection> = collections
                .iter()
                .filter(|c| c.data.name.to_lowercase().contains(&query_lower))
                .collect();

            matches.sort_by(|a, b| a.data.name.to_lowercase().cmp(&b.data.name.to_lowercase()));

            if matches.is_empty() {
                return Ok("No collections matched.".to_string());
            }

            let lines: Vec<String> = matches
                .iter()
                .enumerate()
                .map(|(idx, c)| format!("{}. {} ({})", idx + 1, c.data.name, c.key))
                .collect();

            if args.include_items {
                // For now, just return the list (items would require additional API calls)
                return Ok(lines.join("\n"));
            }

            return Ok(lines.join("\n"));
        }
    }

    // Case 3: List all collections with hierarchical tree
    let params = HashMap::new();
    let collections = client
        .paginate(|p| client.get_collections(p), params, Some(limit))
        .await?;

    if collections.is_empty() {
        return Ok("No collections found.".to_string());
    }

    // Build lookup map
    let by_key: HashMap<String, &ZoteroCollection> = collections
        .iter()
        .map(|c| (c.key.clone(), c))
        .collect();

    // Build parent → children map
    let mut children_by_parent: HashMap<String, Vec<&ZoteroCollection>> = HashMap::new();
    let mut roots: Vec<&ZoteroCollection> = Vec::new();

    for collection in &collections {
        if let Some(parent_key) = collection.data.parent_collection_key() {
            if by_key.contains_key(parent_key) {
                children_by_parent
                    .entry(parent_key.to_string())
                    .or_default()
                    .push(collection);
            } else {
                roots.push(collection);
            }
        } else {
            roots.push(collection);
        }
    }

    // Sort children and roots alphabetically
    for children in children_by_parent.values_mut() {
        children.sort_by(|a, b| a.data.name.to_lowercase().cmp(&b.data.name.to_lowercase()));
    }
    roots.sort_by(|a, b| a.data.name.to_lowercase().cmp(&b.data.name.to_lowercase()));

    // Walk tree and format with indentation
    let mut lines: Vec<String> = Vec::new();

    fn walk<'a>(
        collection: &'a ZoteroCollection,
        depth: usize,
        children_by_parent: &HashMap<String, Vec<&'a ZoteroCollection>>,
        lines: &mut Vec<String>,
    ) {
        let indent = "  ".repeat(depth);
        lines.push(format!("{}- {} ({})", indent, collection.data.name, collection.key));

        if let Some(children) = children_by_parent.get(&collection.key) {
            for child in children {
                walk(child, depth + 1, children_by_parent, lines);
            }
        }
    }

    for root in &roots {
        walk(root, 0, &children_by_parent, &mut lines);
    }

    Ok(lines.join("\n"))
}

/// Handle the zotero_manage_collections tool.
pub async fn handle_zotero_manage_collections(
    client: &ZoteroClient,
    args: ManageCollectionsArgs,
) -> String {
    match handle_zotero_manage_collections_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_manage_collections_inner(
    client: &ZoteroClient,
    args: ManageCollectionsArgs,
) -> anyhow::Result<String> {
    match args.action.as_str() {
        "create" => {
            let name = args.name.as_deref().unwrap_or("").trim().to_string();
            if name.is_empty() {
                return Err(anyhow::anyhow!("name is required for action=create"));
            }

            let parent_collection: Value = if let Some(parent_ref) = &args.parent_collection {
                let key = resolve_collection_key(client, parent_ref).await?;
                Value::String(key)
            } else {
                Value::Bool(false)
            };

            let data = ZoteroCollectionData {
                name: name.clone(),
                parent_collection: Some(parent_collection),
                ..Default::default()
            };

            let response = client.create_collections(&[data]).await?;
            let status = handle_write_response(&serde_json::to_value(&response)?);

            if !status.ok {
                return Err(anyhow::anyhow!("{}", status.message));
            }

            let created_key = status
                .data
                .as_ref()
                .and_then(|d| d.get("success"))
                .and_then(|s| s.as_object())
                .and_then(|obj| obj.values().next())
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if created_key.is_empty() {
                Ok(format!("Collection created: {}", name))
            } else {
                Ok(format!("Collection created: {} ({})", name, created_key))
            }
        }

        "delete" => {
            let key = args
                .collection_key
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_string();
            if key.is_empty() {
                return Err(anyhow::anyhow!("collection_key is required for action=delete"));
            }

            let collection = client.get_collection(&key).await?;
            let version = collection.version;

            let deleted = client.delete_collection(&key, version).await?;
            if !deleted {
                return Err(anyhow::anyhow!("Failed to delete collection"));
            }

            Ok(format!(
                "Collection deleted: {} ({})",
                collection.data.name, key
            ))
        }

        "add_items" | "remove_items" => {
            let collection_key = args
                .collection_key
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_string();
            if collection_key.is_empty() {
                return Err(anyhow::anyhow!(
                    "collection_key is required for add_items/remove_items"
                ));
            }

            let item_keys = dedupe_strings(parse_str_list(args.item_keys));
            if item_keys.is_empty() {
                return Err(anyhow::anyhow!("item_keys requires at least one value"));
            }

            let mut changed = 0usize;
            for item_key in &item_keys {
                if args.action == "add_items" {
                    client.add_to_collection(&collection_key, item_key).await?;
                } else {
                    client
                        .remove_from_collection(&collection_key, item_key)
                        .await?;
                }
                changed += 1;
            }

            Ok(format!(
                "Action: {}\nCollection: {}\nItems requested: {}\nCollection links changed: {}",
                args.action,
                collection_key,
                item_keys.len(),
                changed
            ))
        }

        _ => Err(anyhow::anyhow!(
            "Invalid action: {}. Must be one of: create, delete, add_items, remove_items",
            args.action
        )),
    }
}
