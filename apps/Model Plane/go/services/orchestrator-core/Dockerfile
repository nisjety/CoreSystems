# syntax=docker/dockerfile:1.4
#
# Multi-stage build for orchestrator-core.
#
# Build context is the Go workspace root `apps/Model Plane/go/` because
# `go.mod` carries local `replace` directives pointing at `../../gen` and
# `../../pkg/{natsx,envelope}`. Those siblings must be present in the build
# context for `go build` to resolve them.
#
# GOPROXY fallback chain: BuildKit on Docker Desktop hits intermittent
# `TLS handshake timeout` errors against proxy.golang.org. The chain falls
# through to goproxy.io and finally direct fetches from VCS so transient
# upstream issues don't fail the build. Module hashes are still verified
# against go.sum locally.
FROM golang:1.25-alpine AS builder
RUN apk --no-cache add git ca-certificates
WORKDIR /workspace
COPY . .
WORKDIR /workspace/services/orchestrator-core
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
RUN apk --no-cache add ca-certificates
COPY --from=builder /service /usr/local/bin/service
ENTRYPOINT ["service"]
