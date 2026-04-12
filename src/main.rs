use anyhow::Result;
use clap::Parser;
use rmcp::{transport::stdio, ServiceExt};

use crate::server::ZoteroServer;
use crate::clients::zotero::ZoteroClient;
use crate::clients::webdav::create_webdav_client;
use crate::config::Config;

mod clients;
mod config;
mod server;
mod services;
mod shared;
mod tools;

#[derive(Parser, Debug)]
#[command(name = "zotero-mcp-rs", about = "Zotero MCP server")]
struct Cli {
    #[arg(long, conflicts_with = "http")]
    stdio: bool,
    #[arg(long, conflicts_with = "stdio")]
    http: bool,
    #[arg(long, default_value = "3000", env = "PORT")]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let _cli = Cli::parse();
    let config = Config::from_env().map_err(|e| anyhow::anyhow!(e))?;

    // Library auto-detect
    let (library_id, library_type) = if let Some(id) = &config.zotero_library_id {
        (id.clone(), config.zotero_library_type.clone())
    } else {
        // Call /keys/current to get user ID
        let client = reqwest::Client::new();
        let resp = client.get("https://api.zotero.org/keys/current")
            .header("Zotero-API-Key", &config.zotero_api_key)
            .header("Zotero-API-Version", "3")
            .send().await?;
        let data: serde_json::Value = resp.json().await?;
        let user_id = data.get("userID")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow::anyhow!("Failed to auto-detect library ID from API key"))?;
        eprintln!("Auto-detected library: user/{}", user_id);
        (user_id.to_string(), "user".to_string())
    };

    let client = ZoteroClient::new(&config.zotero_api_key, &library_id, &library_type);
    let webdav = create_webdav_client(
        config.webdav_url.as_deref(),
        config.webdav_username.as_deref(),
        config.webdav_password.as_deref(),
    );

    eprintln!("Zotero MCP server started (library: {}/{}, transport: stdio)", library_type, library_id);

    let server = ZoteroServer::new(client, webdav);
    server.serve(stdio()).await?.waiting().await?;
    Ok(())
}
