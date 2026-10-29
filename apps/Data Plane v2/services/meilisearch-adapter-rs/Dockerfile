# syntax=docker/dockerfile:1.7
# Build context: Data Plane v2 root.

FROM lukemathwalker/cargo-chef:latest-rust-1.97-bookworm@sha256:974d873357408b6ececac8729285204664e23f5aab21a34e353ae5bf9bb899a2 AS chef
ARG DEBIAN_FRONTEND=noninteractive
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=120 \
    CARGO_HTTP_MULTIPLEXING=false
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends \
        protobuf-compiler libprotobuf-dev libssl-dev pkg-config build-essential \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY proto/ proto/
COPY services/ services/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cooker
COPY --from=planner /build/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json

FROM cooker AS builder
COPY Cargo.toml Cargo.lock ./
COPY proto/ proto/
COPY services/ services/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release --bin meilisearch-adapter \
    && cp target/release/meilisearch-adapter /usr/local/bin/meilisearch-adapter

FROM debian:bookworm-slim AS runtime
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
ENV SOURCE_REVISION=$SOURCE_REVISION BUILD_DATE=$BUILD_DATE
ARG DEBIAN_FRONTEND=noninteractive
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends \
        ca-certificates curl libssl3 \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log
RUN groupadd --gid 10001 dpv2 \
    && useradd --uid 10001 --gid dpv2 --no-create-home --shell /usr/sbin/nologin dpv2
WORKDIR /app
COPY --from=builder /usr/local/bin/meilisearch-adapter /usr/local/bin/meilisearch-adapter
USER dpv2

EXPOSE 9205

HEALTHCHECK --interval=10s --timeout=5s --start-period=15s --retries=5 \
    CMD curl -sf http://127.0.0.1:9205/readyz || exit 1

ENTRYPOINT ["meilisearch-adapter"]
