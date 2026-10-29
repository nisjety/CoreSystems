# syntax=docker/dockerfile:1.7
FROM lukemathwalker/cargo-chef:latest-rust-1.89-bookworm@sha256:0a0b3f65242ff7f1d5c3ea5c3ae10f4a1897204fedb8a4602651014f4302a129 AS chef
WORKDIR /app
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse \
    CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=120 \
    CARGO_HTTP_MULTIPLEXING=false
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
    && test "$(stat -c%s target/release/verevon-gateway-rs)" -gt 2000000 \
    && cp target/release/verevon-gateway-rs /usr/local/bin/verevon-gateway-rs

FROM debian:bookworm-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"
# Re-expose the build args as runtime ENV so the running process can answer
# GET /version (src/main.rs) without needing Docker/registry access to read
# the image label above.
ENV SOURCE_REVISION=${SOURCE_REVISION} \
    BUILD_DATE=${BUILD_DATE}
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive; \
    apt-get update -qq; \
    apt-get install -y -qq --no-install-recommends ca-certificates curl; \
    groupadd --gid 10001 appuser; \
    useradd --uid 10001 --gid appuser --home-dir /app --shell /usr/sbin/nologin appuser
WORKDIR /app
COPY --from=build /usr/local/bin/verevon-gateway-rs /usr/local/bin/verevon-gateway-rs
USER appuser
EXPOSE 3185
CMD ["verevon-gateway-rs"]
