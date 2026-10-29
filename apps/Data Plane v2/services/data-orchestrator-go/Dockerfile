# syntax=docker/dockerfile:1.7
# Multi-stage build for data-orchestrator-go.
# Build context: services/data-orchestrator-go. The ../../shared/go replace target is NOT in
# that context, so it arrives as a named additional build context (`shared-go`,
# wired in docker-compose.yml) — the same mechanism the Rust services use for
# the Model Plane protos. Widening to the repo root was rejected: .dockerignore
# deliberately excludes this tree so root-context Rust builds stay lean.
# WORKDIR is two levels deep so the go.mod replace path resolves.

FROM golang:1.26.5-bookworm AS builder
WORKDIR /workspace/services/data-orchestrator-go
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org
COPY go.mod go.sum ./
COPY --from=shared-go go.mod go.sum /workspace/shared/go/
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    set -e; \
    for i in 1 2 3 4 5; do \
        if go mod download; then \
            exit 0; \
        fi; \
        echo "go mod download retry $i"; \
        sleep 8; \
    done; \
    exit 1
COPY --from=shared-go . /workspace/shared/go
COPY . ./
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOFLAGS="-trimpath" \
    go build -ldflags="-s -w" -o /data-orchestrator ./cmd

FROM debian:bookworm-slim AS runtime
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
ENV SOURCE_REVISION=$SOURCE_REVISION BUILD_DATE=$BUILD_DATE
ARG DEBIAN_FRONTEND=noninteractive
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends ca-certificates curl \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log
RUN groupadd --gid 10001 dpv2 \
    && useradd --uid 10001 --gid dpv2 --no-create-home --shell /usr/sbin/nologin dpv2
USER dpv2

COPY --from=builder /data-orchestrator /usr/local/bin/data-orchestrator

EXPOSE 8012

HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=5 \
    CMD curl -sf http://127.0.0.1:8012/readyz || exit 1

ENTRYPOINT ["data-orchestrator"]
