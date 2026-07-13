# syntax=docker/dockerfile:1.7
FROM lukemathwalker/cargo-chef:latest-rust-1.85.1-bookworm@sha256:db2532d7c4ef39a0e7f4d48fa15eee2c6db12567baacb12823a42462346a9b11 AS chef
WORKDIR /app
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=120 \
    CARGO_HTTP_MULTIPLEXING=false
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

FROM debian:bookworm-20260623-slim
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.title="CoreSystem integration webhook normalizer" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && useradd --system --uid 10001 --create-home app \
    && rm -rf /var/lib/apt/lists/*
USER app
WORKDIR /app
COPY --from=builder /usr/local/bin/integration-webhook-normalizer /usr/local/bin/integration-webhook-normalizer

ENV PORT=3036
EXPOSE 3036

CMD ["integration-webhook-normalizer"]
