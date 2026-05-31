# =============================================================
# user-core — Go service (with gRPC / protobuf generation)
# Build: golang:1.25-alpine (builder) → alpine:3.22 (runtime)
# =============================================================

# ── Builder ──────────────────────────────────────────────────
FROM golang:1.25-alpine AS builder

RUN set -e; \
    if ! apk add --no-cache --quiet git make protobuf protobuf-dev \
        > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log

WORKDIR /build

# Dep layer — only invalidated when go.mod/go.sum changes
COPY go.mod go.sum ./
ENV GOPROXY=https://proxy.golang.org,https://goproxy.io,direct
ENV GOSUMDB=sum.golang.org
RUN set -e; \
    for i in 1 2 3 4 5; do \
        if go mod download; then \
            exit 0; \
        fi; \
        echo "go mod download retry $i"; \
        sleep 8; \
    done; \
    exit 1

# Install protoc plugins in a separate cached layer
# Keep codegen deterministic and retry transient module proxy failures.
RUN set -e; \
    for i in 1 2 3 4 5; do \
        if go install google.golang.org/protobuf/cmd/protoc-gen-go@v1.36.11 && \
           go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@v1.6.0; then \
            exit 0; \
        fi; \
        echo "go tool install retry $i"; \
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
RUN CGO_ENABLED=0 GOOS=linux go build \
    -ldflags="-s -w" \
    -trimpath \
    -o user-service ./cmd/server/main.go

# ── Runtime ──────────────────────────────────────────────────
FROM alpine:3.22

# Pinned from alpine:latest — deterministic builds
# Merge apk layers and create non-root user
RUN set -e; \
    if ! apk add --no-cache --quiet ca-certificates postgresql-client \
        > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log; \
    addgroup -S appgroup && adduser -S appuser -G appgroup

WORKDIR /app

COPY --from=builder /build/user-service      ./user-service
COPY --from=builder /build/migrations        ./migrations
COPY docker-entrypoint.sh                    /app/docker-entrypoint.sh
COPY scripts/shared-entrypoint.sh            /app/scripts/shared-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh /app/scripts/shared-entrypoint.sh && \
    chown -R appuser:appgroup /app

# HTTP + gRPC
EXPOSE 3012 50012

USER appuser

ENTRYPOINT ["/bin/sh", "/app/docker-entrypoint.sh"]
CMD ["./user-service"]
