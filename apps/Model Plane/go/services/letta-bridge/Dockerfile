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
RUN apk --no-cache add ca-certificates curl postgresql-client
COPY --from=builder /service /usr/local/bin/service
COPY services/letta-bridge/migrations /app/migrations
COPY services/letta-bridge/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["service"]
