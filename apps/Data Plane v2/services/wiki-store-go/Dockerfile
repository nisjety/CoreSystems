# syntax=docker/dockerfile:1.7
# Multi-stage build for wiki-store-go.
# Build context: repository root. This module uses ../../gen/go via go.mod
# replace, so the generated module must be available during dependency fetch.

FROM golang:1.26.2-bookworm AS builder
WORKDIR /workspace/services/wiki-store-go
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org
COPY services/wiki-store-go/go.mod services/wiki-store-go/go.sum ./
COPY gen/go/go.mod gen/go/go.sum /workspace/gen/go/
RUN set -e; \
    for i in 1 2 3 4 5; do \
        if go mod download; then \
            exit 0; \
        fi; \
        echo "go mod download retry $i"; \
        sleep 8; \
    done; \
    exit 1
COPY gen/go /workspace/gen/go
COPY services/wiki-store-go ./
RUN CGO_ENABLED=0 GOFLAGS="-trimpath" \
    go build -ldflags="-s -w" -o /wiki-store ./cmd

FROM debian:bookworm-slim AS runtime
ARG DEBIAN_FRONTEND=noninteractive
RUN set -e; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends ca-certificates curl \
        > /tmp/apt-install.log 2>&1; then \
        cat /tmp/apt-install.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apt-install.log; \
    rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin dpv2
USER dpv2

COPY --from=builder /wiki-store /usr/local/bin/wiki-store

EXPOSE 8011

HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=5 \
    CMD curl -sf http://127.0.0.1:8011/readyz || exit 1

ENTRYPOINT ["wiki-store"]
