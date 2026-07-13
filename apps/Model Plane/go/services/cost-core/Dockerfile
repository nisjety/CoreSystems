# syntax=docker/dockerfile:1.7
# Multi-stage build for cost-core.
#
# Build context is apps/Model Plane/go so the shared Auth Core verifier can be
# compiled from pkg/authctx without vendoring security logic into the service.
FROM golang:1.25.11-alpine3.24 AS builder
RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    apk add git ca-certificates
WORKDIR /app/services/cost-core
COPY services/cost-core/go.mod services/cost-core/go.sum ./
COPY pkg/authctx/go.mod pkg/authctx/go.sum /app/pkg/authctx/
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
COPY pkg/authctx /app/pkg/authctx
COPY services/cost-core .
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /service ./cmd/

FROM alpine:3.24
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    apk add ca-certificates curl postgresql-client \
    && addgroup -S -g 10001 modelplane \
    && adduser -S -D -H -u 10001 -G modelplane modelplane
COPY --from=builder /service /usr/local/bin/service
COPY services/cost-core/migrations /app/migrations
COPY services/cost-core/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod +x /usr/local/bin/docker-entrypoint.sh
USER modelplane
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["service"]
