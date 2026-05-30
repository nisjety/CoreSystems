# =============================================================
# user-core — Go service (with gRPC / protobuf generation)
# Build: golang:1.25-alpine (builder) → alpine:3.22 (runtime)
# =============================================================

# ── Builder ──────────────────────────────────────────────────
FROM golang:1.25-alpine AS builder

RUN apk add --no-cache git make protobuf protobuf-dev

WORKDIR /build

# Dep layer — only invalidated when go.mod/go.sum changes
COPY go.mod go.sum ./
RUN go mod download

# Install protoc plugins in a separate cached layer
# Runs only when go.mod changes (tools are tracked there)
RUN go install google.golang.org/protobuf/cmd/protoc-gen-go@latest && \
    go install google.golang.org/grpc/cmd/protoc-gen-go-grpc@latest

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
RUN apk add --no-cache ca-certificates postgresql-client && \
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
