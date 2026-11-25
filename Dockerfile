# syntax=docker/dockerfile:1

# Multi-stage build for the `pamet` CLI. Default build is the SQLite-only,
# fully offline/dependency-free configuration (no system libsqlite/libpq/
# libssl needed — rusqlite's `bundled` feature compiles its own SQLite).
#
# Build extra capability in via FEATURES, e.g.:
#   docker build --build-arg FEATURES=postgres-backend,mcp -t pamet .
# Available: mcp, postgres-backend, baseline-hindsight (comma-separated,
# passed straight through to `cargo build --features`).

FROM rust:1-slim-bookworm AS builder
ARG FEATURES=""
WORKDIR /build

# Needed only when FEATURES pulls in postgres-backend/reqwest (libpq/openssl
# dev headers for linking) — installed unconditionally since it's cheap and
# keeps this Dockerfile working for any FEATURES value without edits.
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev libpq-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency compilation separately from source changes.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests

RUN set -eux; \
    if [ -n "$FEATURES" ]; then \
        cargo build --release --features "$FEATURES"; \
    else \
        cargo build --release; \
    fi; \
    mkdir -p /build/dist; \
    cp target/release/pamet /build/dist/; \
    cp target/release/pamet-mcp /build/dist/ 2>/dev/null || true

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libpq5 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 1000 pamet \
    && mkdir -p /data && chown pamet:pamet /data

# Copies `pamet` always, and `pamet-mcp` too when built with --build-arg FEATURES=mcp.
COPY --from=builder /build/dist/. /usr/local/bin/

USER pamet
WORKDIR /home/pamet
VOLUME ["/data"]
ENV PAMET_DB=/data/pamet.db

ENTRYPOINT ["pamet"]
CMD ["--db", "/data/pamet.db", "--help"]
