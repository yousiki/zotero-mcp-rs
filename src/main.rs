use anyhow::Result;
use clap::Parser;
use crate::server::ZoteroServer;
use rmcp::{transport::stdio, ServiceExt};

mod clients;
mod config;
mod server;
mod services;
mod shared;
mod tools;

#[derive(Parser, Debug)]
#[command(name = "zotero-mcp-rs", about = "Zotero MCP server")]
struct Cli {
    /// Use stdio transport (default)
    #[arg(long, conflicts_with = "http")]
    stdio: bool,

    /// Use HTTP Streamable transport
    #[arg(long, conflicts_with = "stdio")]
    http: bool,

    /// Port for HTTP transport
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

    let server = ZoteroServer::new();
    server.serve(stdio()).await?.waiting().await?;
    Ok(())
}
