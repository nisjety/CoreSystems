# =============================================================
# org-core — Go service
# Build: golang:1.25-alpine (builder) → alpine:3.22 (runtime)
# =============================================================

# ── Builder ──────────────────────────────────────────────────
FROM golang:1.25-alpine AS builder

RUN apk add --no-cache git

WORKDIR /build
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org

# Copy both go.mod AND go.sum — layer only invalidated when deps change
COPY go.mod go.sum ./
RUN set -e; \
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
RUN CGO_ENABLED=0 GOOS=linux go build \
    -ldflags="-s -w" \
    -trimpath \
    -o org-core ./cmd/server

# ── Runtime ──────────────────────────────────────────────────
FROM alpine:3.22

# Merge all apk installs into one layer
RUN set -e; \
    if ! apk add --no-cache --quiet ca-certificates postgresql-client \
        > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log; \
    addgroup -S appgroup && adduser -S appuser -G appgroup

WORKDIR /app

COPY --from=builder /build/org-core          ./org-core
COPY --from=builder /build/migrations        ./migrations
COPY docker-entrypoint.sh                    /app/docker-entrypoint.sh
COPY scripts/shared-entrypoint.sh            /app/scripts/shared-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh /app/scripts/shared-entrypoint.sh && \
    chown -R appuser:appgroup /app

# HTTP + gRPC + metrics + pprof
EXPOSE 8080 9090 9091 6061

USER appuser

ENTRYPOINT ["/bin/sh", "/app/docker-entrypoint.sh"]
CMD ["./org-core"]
