# syntax=docker/dockerfile:1.7
#
# Multi-stage build for browser-broker.
#
# Build context is the Go workspace root `apps/Model Plane/go/` because
# `go.mod` carries a local `replace ../../gen` directive.
FROM golang:1.25-alpine AS builder
RUN --mount=type=cache,target=/var/cache/apk \
    apk add git ca-certificates
WORKDIR /workspace
COPY . .
WORKDIR /workspace/services/browser-broker
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org
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
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    go build -o /service ./cmd/

FROM alpine:3.20
RUN --mount=type=cache,target=/var/cache/apk \
    apk add ca-certificates curl
COPY --from=builder /service /usr/local/bin/service
ENTRYPOINT ["service"]
