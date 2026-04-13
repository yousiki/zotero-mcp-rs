use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::{
    ServerHandler, handler::server::wrapper::Parameters, model::*, tool, tool_handler, tool_router,
};

use crate::clients::webdav::WebDavClient;
use crate::clients::zotero::ZoteroClient;
use crate::tools::add_paper::{AddPaperArgs, handle_zotero_add_paper};
use crate::tools::annotations::{
    AddNoteArgs, GetAnnotationsArgs, GetNotesArgs, handle_zotero_add_note,
    handle_zotero_get_annotations, handle_zotero_get_notes,
};
use crate::tools::collections::{
    ListCollectionsArgs, ManageCollectionsArgs, handle_zotero_list_collections,
    handle_zotero_manage_collections,
};
use crate::tools::items::{
    ItemDeleteArgs, ItemGetArgs, ItemUpdateArgs, handle_zotero_delete_item, handle_zotero_get_item,
    handle_zotero_update_item,
};
use crate::tools::library::{
    DeduplicateArgs, FetchArgs, GetRecentArgs, ListTagsArgs, handle_fetch,
    handle_zotero_deduplicate, handle_zotero_get_recent, handle_zotero_list_tags,
};
use crate::tools::rename::{RenameArgs, handle_zotero_rename_attachments};
use crate::tools::search::{SearchArgs, handle_zotero_search};

#[derive(Clone)]
pub struct ZoteroServer {
    client: Arc<ZoteroClient>,
    webdav: Arc<Option<WebDavClient>>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl ZoteroServer {
    pub fn new(client: ZoteroClient, webdav: Option<WebDavClient>) -> Self {
        Self {
            client: Arc::new(client),
            webdav: Arc::new(webdav),
            tool_router: Self::tool_router(),
        }
    }

    // === Search (1 tool) ===
    #[tool(
        description = "Unified Zotero search by text, tags, citation key, or advanced conditions."
    )]
    async fn zotero_search(&self, Parameters(args): Parameters<SearchArgs>) -> String {
        handle_zotero_search(&self.client, args).await
    }

    // === Items (3 tools) ===
    #[tool(
        description = "Get item metadata, children, fulltext, and/or BibTeX sections for a Zotero item."
    )]
    async fn zotero_get_item(&self, Parameters(args): Parameters<ItemGetArgs>) -> String {
        handle_zotero_get_item(&self.client, args).await
    }

    #[tool(description = "Update one item, or batch-update tags across many items.")]
    async fn zotero_update_item(&self, Parameters(args): Parameters<ItemUpdateArgs>) -> String {
        handle_zotero_update_item(&self.client, args).await
    }

    #[tool(description = "Delete one or more Zotero items by key.")]
    async fn zotero_delete_item(&self, Parameters(args): Parameters<ItemDeleteArgs>) -> String {
        handle_zotero_delete_item(&self.client, &self.webdav, args).await
    }

    // === Annotations & Notes (3 tools) ===
    #[tool(
        description = "Get all annotations for a specific item or across your entire Zotero library."
    )]
    async fn zotero_get_annotations(
        &self,
        Parameters(args): Parameters<GetAnnotationsArgs>,
    ) -> String {
        handle_zotero_get_annotations(&self.client, args).await
    }

    #[tool(description = "Retrieve notes, or search notes + annotations when query is provided.")]
    async fn zotero_get_notes(&self, Parameters(args): Parameters<GetNotesArgs>) -> String {
        handle_zotero_get_notes(&self.client, args).await
    }

    #[tool(description = "Create either a Zotero note or annotation based on type.")]
    async fn zotero_add_note(&self, Parameters(args): Parameters<AddNoteArgs>) -> String {
        handle_zotero_add_note(&self.client, args).await
    }

    // === Collections (2 tools) ===
    #[tool(
        description = "List collections, search collections, or retrieve items in a collection."
    )]
    async fn zotero_list_collections(
        &self,
        Parameters(args): Parameters<ListCollectionsArgs>,
    ) -> String {
        handle_zotero_list_collections(&self.client, args).await
    }

    #[tool(description = "Create/delete collections, or add/remove items in a collection.")]
    async fn zotero_manage_collections(
        &self,
        Parameters(args): Parameters<ManageCollectionsArgs>,
    ) -> String {
        handle_zotero_manage_collections(&self.client, args).await
    }

    // === Library (4 tools) ===
    #[tool(description = "Get recently added items to your Zotero library.")]
    async fn zotero_get_recent(&self, Parameters(args): Parameters<GetRecentArgs>) -> String {
        handle_zotero_get_recent(&self.client, args).await
    }

    #[tool(description = "Get all tags used in your Zotero library.")]
    async fn zotero_list_tags(&self, Parameters(args): Parameters<ListTagsArgs>) -> String {
        handle_zotero_list_tags(&self.client, args).await
    }

    #[tool(description = "Find or merge duplicate items in your library.")]
    async fn zotero_deduplicate(&self, Parameters(args): Parameters<DeduplicateArgs>) -> String {
        handle_zotero_deduplicate(&self.client, args).await
    }

    #[tool(description = "Fetch item text payload by Zotero item key, DOI, or arXiv ID.")]
    async fn fetch(&self, Parameters(args): Parameters<FetchArgs>) -> String {
        handle_fetch(&self.client, args).await
    }

    // === Add Paper (1 tool) ===
    #[tool(
        description = "Add a paper to Zotero by URL, DOI, arXiv ID, ISBN, or local file path. Automatically extracts metadata and downloads PDF via WebDAV."
    )]
    async fn zotero_add_paper(&self, Parameters(args): Parameters<AddPaperArgs>) -> String {
        match self.webdav.as_ref() {
            Some(webdav) => handle_zotero_add_paper(&self.client, webdav, args).await,
            None => "WebDAV is not configured — add_paper requires WebDAV".to_string(),
        }
    }

    // === Rename (1 tool) ===
    #[tool(
        description = "Rename PDF/file attachments of a Zotero item using Zotero's naming template (default: 'firstCreator - year - title.ext')."
    )]
    async fn zotero_rename_attachments(&self, Parameters(args): Parameters<RenameArgs>) -> String {
        handle_zotero_rename_attachments(&self.client, args).await
    }
}

#[tool_handler]
impl ServerHandler for ZoteroServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_server_info(
            Implementation::new("zotero-mcp-rs", env!("CARGO_PKG_VERSION")),
        )
    }
}
