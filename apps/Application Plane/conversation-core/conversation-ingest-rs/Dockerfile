# rust:1 = latest stable 1.x (matches the other planes' base + satisfies deps
# like icu_*/idna_adapter that now require rustc >= 1.86; the old 1.85 pin
# broke `cargo build --release` here with "rustc 1.85.1 is not supported").
FROM rust:1-bookworm AS build

WORKDIR /src
COPY Cargo.toml ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
WORKDIR /app
RUN set -e; \
    export DEBIAN_FRONTEND=noninteractive; \
    apt-get update -qq; \
    apt-get install -y -qq --no-install-recommends ca-certificates wget; \
    rm -rf /var/lib/apt/lists/*; \
    groupadd --gid 10001 appuser; \
    useradd --uid 10001 --gid appuser --home-dir /app --shell /usr/sbin/nologin appuser

COPY --from=build /src/target/release/conversation-ingest-rs /usr/local/bin/conversation-ingest-rs
USER appuser
EXPOSE 3161
CMD ["conversation-ingest-rs"]
