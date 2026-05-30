# syntax=docker/dockerfile:1.7
# Multi-stage build for embedding-engine-rs (Model Plane embeddings + Qdrant writer).
# See services/retrieval-engine-rs/Dockerfile for design notes.
# Build context: project root.

FROM rust:1-bookworm AS chef
RUN apt-get update && apt-get install -y --no-install-recommends \
        protobuf-compiler libprotobuf-dev libssl-dev pkg-config build-essential \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked --version 0.1.71
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY proto/ proto/
COPY services/ services/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cooker
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM cooker AS builder
COPY Cargo.toml Cargo.lock ./
COPY proto/ proto/
COPY services/ services/
RUN cargo build --release --bin embedding-engine

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin dpv2
USER dpv2

COPY --from=builder /build/target/release/embedding-engine /usr/local/bin/embedding-engine

EXPOSE 9202

HEALTHCHECK --interval=10s --timeout=5s --start-period=15s --retries=5 \
    CMD curl -sf http://127.0.0.1:9202/readyz || exit 1

ENTRYPOINT ["embedding-engine"]
