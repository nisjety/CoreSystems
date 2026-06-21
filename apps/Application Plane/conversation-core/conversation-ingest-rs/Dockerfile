# syntax=docker/dockerfile:1.7
# rust:1 = latest stable 1.x (matches the other planes' base + satisfies deps
# like icu_*/idna_adapter that now require rustc >= 1.86; the old 1.85 pin
# broke `cargo build --release` here with "rustc 1.85.1 is not supported").

# ---- chef: pin cargo-chef once on the shared toolchain image ----------------
FROM rust:1-bookworm AS chef
WORKDIR /src
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo install cargo-chef --locked

# ---- planner: compute the dependency recipe ---------------------------------
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# ---- build: cook deps (cached) then compile the crate ------------------------
FROM chef AS build
COPY --from=planner /src/recipe.json recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# target/ is a cache mount, so the compiled binary lives INSIDE the cache and is
# NOT in the image layer; copy it out to a non-cache path within the same RUN.
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release \
    && cp target/release/conversation-ingest-rs /usr/local/bin/conversation-ingest-rs

FROM debian:bookworm-slim
WORKDIR /app
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive; \
    apt-get update -qq; \
    apt-get install -y -qq --no-install-recommends ca-certificates wget; \
    groupadd --gid 10001 appuser; \
    useradd --uid 10001 --gid appuser --home-dir /app --shell /usr/sbin/nologin appuser

COPY --from=build /usr/local/bin/conversation-ingest-rs /usr/local/bin/conversation-ingest-rs
USER appuser
EXPOSE 3161
CMD ["conversation-ingest-rs"]
