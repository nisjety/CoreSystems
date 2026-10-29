# syntax=docker/dockerfile:1.7
# ── Build stage ───────────────────────────────────────
FROM golang:1.26.5-alpine3.24@sha256:0178a641fbb4858c5f1b48e34bdaabe0350a330a1b1149aabd498d0699ff5fb2 AS builder

RUN --mount=type=cache,target=/var/cache/apk \
    apk add git ca-certificates

WORKDIR /build
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org
COPY go.mod go.sum ./
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
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
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags="-s -w" -o /session-core ./cmd/server

# ── Runtime stage ─────────────────────────────────────
FROM alpine:3.22
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE

RUN --mount=type=cache,target=/var/cache/apk \
    set -e; \
    if ! apk add --quiet ca-certificates postgresql-client \
        > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log; \
    addgroup -S appgroup && adduser -S appuser -G appgroup

COPY --chown=appuser:appgroup --from=builder /session-core /usr/local/bin/session-core
COPY --chown=appuser:appgroup migrations /app/migrations
COPY --chown=appuser:appgroup --chmod=755 docker-entrypoint.sh /app/docker-entrypoint.sh
COPY --chown=appuser:appgroup --chmod=755 scripts/shared-entrypoint.sh /app/scripts/shared-entrypoint.sh
USER appuser

WORKDIR /app
ENV GIN_MODE=release

EXPOSE 3017 50017 9091

ENTRYPOINT ["/app/docker-entrypoint.sh"]
CMD ["session-core"]
