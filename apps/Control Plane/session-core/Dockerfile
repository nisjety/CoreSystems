# syntax=docker/dockerfile:1.7
# ── Build stage ───────────────────────────────────────
FROM golang:1.25-alpine AS builder

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
    CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o /session-core ./cmd/server

# ── Runtime stage ─────────────────────────────────────
FROM alpine:3.21

RUN --mount=type=cache,target=/var/cache/apk \
    set -e; \
    if ! apk add --quiet ca-certificates curl postgresql16-client bash \
        > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log

COPY --from=builder /session-core /usr/local/bin/session-core
COPY migrations /app/migrations
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
COPY scripts/shared-entrypoint.sh /shared/shared-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh /shared/shared-entrypoint.sh

RUN adduser -D -u 1000 appuser
USER appuser

WORKDIR /app
ENV GIN_MODE=release

EXPOSE 3017 50017

ENTRYPOINT ["/app/docker-entrypoint.sh"]
CMD ["session-core"]
