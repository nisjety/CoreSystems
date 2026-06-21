# syntax=docker/dockerfile:1.7
FROM golang:1.25-bookworm AS build

WORKDIR /src
ARG TARGETOS=linux
ARG TARGETARCH
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
    CGO_ENABLED=0 GOOS="${TARGETOS}" GOARCH="${TARGETARCH:-$(go env GOARCH)}" go build -o /out/notification-core ./cmd/server

FROM debian:bookworm-slim

WORKDIR /app

RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    set -e; \
    rm -f /etc/apt/apt.conf.d/docker-clean; \
    export DEBIAN_FRONTEND=noninteractive DEBCONF_NOWARNINGS=yes; \
    apt-get update -qq; \
    if ! apt-get install -y -qq --no-install-recommends ca-certificates wget \
      > /tmp/apt-install.log 2>&1; then \
      cat /tmp/apt-install.log; \
      exit 1; \
    fi; \
    rm -f /tmp/apt-install.log; \
    groupadd --gid 10001 appuser; \
    useradd --uid 10001 --gid appuser --home-dir /app --shell /usr/sbin/nologin appuser; \
    chown -R appuser:appuser /app

COPY --from=build /out/notification-core /usr/local/bin/notification-core
COPY migrations /app/migrations

USER appuser

EXPOSE 3140

CMD ["notification-core"]
