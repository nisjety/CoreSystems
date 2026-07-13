# syntax=docker/dockerfile:1.7
# =============================================================
# billing-core — Go service
# Build: golang:1.25-alpine (builder) → alpine:3.22 (runtime)
# =============================================================

# ── Builder ──────────────────────────────────────────────────
FROM golang:1.26.5-alpine3.24@sha256:0178a641fbb4858c5f1b48e34bdaabe0350a330a1b1149aabd498d0699ff5fb2 AS builder

RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    apk add git

WORKDIR /build
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org

# Copy both go.mod AND go.sum — layer only invalidated when deps change
COPY go.mod go.sum ./
# Cache /go/pkg/mod across builds — avoids re-downloading ~42 modules on each run
RUN --mount=type=cache,target=/go/pkg/mod \
    set -e; \
    for i in 1 2 3 4 5; do \
        if go mod download; then \
            exit 0; \
        fi; \
        echo "go mod download retry $i"; \
        sleep 8; \
    done; \
    exit 1

COPY . .
# -ldflags="-s -w" strips debug symbols; -trimpath removes local path references
# Results in ~30% smaller binary
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS=linux go build \
    -ldflags="-s -w" \
    -trimpath \
    -o billing-core ./cmd/server

# ── Runtime ──────────────────────────────────────────────────
FROM alpine:3.22
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE

# Merge all apk installs into one layer
RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    set -e; \
    if ! apk add --quiet ca-certificates postgresql-client \
        > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log; \
    addgroup -S appgroup && adduser -S appuser -G appgroup

WORKDIR /app

COPY --chown=appuser:appgroup --from=builder /build/billing-core ./billing-core
COPY --chown=appuser:appgroup --from=builder /build/migrations ./migrations
COPY --chown=appuser:appgroup --chmod=755 docker-entrypoint.sh /app/docker-entrypoint.sh
COPY --chown=appuser:appgroup --chmod=755 scripts/shared-entrypoint.sh /app/scripts/shared-entrypoint.sh

EXPOSE 3014 50013 9091

USER appuser

ENTRYPOINT ["/bin/sh", "/app/docker-entrypoint.sh"]
CMD ["./billing-core"]
