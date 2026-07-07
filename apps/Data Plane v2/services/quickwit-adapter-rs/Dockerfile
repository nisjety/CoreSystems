# syntax=docker/dockerfile:1.7
# Build context: Data Plane v2 root.

FROM rust:1-bookworm AS chef
ARG DEBIAN_FRONTEND=noninteractive
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=git \
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
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    set -e; \
    for i in 1 2 3 4 5; do \
        if cargo install cargo-chef --locked --version 0.1.77; then \
            exit 0; \
        fi; \
        echo "cargo-chef install retry $i"; \
        sleep 8; \
    done; \
    exit 1
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
COPY infra/quickwit/ infra/quickwit/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/target,sharing=locked \
    cargo build --release --bin quickwit-adapter \
    && cp target/release/quickwit-adapter /usr/local/bin/quickwit-adapter

FROM debian:bookworm-slim AS runtime
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
RUN useradd --system --no-create-home --shell /usr/sbin/nologin dpv2
WORKDIR /app
COPY --from=builder /usr/local/bin/quickwit-adapter /usr/local/bin/quickwit-adapter
COPY --from=builder /build/infra/quickwit/ /app/infra/quickwit/
USER dpv2

EXPOSE 9204

HEALTHCHECK --interval=10s --timeout=5s --start-period=15s --retries=5 \
    CMD curl -sf http://127.0.0.1:9204/readyz || exit 1

ENTRYPOINT ["quickwit-adapter"]
