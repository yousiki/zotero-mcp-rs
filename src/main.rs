use anyhow::Result;
use crate::server::ZoteroServer;
use rmcp::{transport::stdio, ServiceExt};

mod clients;
mod config;
mod server;
mod services;
mod shared;
mod tools;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let _scaffold = (
        config::Config {},
        clients::zotero::ZoteroClient,
        clients::webdav::WebDavClient,
    );

    let server = ZoteroServer::new();
    server.serve(stdio()).await?.waiting().await?;
    Ok(())
}
