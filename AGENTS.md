# AGENTS.md - Zotero MCP Server (Rust)

## Runtime & Toolchain

- **Language**: Rust (edition 2021)
- **Build**: `cargo build`, `cargo build --release`
- **Test**: `cargo test`
- **Lint**: `cargo clippy -- -D warnings`
- **Format**: `cargo fmt`
- **Binary**: `zotero-mcp-rs`

## Project Structure

```
zotero-mcp-rs/
├── src/
│   ├── main.rs           # Entry point, CLI parsing, transport setup
│   ├── server.rs         # MCP server handler, tool registration
│   ├── config.rs         # Environment variable configuration
│   ├── clients/          # HTTP API clients
│   │   ├── mod.rs
│   │   ├── zotero.rs     # Zotero API client
│   │   └── webdav.rs     # WebDAV client for file uploads
│   ├── services/         # Business logic services
│   │   ├── mod.rs
│   │   ├── arxiv.rs      # arXiv API integration
│   │   ├── crossref.rs   # CrossRef API integration
│   │   ├── identifiers.rs # DOI/ISBN/arXiv identifier parsing
│   │   ├── oa_sources.rs # Open Access source detection
│   │   ├── pdf.rs        # PDF metadata extraction
│   │   └── search_engine.rs # Unified search engine
│   ├── tools/            # MCP tool handlers
│   │   ├── mod.rs
│   │   ├── search.rs     # zotero_search tool
│   │   ├── items.rs      # zotero_get_item, zotero_update_item, zotero_delete_item
│   │   ├── annotations.rs # zotero_get_annotations, zotero_get_notes, zotero_add_note
│   │   ├── collections.rs # zotero_list_collections, zotero_manage_collections
│   │   ├── library.rs    # zotero_get_recent, zotero_list_tags, zotero_deduplicate, fetch
│   │   ├── add_paper.rs  # zotero_add_paper
│   │   └── rename.rs     # zotero_rename_attachments
│   └── shared/           # Shared types and utilities
│       ├── mod.rs
│       ├── types.rs      # Common types and enums
│       ├── formatters.rs # Output formatting
│       ├── template_engine.rs # File naming templates
│       └── validators.rs # Input validation
├── Cargo.toml
├── Cargo.lock
└── .gitignore
```

## Code Conventions

### Error Handling
- Use `thiserror` for custom error types
- Use `anyhow` for application-level error handling
- Errors should be descriptive and actionable

### Logging
- Use `tracing` for structured logging
- Use `tracing-subscriber` with env-filter for log level control
- Log to stderr (not stdout) to avoid interfering with stdio transport

### JSON Handling
- Use `serde` with `derive` feature for serialization
- Use `serde_json` for JSON parsing
- Use `schemars` for JSON Schema generation (via rmcp's `JsonSchema` derive)

### Dependencies
- `rmcp` - MCP protocol implementation
- `tokio` - Async runtime
- `reqwest` - HTTP client
- `axum` - HTTP server (for HTTP transport)
- `clap` - CLI argument parsing
- `quick-xml` - XML parsing (Zotero API responses)
- `regex` - Pattern matching
- `phf` - Perfect hash functions for static maps

## Architecture

### clients/
HTTP clients for external APIs:
- `ZoteroClient`: Handles all Zotero API requests (items, collections, tags, etc.)
- `WebDavClient`: Handles file uploads to WebDAV servers

### services/
Business logic that doesn't directly interact with MCP:
- Identifier parsing (DOI, arXiv, ISBN)
- Metadata extraction from PDFs
- Crossref/arXiv API integration for paper metadata
- Search engine for unified querying

### tools/
MCP tool handlers. Each tool:
1. Defines an args struct with `#[derive(Deserialize, JsonSchema)]`
2. Implements a handler function returning `String`
3. Is registered in `server.rs` with `#[tool]` attribute

### shared/
Common types, formatters, and utilities used across modules.

## How to Add a New Tool

### 1. Create Args Struct

In the appropriate `tools/*.rs` file:

```rust
use serde::Deserialize;
use schemars::JsonSchema;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MyToolArgs {
    /// Description of the parameter
    #[schemars(description = "The item key to fetch")]
    pub key: String,
    
    /// Optional parameter
    #[schemars(description = "Whether to include children items")]
    #[serde(default)]
    pub include_children: bool,
}
```

### 2. Implement Handler Function

```rust
use crate::clients::zotero::ZoteroClient;

pub async fn handle_my_tool(
    client: &ZoteroClient,
    args: MyToolArgs,
) -> String {
    match client.get_item(&args.key).await {
        Ok(item) => format!("Item: {}", item.title),
        Err(e) => format!("Error: {}", e),
    }
}
```

### 3. Register in server.rs

```rust
use crate::tools::my_module::{MyToolArgs, handle_my_tool};

#[tool(description = "Description of what this tool does.")]
async fn my_tool(&self, Parameters(args): Parameters<MyToolArgs>) -> String {
    handle_my_tool(&self.client, args).await
}
```

The `#[tool]` macro automatically:
- Generates JSON Schema from the args struct
- Registers the tool with the MCP server
- Handles parameter deserialization

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `ZOTERO_API_KEY` | Yes | - | Zotero API key from https://www.zotero.org/settings/keys |
| `ZOTERO_LIBRARY_ID` | No | Auto-detected | Library ID (user ID or group ID) |
| `ZOTERO_LIBRARY_TYPE` | No | `user` | Library type: `user` or `group` |
| `WEBDAV_URL` | No | - | WebDAV server URL for file uploads |
| `WEBDAV_USERNAME` | No | - | WebDAV username |
| `WEBDAV_PASSWORD` | No | - | WebDAV password |
| `PORT` | No | `3000` | HTTP server port (HTTP transport only) |
| `MCP_TRANSPORT` | No | `stdio` | Transport mode: `stdio` or `http` |
| `RUST_LOG` | No | - | Log level filter (e.g., `info`, `debug`) |

## Testing

### Unit Tests
```bash
cargo test
```

### Integration Tests
Integration tests use `wiremock` to mock HTTP responses:

```rust
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path};

#[tokio::test]
async fn test_get_item() {
    let mock_server = MockServer::start().await;
    
    Mock::given(method("GET"))
        .and(path("/items/ABC123"))
        .respond_with(ResponseTemplate::new(200)
            .set_body_string(r#"{"key":"ABC123","title":"Test"}"#))
        .mount(&mock_server)
        .await;
    
    // Test with mock_server.uri()
}
```

### Running Specific Tests
```bash
cargo test test_name
cargo test --lib
cargo test --test integration_test
```

## Common Patterns

### Tool Response Format
Tools return formatted strings, not JSON. Use consistent formatting:

```rust
// For single items
format!("Title: {}\nAuthors: {}\nYear: {}", title, authors, year)

// For lists
items.iter()
    .map(|item| format!("• {} ({})", item.title, item.year))
    .collect::<Vec<_>>()
    .join("\n")
```

### Error Responses
Return error messages as strings (not panics):

```rust
match result {
    Ok(data) => format_response(data),
    Err(e) => format!("Error: {}", e),
}
```

### Async Operations
All tool handlers are async. Use `tokio` for concurrent operations:

```rust
let (result1, result2) = tokio::join!(
    client.get_item(&key1),
    client.get_item(&key2),
);
```
