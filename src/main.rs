use anyhow::Result;
use clap::Parser;
use rmcp::{ServiceExt, transport::stdio};

use crate::clients::webdav::create_webdav_client;
use crate::clients::zotero::ZoteroClient;
use crate::config::{Config, TransportMode};
use crate::server::ZoteroServer;

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

    let cli = Cli::parse();
    let config = Config::from_env().map_err(|e| anyhow::anyhow!(e))?;

    // Library auto-detect
    let (library_id, library_type) = if let Some(id) = &config.zotero_library_id {
        (id.clone(), config.zotero_library_type.clone())
    } else {
        // Call /keys/current to get user ID
        let client = reqwest::Client::new();
        let resp = client
            .get("https://api.zotero.org/keys/current")
            .header("Zotero-API-Key", &config.zotero_api_key)
            .header("Zotero-API-Version", "3")
            .send()
            .await?;
        let data: serde_json::Value = resp.json().await?;
        let user_id = data
            .get("userID")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow::anyhow!("Failed to auto-detect library ID from API key"))?;
        eprintln!("Auto-detected library: user/{}", user_id);
        (user_id.to_string(), "user".to_string())
    };

    let zotero_client = ZoteroClient::new(&config.zotero_api_key, &library_id, &library_type);
    let webdav = create_webdav_client(
        config.webdav_url.as_deref(),
        config.webdav_username.as_deref(),
        config.webdav_password.as_deref(),
    );

    // Determine transport: CLI flag > env > default (stdio)
    let transport = if cli.http {
        TransportMode::Http
    } else if cli.stdio {
        TransportMode::Stdio
    } else {
        config.transport.clone()
    };

    let port = if cli.http { cli.port } else { config.port };

    match transport {
        TransportMode::Stdio => {
            eprintln!(
                "Zotero MCP server started (library: {}/{}, transport: stdio)",
                library_type, library_id
            );
            let server = ZoteroServer::new(zotero_client, webdav);
            server.serve(stdio()).await?.waiting().await?;
        }
        TransportMode::Http => {
            eprintln!("Zotero MCP server listening on http://0.0.0.0:{}", port);
            // HTTP transport using rmcp's StreamableHttpService
            use rmcp::transport::streamable_http_server::{
                StreamableHttpServerConfig, StreamableHttpService,
                session::local::LocalSessionManager,
            };

            let service = StreamableHttpService::new(
                {
                    let zotero_client = zotero_client.clone();
                    let webdav = webdav.clone();
                    move || Ok(ZoteroServer::new(zotero_client.clone(), webdav.clone()))
                },
                std::sync::Arc::new(LocalSessionManager::default()),
                StreamableHttpServerConfig::default(),
            );

            let router = axum::Router::new().nest_service("/mcp", service);
            let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
            axum::serve(listener, router).await?;
        }
    }

    Ok(())
}
