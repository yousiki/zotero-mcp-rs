# Zotero MCP Server

A Model Context Protocol (MCP) server for Zotero, written in Rust. This server enables AI assistants like Claude Desktop and Cursor to interact with your Zotero library through a standardized interface.

## Features

- **15 MCP tools** for comprehensive Zotero library management
- **Dual transport**: stdio (default) and HTTP server modes
- **Auto-detection**: Automatically detects library ID from API key
- **WebDAV support**: Upload PDFs directly to your WebDAV server
- **Paper import**: Add papers by DOI, arXiv ID, or ISBN with automatic metadata extraction
- **Smart search**: Unified search across text, tags, citation keys, and advanced conditions

## Installation

### From Git

```bash
cargo install --git https://github.com/yousiki/zotero-mcp-rs
```

### From Source

```bash
git clone https://github.com/yousiki/zotero-mcp-rs.git
cd zotero-mcp-rs
cargo build --release
# Binary will be at target/release/zotero-mcp-rs
```

## Configuration

### Environment Variables

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

### Getting a Zotero API Key

1. Go to https://www.zotero.org/settings/keys
2. Click "Create new private key"
3. Grant appropriate permissions (read/write access to library)
4. Copy the generated key

## MCP Client Configuration

### Claude Desktop

Add to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "zotero": {
      "command": "zotero-mcp-rs",
      "env": {
        "ZOTERO_API_KEY": "your_api_key_here"
      }
    }
  }
}
```

### Cursor

Add to your `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "zotero": {
      "command": "zotero-mcp-rs",
      "env": {
        "ZOTERO_API_KEY": "your_api_key_here"
      }
    }
  }
}
```

### HTTP Mode

For HTTP transport (useful for remote access or containerized deployments):

```bash
MCP_TRANSPORT=http PORT=3000 zotero-mcp-rs
```

Then configure your MCP client to connect to `http://localhost:3000/mcp`.

## Available Tools

### Search

| Tool | Description |
|------|-------------|
| `zotero_search` | Unified search by text, tags, citation key, or advanced conditions |

### Items

| Tool | Description |
|------|-------------|
| `zotero_get_item` | Get item metadata, children, fulltext, and/or BibTeX |
| `zotero_update_item` | Update one item, or batch-update tags across many items |
| `zotero_delete_item` | Delete one or more items by key |

### Annotations & Notes

| Tool | Description |
|------|-------------|
| `zotero_get_annotations` | Get annotations for a specific item or across the library |
| `zotero_get_notes` | Retrieve notes, or search notes + annotations by query |
| `zotero_add_note` | Create a note or annotation on an item |

### Collections

| Tool | Description |
|------|-------------|
| `zotero_list_collections` | List, search, or retrieve items in a collection |
| `zotero_manage_collections` | Create/delete collections, or add/remove items |

### Library

| Tool | Description |
|------|-------------|
| `zotero_get_recent` | Get recently added items |
| `zotero_list_tags` | Get all tags in the library |
| `zotero_deduplicate` | Find or merge duplicate items |
| `fetch` | ChatGPT connector - return item text payload by key |

### Paper Management

| Tool | Description |
|------|-------------|
| `zotero_add_paper` | Add a paper by DOI or arXiv ID (with WebDAV) |
| `zotero_rename_attachments` | Rename PDF attachments using Zotero's naming template |

## Development

### Prerequisites

- Rust 1.75+ (edition 2021)
- Cargo

### Building

```bash
cargo build
```

### Running Tests

```bash
cargo test
```

### Linting

```bash
cargo clippy -- -D warnings
```

### Formatting

```bash
cargo fmt
```

## Architecture

The project follows a modular architecture:

- **clients/**: HTTP clients for Zotero API and WebDAV
- **services/**: Business logic (arXiv, CrossRef, PDF parsing, search)
- **tools/**: MCP tool handlers (one file per tool category)
- **shared/**: Common types, formatters, and utilities

See [AGENTS.md](AGENTS.md) for detailed architecture documentation and guides for adding new tools.

## License

MIT

## Acknowledgments

This project was inspired by [zotero-mcp](https://github.com/54yyyu/zotero-mcp) and references code from [zotero/translation-server](https://github.com/zotero/translation-server). Special thanks to [oh-my-openagent](https://github.com/code-yeongyu/oh-my-openagent) for the excellent opencode plugin, and to Xiaomi for their powerful [MIMO-V2-Pro model](https://mimo.xiaomi.com/mimo-v2-pro).
