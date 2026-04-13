FROM rust:1.85-slim-bookworm AS builder

WORKDIR /app

# Cargo.lock currently resolves crates that need a newer compiler than the base
# image ships, so install the minimum compatible toolchain while keeping the
# required builder image family.
RUN rustup toolchain install 1.88.0 && rustup default 1.88.0

# Cache dependency resolution before copying source.
COPY Cargo.toml Cargo.lock ./
RUN cargo fetch --locked

COPY src ./src
RUN cargo build --release --locked

FROM gcr.io/distroless/cc-debian12:nonroot

ENV RUST_LOG=info

# HTTP mode listens on PORT when MCP_TRANSPORT=http or --http is used.
EXPOSE 3000

COPY --from=builder /app/target/release/zotero-mcp-rs /zotero-mcp-rs

# Health checks are best handled by the orchestrator. For HTTP mode, probe
# the configured port/path externally; stdio mode has no meaningful in-image check.
ENTRYPOINT ["/zotero-mcp-rs"]
