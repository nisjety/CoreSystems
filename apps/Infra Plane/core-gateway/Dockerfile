# syntax=docker/dockerfile:1.7
# Aquatiq Integration Gateway - Multi-stage build
FROM golang:1.24-alpine AS builder

# Install build dependencies
RUN apk add --no-cache git make protobuf protobuf-dev

WORKDIR /build

# Copy go mod files
COPY go.mod go.sum ./
RUN --mount=type=cache,target=/go/pkg/mod,sharing=locked \
    go mod download

# Copy source code
COPY . .

# Build the application
RUN --mount=type=cache,target=/go/pkg/mod,sharing=locked \
    --mount=type=cache,target=/root/.cache/go-build,sharing=locked \
    CGO_ENABLED=0 GOOS=linux go build -a -installsuffix cgo -ldflags="-w -s" -o gateway ./cmd/gateway

# Final stage - minimal runtime image
FROM alpine:3.21

# Add ca-certificates for HTTPS and curl for healthcheck
RUN apk --no-cache add ca-certificates curl tzdata

# Create non-root user
RUN addgroup -g 1000 coresystem && \
    adduser -D -u 1000 -G coresystem coresystem

WORKDIR /app

# Copy binary from builder
COPY --from=builder /build/gateway .
COPY --from=builder /build/configs/config.yaml /app/configs/config.yaml

# Create data directory for Traefik config
RUN mkdir -p /data && chown -R coresystem:coresystem /app /data

USER coresystem

EXPOSE 5001 50051

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -f http://localhost:5001/health || exit 1

CMD ["./gateway"]
