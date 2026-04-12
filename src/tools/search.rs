use std::collections::{HashMap, HashSet};

use rmcp::schemars::{self, JsonSchema};
use serde::Deserialize;

use crate::clients::zotero::ZoteroClient;
use crate::services::search_engine::{
    compare_comparable, evaluate_condition, first_comparable_value, matches_tag_clauses,
    parse_tag_clause, read_citation_key_from_extra,
};
use crate::shared::formatters::format_item_result;
use crate::shared::types::SearchCondition;
use crate::shared::validators::{StringOrList, normalize_limit, parse_str_list};

/// Parameters for the zotero_search tool.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchArgs {
    /// Free-text search query (searches title, creator, year).
    pub query: Option<String>,
    /// Filter by tags. Accepts a single string (comma-separated) or array of strings.
    pub tags: Option<StringOrList>,
    /// Search by citation key (exact match in the Extra field).
    pub citation_key: Option<String>,
    /// Advanced search conditions (field/operation/value triples).
    pub conditions: Option<Vec<SearchCondition>>,
    /// How to combine multiple conditions: "all" (AND) or "any" (OR).
    #[serde(default = "default_join_mode")]
    pub join_mode: String,
    /// Field to sort results by (e.g. "title", "date", "dateModified").
    pub sort_by: Option<String>,
    /// Sort direction: "asc" or "desc".
    #[serde(default = "default_sort_dir")]
    pub sort_direction: String,
    /// Maximum number of results to return.
    #[serde(default = "default_limit")]
    pub limit: i64,
}

fn default_join_mode() -> String {
    "all".to_string()
}
fn default_sort_dir() -> String {
    "asc".to_string()
}
fn default_limit() -> i64 {
    10
}

/// Execute the zotero_search tool, returning formatted Markdown results.
pub async fn handle_zotero_search(client: &ZoteroClient, args: SearchArgs) -> String {
    match handle_zotero_search_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn handle_zotero_search_inner(
    client: &ZoteroClient,
    args: SearchArgs,
) -> anyhow::Result<String> {
    // Path 1: Citation key search
    if let Some(citekey) = &args.citation_key {
        let citekey = citekey.trim();
        if citekey.is_empty() {
            return Ok("citation_key cannot be empty".to_string());
        }
        let mut params = HashMap::new();
        params.insert("q".to_string(), citekey.to_string());
        params.insert("qmode".to_string(), "everything".to_string());
        params.insert("limit".to_string(), "200".to_string());
        params.insert("sort".to_string(), "dateModified".to_string());
        params.insert("direction".to_string(), "desc".to_string());
        let searched = client.get_items(params).await?;
        let matches: Vec<_> = searched
            .iter()
            .filter(|item| {
                let extra = item.data.extra.as_deref().unwrap_or("");
                read_citation_key_from_extra(extra)
                    .map(|k| k.to_lowercase() == citekey.to_lowercase())
                    .unwrap_or(false)
            })
            .collect();
        if matches.is_empty() {
            return Ok(format!("No items found for citation key '{}'.", citekey));
        }
        return Ok(matches
            .iter()
            .enumerate()
            .map(|(i, item)| format_item_result(item, Some(i + 1), true))
            .collect::<Vec<_>>()
            .join("\n\n"));
    }

    // Path 2: Advanced conditions
    if let Some(conditions) = &args.conditions
        && !conditions.is_empty()
    {
        let limit = normalize_limit(Some(args.limit), 50, 1000) as usize;
        let all_items = client
            .paginate(
                |params| client.get_items(params),
                HashMap::new(),
                Some(10000),
            )
            .await?;
        let mut matched: Vec<_> = all_items
            .iter()
            .filter(|item| {
                let outcomes: Vec<bool> = conditions
                    .iter()
                    .map(|cond| evaluate_condition(item, cond))
                    .collect();
                if args.join_mode == "all" {
                    outcomes.iter().all(|&b| b)
                } else {
                    outcomes.iter().any(|&b| b)
                }
            })
            .collect();
        if let Some(sort_field) = &args.sort_by {
            matched.sort_by(|a, b| {
                let va = first_comparable_value(a, sort_field);
                let vb = first_comparable_value(b, sort_field);
                let ord = compare_comparable(&va, &vb);
                if args.sort_direction == "asc" {
                    ord
                } else {
                    ord.reverse()
                }
            });
        }
        let sliced: Vec<_> = matched.into_iter().take(limit).collect();
        if sliced.is_empty() {
            return Ok("No items matched advanced search conditions.".to_string());
        }
        return Ok(sliced
            .iter()
            .enumerate()
            .map(|(i, item)| format_item_result(item, Some(i + 1), true))
            .collect::<Vec<_>>()
            .join("\n\n"));
    }

    // Path 3: Text + tag search
    let limit = normalize_limit(Some(args.limit), 10, 200) as usize;
    let tag_strs = parse_str_list(args.tags.clone());

    if !tag_strs.is_empty() && args.query.as_deref().unwrap_or("").trim().is_empty() {
        // Tag-only search: paginate all items, filter client-side
        let clauses: Vec<_> = tag_strs
            .iter()
            .map(|t| parse_tag_clause(t))
            .filter(|c| !c.include.is_empty() || !c.exclude.is_empty())
            .collect();
        if clauses.is_empty() {
            return Ok("tags clauses are empty".to_string());
        }
        let mut api_params = HashMap::new();
        api_params.insert("itemType".to_string(), "-attachment".to_string());
        let all_items = client
            .paginate(|params| client.get_items(params), api_params, Some(5000))
            .await?;
        let matches: Vec<_> = all_items
            .iter()
            .filter(|item| matches_tag_clauses(item, &clauses))
            .take(limit)
            .collect();
        if matches.is_empty() {
            return Ok("No items found with the specified tag conditions.".to_string());
        }
        return Ok(matches
            .iter()
            .enumerate()
            .map(|(i, item)| format_item_result(item, Some(i + 1), true))
            .collect::<Vec<_>>()
            .join("\n\n"));
    }

    // Text search via Zotero API
    let mut params = HashMap::new();
    params.insert(
        "q".to_string(),
        args.query.as_deref().unwrap_or("").trim().to_string(),
    );
    params.insert("qmode".to_string(), "titleCreatorYear".to_string());
    params.insert("itemType".to_string(), "-attachment".to_string());
    params.insert("sort".to_string(), "dateModified".to_string());
    params.insert("direction".to_string(), "desc".to_string());
    params.insert("limit".to_string(), limit.to_string());
    let items = client.get_items(params).await?;

    let lower_tags: Vec<String> = tag_strs.iter().map(|t| t.to_lowercase()).collect();
    let filtered: Vec<_> = if lower_tags.is_empty() {
        items.iter().collect()
    } else {
        items
            .iter()
            .filter(|item| {
                let item_tags: HashSet<String> = item
                    .data
                    .tags
                    .as_ref()
                    .map(|tags| tags.iter().map(|t| t.tag.to_lowercase()).collect())
                    .unwrap_or_default();
                lower_tags.iter().all(|tag| item_tags.contains(tag))
            })
            .collect()
    };

    if filtered.is_empty() {
        return Ok("No matching items found.".to_string());
    }
    Ok(filtered
        .iter()
        .enumerate()
        .map(|(i, item)| format_item_result(item, Some(i + 1), true))
        .collect::<Vec<_>>()
        .join("\n\n"))
}
