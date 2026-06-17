# Multi-stage build for cost-core.
#
# Build context is this service directory (apps/Model Plane/go/services/cost-core)
# — cost-core's go.mod has no local replace directives, so it builds standalone.
FROM golang:1.25-alpine AS builder
RUN apk --no-cache add git ca-certificates
WORKDIR /app
COPY . .
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    set -e; \
    for i in 1 2 3 4 5; do \
        if go mod download; then \
            break; \
        fi; \
        echo "go mod download retry $i"; \
        sleep 8; \
    done
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    go build -o /service ./cmd/

FROM alpine:3.20
RUN apk --no-cache add ca-certificates curl postgresql-client
COPY --from=builder /service /usr/local/bin/service
COPY migrations /app/migrations
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["service"]
