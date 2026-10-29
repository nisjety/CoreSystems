# syntax=docker/dockerfile:1.7
# Multi-stage build for wiki-store-go.
# Build context: repository root. This module uses ../../gen/go and
# ../../shared/go via go.mod replace, so both sibling modules must be
# available during dependency fetch AND during the build itself.

FROM golang:1.26.5-bookworm AS builder
WORKDIR /workspace/services/wiki-store-go
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org
COPY services/wiki-store-go/go.mod services/wiki-store-go/go.sum ./
COPY gen/go/go.mod gen/go/go.sum /workspace/gen/go/
COPY shared/go/go.mod shared/go/go.sum /workspace/shared/go/
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
COPY gen/go /workspace/gen/go
COPY shared/go /workspace/shared/go
COPY services/wiki-store-go ./
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOFLAGS="-trimpath" \
    go build -ldflags="-s -w" -o /wiki-store ./cmd

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

COPY --from=builder /wiki-store /usr/local/bin/wiki-store

EXPOSE 8011

HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=5 \
    CMD curl -sf http://127.0.0.1:8011/readyz || exit 1

ENTRYPOINT ["wiki-store"]
