# syntax=docker/dockerfile:1.7
# Rust 1.89 matches the gateway toolchain and satisfies transitive dependencies
# that require rustc >= 1.86 while remaining reproducible across rebuilds.

# ---- chef: pin cargo-chef once on the shared toolchain image ----------------
FROM lukemathwalker/cargo-chef:latest-rust-1.89-bookworm@sha256:0a0b3f65242ff7f1d5c3ea5c3ae10f4a1897204fedb8a4602651014f4302a129 AS chef
WORKDIR /src
ENV CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse

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

FROM debian:bookworm-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"
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
