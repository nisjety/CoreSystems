FROM rust:1.89-bookworm AS build
WORKDIR /app

# Dependency cache layer: build a stub so the dependency compilation is cached
# independently of source changes.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src && echo "fn main() {}" > src/main.rs
RUN cargo build --release || true

# Real build. Remove the stub's binary + fingerprint artifacts so cargo is
# forced to recompile main from the real sources (a bare `COPY src` does not
# always bust cargo's fingerprint, which previously shipped the empty stub).
COPY src ./src
RUN rm -f target/release/velion-gateway-rs \
    && rm -f target/release/deps/velion_gateway_rs* \
    && cargo build --release \
    && test "$(stat -c%s target/release/velion-gateway-rs)" -gt 2000000

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/target/release/velion-gateway-rs /usr/local/bin/velion-gateway-rs
EXPOSE 3185
CMD ["velion-gateway-rs"]
