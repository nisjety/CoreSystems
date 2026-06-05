# syntax=docker/dockerfile:1.4
# Build context: Model Plane root (sibling of `rust/`, `proto/`).
FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app
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
RUN cargo build --release --bin execution-core

FROM debian:trixie-slim AS runtime
# Trixie has GLIBC >= 2.38 to match the cargo-chef builder image. curl is
# needed by the compose healthcheck (curl -f /healthz). bubblewrap (`bwrap`)
# is REQUIRED for sandbox isolation: `sandbox::is_supported()` checks for it on
# PATH, and without it code execution degrades to an UNSANDBOXED passthrough —
# so it ships in the runtime image, not just dev hosts (chat-parity Phase 3 /
# capability-ownership-matrix §G1, Tier-2 sandbox isolation).
RUN set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends ca-certificates curl bubblewrap \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log; \
    rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/rust/target/release/execution-core /usr/local/bin/execution-core
EXPOSE 8080 9090
ENTRYPOINT ["execution-core"]
