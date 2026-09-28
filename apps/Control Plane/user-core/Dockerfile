# syntax=docker/dockerfile:1.7
# =============================================================
# user-core — Go service (with gRPC / protobuf generation)
# Build: golang:1.25-alpine (builder) → alpine:3.22 (runtime)
# =============================================================

# ── Builder ──────────────────────────────────────────────────
FROM golang:1.26.5-alpine3.24@sha256:0178a641fbb4858c5f1b48e34bdaabe0350a330a1b1149aabd498d0699ff5fb2 AS builder

RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    set -e; \
    if ! apk add --quiet git make protobuf protobuf-dev \
        > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log

WORKDIR /build

# Dep layer — only invalidated when go.mod/go.sum changes
COPY go.mod go.sum ./

# go.mod carries `replace github.com/triodelab/model-plane/gen/go =>
# "../../Model Plane/go/gen"`, and a `replace` is resolved on the FILESYSTEM, not
# through the module proxy. The build context is ./user-core, so without this the
# sibling module simply is not there and `go mod download` fails — which is not a
# network problem, so the surrounding retry loop just fails five times slowly.
#
# WORKDIR is /build, so Go resolves that relative path to /Model Plane/go/gen.
# The named context is supplied by `additional_contexts` in Control's compose;
# this must land BEFORE `go mod download` reads go.mod.
COPY --from=model-plane-go-gen [".", "/Model Plane/go/gen/"]

ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    set -e; \
    for i in 1 2 3 4 5; do \
        if go mod download; then \
            exit 0; \
        fi; \
        echo "go mod download retry ${i}"; \
        sleep 8; \
    done; \
    exit 1

# Install protoc plugins in a separate cached layer
# Keep codegen deterministic and retry transient module proxy failures.
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    set -e; \
    for i in 1 2 3 4 5; do \
        if go install google.golang.org/protobuf/cmd/protoc-gen-go@v1.36.11 && \
           go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@v1.6.0; then \
            exit 0; \
        fi; \
        echo "go tool install retry ${i}"; \
        sleep 8; \
    done; \
    exit 1

# Copy source
COPY . .

# Generate protobuf code
RUN protoc --go_out=. --go_opt=paths=source_relative \
    --go-grpc_out=. --go-grpc_opt=paths=source_relative \
    proto/user/v1/*.proto

# Build — removed -a flag (forces full rebuild every time, defeating cache)
# -ldflags="-s -w" strips debug; -trimpath removes host path info
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS=linux go build \
    -ldflags="-s -w" \
    -trimpath \
    -o user-service ./cmd/server/main.go

# ── Runtime ──────────────────────────────────────────────────
FROM alpine:3.22
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE

# Pinned from alpine:latest — deterministic builds
# Merge apk layers and create non-root user
RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    set -e; \
    if ! apk add --quiet ca-certificates postgresql-client su-exec \
        > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log; \
    addgroup -S appgroup && adduser -S appuser -G appgroup

WORKDIR /app

COPY --chown=appuser:appgroup --from=builder /build/user-service ./user-service
COPY --chown=appuser:appgroup --from=builder /build/migrations ./migrations
COPY --chown=appuser:appgroup --chmod=755 docker-entrypoint.sh /app/docker-entrypoint.sh
COPY --chown=appuser:appgroup --chmod=755 scripts/shared-entrypoint.sh /app/scripts/shared-entrypoint.sh

# HTTP + gRPC
EXPOSE 3012 50012 9091

USER appuser

ENTRYPOINT ["/bin/sh", "/app/docker-entrypoint.sh"]
CMD ["./user-service"]
