# syntax=docker/dockerfile:1.7
FROM rust:1.85-bookworm AS chef
WORKDIR /app
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=git \
    CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=120 \
    CARGO_HTTP_MULTIPLEXING=false
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    set -e; \
    for i in 1 2 3 4 5; do \
        if cargo install cargo-chef --locked --version 0.1.75; then \
            exit 0; \
        fi; \
        echo "cargo-chef install retry $i"; \
        sleep 8; \
    done; \
    exit 1

FROM chef AS planner
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release \
    && cp target/release/integration-webhook-normalizer /usr/local/bin/integration-webhook-normalizer

FROM debian:bookworm-slim

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && useradd --system --uid 10001 --create-home app
USER app
WORKDIR /app
COPY --from=builder /usr/local/bin/integration-webhook-normalizer /usr/local/bin/integration-webhook-normalizer

ENV PORT=3036
EXPOSE 3036

CMD ["integration-webhook-normalizer"]
