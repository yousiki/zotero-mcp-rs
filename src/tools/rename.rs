use serde::Deserialize;
use rmcp::schemars::{self, JsonSchema};
use crate::clients::zotero::ZoteroClient;
use crate::shared::template_engine::build_renamed_filename;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RenameArgs {
    pub item_key: String,
}

pub async fn handle_zotero_rename_attachments(client: &ZoteroClient, args: RenameArgs) -> String {
    match rename_inner(client, args).await {
        Ok(s) => s,
        Err(e) => format!("Error: {}", e),
    }
}

async fn rename_inner(client: &ZoteroClient, args: RenameArgs) -> anyhow::Result<String> {
    let parent_key = args.item_key.trim();
    if parent_key.is_empty() {
        return Ok("item_key cannot be empty".to_string());
    }

    let parent = client.get_item(parent_key).await?;
    if parent.data.title.is_none() && parent.data.creators.as_ref().map_or(true, |c| c.is_empty()) {
        return Ok("Parent item has no metadata to generate a filename from".to_string());
    }

    let children = client.get_item_children(parent_key).await?;
    let attachments: Vec<_> = children.iter()
        .filter(|child| {
            child.data.item_type == "attachment"
                && child.data.filename.is_some()
                && child.data.content_type.as_deref() != Some("text/html")
        })
        .collect();

    if attachments.is_empty() {
        return Ok("No file attachments found for this item.".to_string());
    }

    let mut results = Vec::new();
    for attachment in &attachments {
        let old_name = attachment.data.filename.as_deref().unwrap_or("");
        let ext = old_name.rsplit('.').next().unwrap_or("pdf");
        let new_name = build_renamed_filename(&parent.data, ext);

        if new_name == old_name {
            results.push(format!("- {} (unchanged)", old_name));
            continue;
        }

        let mut updated_data = attachment.data.clone();
        updated_data.filename = Some(new_name.clone());
        updated_data.title = Some(new_name.clone());
        client.update_item(&attachment.key, &updated_data, attachment.version).await?;
        results.push(format!("- {} → {}", old_name, new_name));
    }

    Ok(format!("Renamed {} attachment(s):\n{}", attachments.len(), results.join("\n")))
}
