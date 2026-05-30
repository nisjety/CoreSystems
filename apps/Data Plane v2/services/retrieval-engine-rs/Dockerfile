# syntax=docker/dockerfile:1.7
#
# Multi-stage build for retrieval-engine-rs.
#
# Stage 1 (chef-planner): generates a recipe.json describing all workspace deps.
# Stage 2 (chef-cooker):  builds dependencies only, cached as a Docker layer.
# Stage 3 (builder):      copies source + builds the binary.
# Stage 4 (runtime):      slim Debian with the compiled binary + ca-certs.
#
# Uses cargo-chef so rebuilds on source-only changes skip dep recompilation
# (10-100x faster than naive `cargo build` on every layer change).
# Build context: project root (so the entire workspace is available).

FROM rust:1-bookworm AS chef
RUN apt-get update && apt-get install -y --no-install-recommends \
        protobuf-compiler libprotobuf-dev libssl-dev pkg-config build-essential \
    && rm -rf /var/lib/apt/lists/*
# tonic-build invokes protoc, which needs to resolve `google/protobuf/*.proto`
# imports. libprotobuf-dev installs them under /usr/include/google/protobuf/.
ENV PROTOC_INCLUDE=/usr/include
RUN cargo install cargo-chef --locked --version 0.1.71
WORKDIR /build

# ─── Stage 1: planner ──────────────────────────────────────────────────────
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY proto/ proto/
COPY services/ services/
RUN cargo chef prepare --recipe-path recipe.json

# ─── Stage 2: cook deps (cached layer) ─────────────────────────────────────
FROM chef AS cooker
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# ─── Stage 3: build binary ─────────────────────────────────────────────────
FROM cooker AS builder
COPY Cargo.toml Cargo.lock ./
COPY proto/ proto/
COPY services/ services/
RUN cargo build --release --bin retrieval-engine

# ─── Stage 4: runtime ──────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Run as non-root for defense in depth.
RUN useradd --system --no-create-home --shell /usr/sbin/nologin dpv2
USER dpv2

COPY --from=builder /build/target/release/retrieval-engine /usr/local/bin/retrieval-engine

EXPOSE 8004 50052

# Health check is intentionally HTTP-only; readyz checks Postgres + Qdrant + Redis.
HEALTHCHECK --interval=10s --timeout=5s --start-period=15s --retries=5 \
    CMD curl -sf http://127.0.0.1:8004/readyz || exit 1

ENTRYPOINT ["retrieval-engine"]
