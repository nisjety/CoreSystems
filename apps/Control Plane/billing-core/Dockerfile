# =============================================================
# billing-core — Go service
# Build: golang:1.25-alpine (builder) → alpine:3.22 (runtime)
# =============================================================

# ── Builder ──────────────────────────────────────────────────
FROM golang:1.25-alpine AS builder

RUN apk add --no-cache git

WORKDIR /build

# Copy both go.mod AND go.sum — layer only invalidated when deps change
COPY go.mod go.sum ./
# Cache /go/pkg/mod across builds — avoids re-downloading ~42 modules on each run
RUN --mount=type=cache,target=/go/pkg/mod \
    go mod download

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

# Merge all apk installs into one layer
RUN apk add --no-cache ca-certificates postgresql-client && \
    addgroup -S appgroup && adduser -S appuser -G appgroup

WORKDIR /app

COPY --from=builder /build/billing-core      ./billing-core
COPY --from=builder /build/migrations        ./migrations
COPY docker-entrypoint.sh                    /app/docker-entrypoint.sh
COPY scripts/shared-entrypoint.sh            /app/scripts/shared-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh /app/scripts/shared-entrypoint.sh && \
    chown -R appuser:appgroup /app

EXPOSE 3014 50013

USER appuser

ENTRYPOINT ["/bin/sh", "/app/docker-entrypoint.sh"]
CMD ["./billing-core"]
