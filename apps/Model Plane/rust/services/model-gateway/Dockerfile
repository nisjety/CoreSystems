# syntax=docker/dockerfile:1.4
#
# Build context: the Model Plane root (sibling of `rust/`, `go/`, `proto/`).
# Required because `crates/mp-contracts/build.rs` reads `../../../proto`
# relative to the crate dir.
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app
# `mp-contracts/build.rs` invokes tonic_build → protoc to compile the .proto
# tree at build time, so the protobuf compiler must be present in every
# stage that runs cargo (planner + builder).
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
RUN cargo build --release --bin model-gateway

FROM debian:trixie-slim AS runtime
# Trixie has GLIBC >= 2.38, matching the cargo-chef:latest-rust-1 builder.
# bookworm-slim ships GLIBC 2.36 and produced `version 'GLIBC_2.38' not found`.
# curl ships the binary the compose healthcheck calls (curl -f /healthz).
RUN apt-get update && apt-get install -y ca-certificates curl && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/rust/target/release/model-gateway /usr/local/bin/model-gateway
EXPOSE 8080 9090
ENTRYPOINT ["model-gateway"]
