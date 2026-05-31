FROM rust:1.85-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim AS runtime

RUN set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends ca-certificates wget \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log; \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/autocomplete-core /usr/local/bin/autocomplete-core

ENV AUTOCOMPLETE_HTTP_ADDR=0.0.0.0:3219
ENV AUTOCOMPLETE_METADATA_DB=/data/autocomplete-core.sqlite3

VOLUME ["/data"]
EXPOSE 3219

CMD ["autocomplete-core"]
