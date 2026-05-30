# syntax=docker/dockerfile:1.7
# Build context: Data Plane v2 root.

FROM rust:1-bookworm AS chef
RUN apt-get update && apt-get install -y --no-install-recommends \
        protobuf-compiler libprotobuf-dev libssl-dev pkg-config build-essential \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked --version 0.1.71
WORKDIR /build

FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY proto/ proto/
COPY services/ services/
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cooker
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM cooker AS builder
COPY Cargo.toml Cargo.lock ./
COPY proto/ proto/
COPY services/ services/
COPY infra/quickwit/ infra/quickwit/
RUN cargo build --release --bin quickwit-adapter

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl libssl3 \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin dpv2
WORKDIR /app
COPY --from=builder /build/target/release/quickwit-adapter /usr/local/bin/quickwit-adapter
COPY --from=builder /build/infra/quickwit/ /app/infra/quickwit/
USER dpv2

EXPOSE 9204

HEALTHCHECK --interval=10s --timeout=5s --start-period=15s --retries=5 \
    CMD curl -sf http://127.0.0.1:9204/readyz || exit 1

ENTRYPOINT ["quickwit-adapter"]
