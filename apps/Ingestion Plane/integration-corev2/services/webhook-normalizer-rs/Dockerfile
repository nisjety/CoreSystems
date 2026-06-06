FROM rust:1.85-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home app
USER app
WORKDIR /app
COPY --from=builder /app/target/release/integration-webhook-normalizer /usr/local/bin/integration-webhook-normalizer

ENV PORT=3036
EXPOSE 3036

CMD ["integration-webhook-normalizer"]
