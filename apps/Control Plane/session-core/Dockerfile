# ── Build stage ───────────────────────────────────────
FROM golang:1.25-alpine AS builder

RUN apk add --no-cache git ca-certificates

WORKDIR /build
COPY go.mod go.sum ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o /session-core ./cmd/server

# ── Runtime stage ─────────────────────────────────────
FROM alpine:3.21

RUN set -e; \
    if ! apk add --no-cache --quiet ca-certificates curl postgresql16-client bash \
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
