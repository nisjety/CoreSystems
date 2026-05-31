# syntax=docker/dockerfile:1.4
#
# Build context: the Model Plane root (sibling of `rust/`, `go/`, `proto/`).
# Required because `crates/mp-contracts/build.rs` reads `../../../proto`
# relative to the crate dir.
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app
# `mp-contracts/build.rs` invokes tonic_build → protoc to compile the .proto
# tree at build time, so protoc and the Google well-known proto includes must
# be present in every stage that runs cargo (planner + builder).
RUN set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends protobuf-compiler libprotobuf-dev \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log; \
    rm -rf /var/lib/apt/lists/*

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
RUN set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends ca-certificates curl \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log; \
    rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/rust/target/release/model-gateway /usr/local/bin/model-gateway
EXPOSE 8080 9090
ENTRYPOINT ["model-gateway"]
