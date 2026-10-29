# syntax=docker/dockerfile:1.7
# Build context: Model Plane root (sibling of `rust/`, `proto/`).
FROM lukemathwalker/cargo-chef:latest-rust-1.97-bookworm@sha256:974d873357408b6ececac8729285204664e23f5aab21a34e353ae5bf9bb899a2 AS chef
WORKDIR /app
ENV CARGO_NET_RETRY=10 \
    CARGO_HTTP_TIMEOUT=120 \
    CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends protobuf-compiler libprotobuf-dev \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log
FROM chef AS planner
COPY rust/ rust/
WORKDIR /app/rust
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
WORKDIR /app/rust
COPY --from=planner /app/rust/recipe.json recipe.json
COPY proto/ /app/proto/
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/rust/target,sharing=locked \
    cargo chef cook --release --recipe-path recipe.json
WORKDIR /app
COPY rust/ rust/
WORKDIR /app/rust
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/rust/target,sharing=locked \
    cargo build --release --bin execution-core \
    && cp target/release/execution-core /usr/local/bin/execution-core

FROM debian:bookworm-slim AS runtime
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
# Trixie has GLIBC >= 2.38 to match the cargo-chef builder image. curl is
# needed by the compose healthcheck (curl -f /healthz). bubblewrap (`bwrap`)
# is REQUIRED for sandbox isolation: `sandbox::is_supported()` checks for it on
# PATH, and without it code execution degrades to an UNSANDBOXED passthrough —
# so it ships in the runtime image, not just dev hosts (chat-parity Phase 3 /
# capability-ownership-matrix §G1, Tier-2 sandbox isolation).
#
# The Python stack is what makes the `code_interpreter` tool
# (src/code_interpreter.rs) able to do real work — build a spreadsheet, a Word
# document, a PDF, a chart, or crunch a CSV — and it must live in the RUNTIME
# image because the sandbox has NO network: nothing can be fetched at call time.
# Debian packages are preferred over pip (distro-pinned, no compiler, no
# build-time surprises) and every name below was verified to exist for
# bookworm. python-pptx is the one exception — it has no bookworm package — so it
# comes from pip as a pure-python wheel, pinned.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends \
        ca-certificates curl bubblewrap \
        python3 python3-pip \
        python3-numpy python3-pandas python3-matplotlib \
        python3-openpyxl python3-xlsxwriter \
        python3-docx python3-reportlab python3-pypdf \
        python3-pil python3-lxml \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log
# `--break-system-packages` is mandatory here: pip refuses to write into a
# distro-managed site-packages under PEP 668, and this IS the image's system
# Python. Same failure handling as apt — surface the log, then fail the build.
RUN set -e; \
    if ! pip3 install --no-cache-dir --break-system-packages python-pptx==1.0.2 \
        > /tmp/pip-install.log 2>&1; then \
        cat /tmp/pip-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/pip-install.log
# Headless rendering: the sandbox has no display, so matplotlib must never try an
# interactive backend. `code_interpreter` also sets this per call (so dev hosts
# behave identically); having it in the image keeps any other Python entry point
# honest too.
ENV MPLBACKEND=Agg
RUN useradd --system --uid 10001 --create-home --shell /usr/sbin/nologin modelplane
COPY --from=builder /usr/local/bin/execution-core /usr/local/bin/execution-core
USER modelplane
EXPOSE 8083 9093
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=5 \
    CMD curl -fsS http://127.0.0.1:8083/healthz >/dev/null || exit 1
ENTRYPOINT ["execution-core"]
