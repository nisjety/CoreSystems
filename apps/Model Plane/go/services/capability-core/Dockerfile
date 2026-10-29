# syntax=docker/dockerfile:1.7
#
# Multi-stage build for capability-core.
#
# Build context is the Go workspace root `apps/Model Plane/go/` because
# `go.mod` carries a local `replace ../../gen` directive.
FROM golang:1.25.12-alpine3.24 AS builder
RUN --mount=type=cache,target=/var/cache/apk \
    apk add git ca-certificates
WORKDIR /workspace
COPY . .
WORKDIR /workspace/services/capability-core
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
RUN --mount=type=cache,target=/var/cache/apk \
    set -e; \
    if ! apk add --quiet ca-certificates curl postgresql-client \
        > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log; \
    addgroup -S -g 10001 modelplane; \
    adduser -S -D -H -u 10001 -G modelplane modelplane; \
    mkdir -p /var/lib/capability-core; \
    chown modelplane:modelplane /var/lib/capability-core
COPY --from=builder /service /usr/local/bin/service
COPY services/capability-core/migrations /app/migrations
COPY services/capability-core/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
USER modelplane
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["service"]
