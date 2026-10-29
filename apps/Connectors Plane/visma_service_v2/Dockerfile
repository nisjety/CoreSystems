# =============================================================================
# visma_service_v2 — multi-stage Dockerfile
#
# Stage 1 (chef-planner): compute recipe (dependency manifest only)
# Stage 2 (chef-builder): compile deps with cached layer, then full binary
# Stage 3 (py-tooling):   builds the Python tooling (vsv2-bq-* CLIs) into a venv
# Stage 4 (runtime):      python slim image, runs the vsv2 CLI; the Rust orchestrator
#                         spawns the vsv2-bq-* commands from the venv on PATH
# =============================================================================

ARG RUST_VERSION=1.88

# ── Stage 1: generate recipe ──────────────────────────────────────────────────
FROM rust:${RUST_VERSION}-bookworm AS chef-planner
RUN cargo install cargo-chef --locked
WORKDIR /app

# The whole workspace goes in, so a crate added to Cargo.toml cannot be missing here: cargo chef
# lists the workspace members from their manifests, and a hand-kept list of manifests broke the
# build when vsv2-interactive and vsv2-mcp joined the workspace. Only recipe.json leaves this stage,
# and it changes when a manifest or Cargo.lock does, so the dependency layer below is still cached
# on a source-only change.
# NOTE: rust-toolchain.toml is intentionally omitted here; the base image is
# already rust:1.88-bookworm. Copying it would trigger rustup to attempt a
# channel-sync from static.rust-lang.org, which fails in network-restricted
# build environments. The full source copy in chef-builder picks it up.
COPY Cargo.toml Cargo.lock ./
COPY core core

RUN cargo chef prepare --recipe-path recipe.json

# ── Stage 2: build ────────────────────────────────────────────────────────────
FROM rust:${RUST_VERSION}-bookworm AS chef-builder
RUN cargo install cargo-chef --locked
WORKDIR /app

# Cache dependencies — this layer is reused on source-only changes
COPY --from=chef-planner /app/recipe.json recipe.json
# Only what the image runs: the MCP server and the write layer are not built into it.
RUN cargo chef cook --release --recipe-path recipe.json --bin vsv2 --bin vsv2-http

# Copy full source and build
COPY . .
RUN rm -f rust-toolchain rust-toolchain.toml \
 && cargo build --release --bin vsv2 --bin vsv2-http

# ── Stage 3: Python tooling ───────────────────────────────────────────────────
# The Rust BqWriter / reconciler do not talk to BigQuery themselves: they spawn the
# `vsv2-bq-stage|merge|shadow|quarantine|reconcile` commands, which own the BQ client and the
# DataMart -> bronze normalizers. Without this stage those spawns fail with "No such file".
FROM python:3.12-slim-bookworm AS py-tooling
WORKDIR /build
COPY pyproject.toml ./
COPY tooling/src tooling/src
RUN python -m venv /opt/venv \
 && /opt/venv/bin/pip install --no-cache-dir --upgrade pip \
 && /opt/venv/bin/pip install --no-cache-dir .
# Fail the build (not the first production sync) if any command the orchestrator spawns is broken.
RUN /opt/venv/bin/vsv2-bq-stage --help >/dev/null \
 && /opt/venv/bin/vsv2-bq-merge --help >/dev/null \
 && /opt/venv/bin/vsv2-bq-shadow --help >/dev/null \
 && /opt/venv/bin/vsv2-bq-quarantine --help >/dev/null \
 && /opt/venv/bin/vsv2-bq-reconcile --help >/dev/null \
 && /opt/venv/bin/vsv2-bq-open-keys --help >/dev/null

# ── Stage 4: runtime ──────────────────────────────────────────────────────────
# Same Debian bookworm / glibc as the Rust build stage, plus the Python that the venv links to.
FROM python:3.12-slim-bookworm AS runtime

# ca-certificates: needed by rustls for TLS chain validation against system roots
RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*

# Non-root user
RUN useradd -m -u 1001 vsv2
USER vsv2

WORKDIR /app

# Binary
COPY --from=chef-builder --chown=vsv2:vsv2 /app/target/release/vsv2 /usr/local/bin/vsv2
COPY --from=chef-builder --chown=vsv2:vsv2 /app/target/release/vsv2-http /usr/local/bin/vsv2-http

# Python tooling: the vsv2-bq-* commands must be on PATH for the orchestrator's subprocess spawns.
COPY --from=py-tooling /opt/venv /opt/venv
ENV PATH="/opt/venv/bin:${PATH}"

# Runtime config directories (read-only mounts in docker-compose)
COPY --chown=vsv2:vsv2 config/ config/

ENTRYPOINT ["/usr/local/bin/vsv2"]
