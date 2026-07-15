# syntax=docker/dockerfile:1.7
#
# audit-core builder. Single Go module, no workspace replace directives,
# so the build context is the service dir itself. Keeps `go mod download`
# under a BuildKit cache mount and uses the GOPROXY fallback chain that
# the other Phase A Go services adopted, so a flaky proxy.golang.org
# doesn't fail the whole build.
FROM golang:1.26.5-alpine3.24@sha256:0178a641fbb4858c5f1b48e34bdaabe0350a330a1b1149aabd498d0699ff5fb2 AS builder
RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    apk add git ca-certificates
WORKDIR /workspace
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org
COPY go.mod go.sum ./
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
COPY . .
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags="-s -w" -o /audit-core ./cmd/server && \
    CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags="-s -w" -o /nats-provisioner ./cmd/nats-provisioner && \
    CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags="-s -w" -o /control-event-bridge ./cmd/control-event-bridge

FROM alpine:3.22
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
RUN --mount=type=cache,target=/var/cache/apk,sharing=locked \
    apk add ca-certificates && \
    addgroup -S appgroup && adduser -S appuser -G appgroup
COPY --chown=appuser:appgroup --from=builder /audit-core /usr/local/bin/audit-core
COPY --chown=appuser:appgroup --from=builder /nats-provisioner /usr/local/bin/nats-provisioner
COPY --chown=appuser:appgroup --from=builder /control-event-bridge /usr/local/bin/control-event-bridge
EXPOSE 8187 9091
USER appuser
ENTRYPOINT ["audit-core"]
