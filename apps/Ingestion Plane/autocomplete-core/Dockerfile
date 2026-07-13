# syntax=docker/dockerfile:1.7
FROM lukemathwalker/cargo-chef:latest-rust-1.97-bookworm@sha256:974d873357408b6ececac8729285204664e23f5aab21a34e353ae5bf9bb899a2 AS chef
WORKDIR /app
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=120 \
    CARGO_HTTP_MULTIPLEXING=false

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release \
    && cp target/release/autocomplete-core /usr/local/bin/autocomplete-core

FROM debian:bookworm-20260623-slim AS runtime
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.title="CoreSystem autocomplete-core" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends ca-certificates wget \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log; \
    useradd --system --uid 10001 --create-home autocomplete; \
    mkdir -p /data; \
    chown autocomplete:autocomplete /data

WORKDIR /app
COPY --from=builder /usr/local/bin/autocomplete-core /usr/local/bin/autocomplete-core

ENV AUTOCOMPLETE_HTTP_ADDR=0.0.0.0:3219
ENV AUTOCOMPLETE_METADATA_DB=/data/autocomplete-core.sqlite3

VOLUME ["/data"]
EXPOSE 3219

USER autocomplete
CMD ["autocomplete-core"]
