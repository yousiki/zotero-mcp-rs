use rmcp::{handler::server::wrapper::Parameters, schemars, tool, tool_router};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    #[schemars(description = "Search query text")]
    pub query: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ZoteroServer;

#[tool_router(server_handler)]
impl ZoteroServer {
    pub fn new() -> Self {
        Self
    }

    #[tool(description = "Unified Zotero search by text, tags, citation key, or advanced conditions.")]
    async fn zotero_search(&self, Parameters(args): Parameters<SearchArgs>) -> String {
        let _ = args.query;
        "stub".to_string()
    }
}
