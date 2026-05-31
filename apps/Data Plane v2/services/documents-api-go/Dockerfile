# syntax=docker/dockerfile:1.7
# Multi-stage build for documents-api-go.
# Build context: services/documents-api-go (single-module Go).

FROM golang:1.25-bookworm AS builder
WORKDIR /app

# Cache deps separately from source.
COPY go.mod go.sum ./
RUN go mod download

COPY . .

# CGO_ENABLED=0 → static binary (works in scratch/distroless).
# -trimpath strips local paths from binary metadata.
# -ldflags="-s -w" strips symbol/debug info (~30% smaller).
# We deliberately exclude the integration_test.go (build tag `integration`)
# from the production binary; it would never be linked anyway since tests
# don't go into `go build` output, but the explicit `./cmd/...` makes it clear.
RUN CGO_ENABLED=0 GOFLAGS="-trimpath" \
    go build -ldflags="-s -w" -o /documents-api ./cmd

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

COPY --from=builder /documents-api /usr/local/bin/documents-api

EXPOSE 8010

HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=5 \
    CMD curl -sf http://127.0.0.1:8010/readyz || exit 1

ENTRYPOINT ["documents-api"]
