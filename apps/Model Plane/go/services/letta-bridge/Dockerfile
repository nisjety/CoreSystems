# syntax=docker/dockerfile:1.4
#
# Multi-stage build for letta-bridge.
#
# Build context is the Go workspace root `apps/Model Plane/go/` because
# `go.mod` carries a local `replace ../../gen` directive.
FROM golang:1.25-alpine AS builder
RUN apk --no-cache add git ca-certificates
WORKDIR /workspace
COPY . .
WORKDIR /workspace/services/letta-bridge
ENV GOPROXY=https://proxy.golang.org,https://goproxy.io,direct
ENV GOSUMDB=sum.golang.org
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    for i in 1 2 3 4 5; do \
      go mod download && break || { echo "go mod download retry $i"; sleep 8; }; \
    done
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    go build -o /service ./cmd/

FROM alpine:3.20
RUN apk --no-cache add ca-certificates curl
COPY --from=builder /service /usr/local/bin/service
ENTRYPOINT ["service"]
