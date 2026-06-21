# syntax=docker/dockerfile:1.7
FROM rust:1.89-bookworm AS chef
WORKDIR /app
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo install cargo-chef

# Planner: produce the dependency recipe from the manifests only.
FROM chef AS planner
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# Builder: cook dependencies (cached on recipe), then build the real binary.
FROM chef AS build
COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
# target/ is a cache mount, so the compiled binary lives inside the cache and is
# NOT in the image layer — copy it out within the same RUN to a non-cache path.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release \
    && test "$(stat -c%s target/release/velion-gateway-rs)" -gt 2000000 \
    && cp target/release/velion-gateway-rs /usr/local/bin/velion-gateway-rs

FROM debian:bookworm-slim
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && apt-get install -y ca-certificates curl
WORKDIR /app
COPY --from=build /usr/local/bin/velion-gateway-rs /usr/local/bin/velion-gateway-rs
EXPOSE 3185
CMD ["velion-gateway-rs"]
