# syntax=docker/dockerfile:1.4
# Build context: Model Plane root (sibling of `rust/`, `proto/`).
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends protobuf-compiler && rm -rf /var/lib/apt/lists/*

FROM chef AS planner
COPY rust/ rust/
WORKDIR /app/rust
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
WORKDIR /app/rust
COPY --from=planner /app/rust/recipe.json recipe.json
COPY proto/ /app/proto/
RUN cargo chef cook --release --recipe-path recipe.json
WORKDIR /app
COPY rust/ rust/
WORKDIR /app/rust
RUN cargo build --release --bin inference-core

FROM debian:trixie-slim AS runtime
# Trixie has GLIBC >= 2.38 to match the cargo-chef builder image. curl is
# needed by the compose healthcheck (curl -f /healthz).
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/rust/target/release/inference-core /usr/local/bin/inference-core
EXPOSE 8082 9092
ENTRYPOINT ["inference-core"]
