# syntax=docker/dockerfile:1.7
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
FROM golang:1.25.11-alpine3.24 AS builder
RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    apk add git ca-certificates
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
    CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /service ./cmd/

FROM alpine:3.24
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    apk add ca-certificates \
    && addgroup -S -g 10001 modelplane \
    && adduser -S -D -H -u 10001 -G modelplane modelplane
COPY --from=builder /service /usr/local/bin/service
USER modelplane
ENTRYPOINT ["service"]
