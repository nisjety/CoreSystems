# syntax=docker/dockerfile:1.7
FROM golang:1.26.5-bookworm@sha256:18aedc16aa19b3fd7ded7245fc14b109e054d65d22ed53c355c899582bbb2113 AS build

WORKDIR /src
ARG TARGETOS=linux
ARG TARGETARCH

COPY go.mod go.sum ./
# Retry the module fetch: proxy.golang.org intermittently returns
# "net/http: TLS handshake timeout" when several Go services in this plane
# build in parallel, and a bare `go mod download` fails the whole plane on the
# first blip (observed 2026-08-06 on golang.org/x/crypto). Mirrors the
# notification-core Dockerfile, which already survives this. The go-build cache
# mount is included here too so this layer matches every sibling service.
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
    CGO_ENABLED=0 GOOS="${TARGETOS}" GOARCH="${TARGETARCH:-$(go env GOARCH)}" go build -trimpath -ldflags="-s -w" -o /out/information-core ./cmd/server

FROM debian:bookworm-slim@sha256:60eac759739651111db372c07be67863818726f754804b8707c90979bda511df
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"

WORKDIR /app

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    export DEBIAN_FRONTEND=noninteractive; \
    apt-get update -qq; \
    apt-get install -y -qq --no-install-recommends ca-certificates wget; \
    groupadd --gid 10001 appuser; \
    useradd --uid 10001 --gid appuser --home-dir /app --shell /usr/sbin/nologin appuser; \
    chown -R appuser:appuser /app

COPY --from=build /out/information-core /usr/local/bin/information-core

USER appuser
EXPOSE 3190

CMD ["information-core"]
