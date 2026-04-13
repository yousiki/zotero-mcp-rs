# syntax=docker/dockerfile:1
FROM rust:slim-bookworm AS builder

WORKDIR /app

# Copy manifest files first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Fetch dependencies (cached layer if Cargo.toml/Cargo.lock unchanged)
RUN cargo fetch --locked

# Copy source code
COPY src ./src

# Build with cache mounts for faster incremental builds
# - /usr/local/cargo/registry: downloaded crate sources
# - /usr/local/cargo/git/db: git dependency checkouts
# - /app/target: compiled artifacts
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git/db \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked && \
    cp /app/target/release/zotero-mcp-rs /zotero-mcp-rs

FROM gcr.io/distroless/cc:nonroot

ENV RUST_LOG=info

# HTTP mode listens on PORT when MCP_TRANSPORT=http or --http is used.
EXPOSE 3000

COPY --from=builder /zotero-mcp-rs /zotero-mcp-rs

# Health checks are best handled by the orchestrator. For HTTP mode, probe
# the configured port/path externally; stdio mode has no meaningful in-image check.
ENTRYPOINT ["/zotero-mcp-rs"]
