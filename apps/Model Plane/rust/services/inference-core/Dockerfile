# syntax=docker/dockerfile:1.7
# Build context: Model Plane root (sibling of `rust/`, `proto/`).
FROM lukemathwalker/cargo-chef:latest-rust-1.97-bookworm@sha256:974d873357408b6ececac8729285204664e23f5aab21a34e353ae5bf9bb899a2 AS chef
WORKDIR /app
ENV CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=120 \
    CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends protobuf-compiler libprotobuf-dev \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log
FROM chef AS planner
COPY rust/ rust/
WORKDIR /app/rust
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
WORKDIR /app/rust
COPY --from=planner /app/rust/recipe.json recipe.json
COPY proto/ /app/proto/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/rust/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
WORKDIR /app
COPY rust/ rust/
WORKDIR /app/rust
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/rust/target,sharing=locked \
    cargo build --release --bin inference-core \
    && cp target/release/inference-core /usr/local/bin/inference-core

FROM debian:bookworm-slim AS runtime
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
# Re-expose the build args as runtime ENV so the running process can answer
# GET /version (src/http_health.rs) without needing Docker/registry access to
# read the image label above.
ENV SOURCE_REVISION=${SOURCE_REVISION} \
    BUILD_DATE=${BUILD_DATE}
# Builder and runtime share Debian Bookworm's glibc ABI. curl is needed by the
# compose healthcheck (curl -f /healthz).
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends ca-certificates curl \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin modelplane
COPY --from=builder /usr/local/bin/inference-core /usr/local/bin/inference-core
USER modelplane
EXPOSE 8082 9092
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=5 \
    CMD curl -fsS http://127.0.0.1:8082/healthz >/dev/null || exit 1
ENTRYPOINT ["inference-core"]
