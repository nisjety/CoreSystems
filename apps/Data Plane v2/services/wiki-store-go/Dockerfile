# syntax=docker/dockerfile:1.7
# Multi-stage build for wiki-store-go.
# Build context: services/wiki-store-go.

FROM golang:1.25-bookworm AS builder
WORKDIR /app
COPY go.mod go.sum ./
RUN go mod download
COPY . .
RUN CGO_ENABLED=0 GOFLAGS="-trimpath" \
    go build -ldflags="-s -w" -o /wiki-store ./cmd

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --no-create-home --shell /usr/sbin/nologin dpv2
USER dpv2

COPY --from=builder /wiki-store /usr/local/bin/wiki-store

EXPOSE 8011

HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=5 \
    CMD curl -sf http://127.0.0.1:8011/readyz || exit 1

ENTRYPOINT ["wiki-store"]
