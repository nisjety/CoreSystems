# syntax=docker/dockerfile:1.7
FROM golang:1.25-bookworm AS build

WORKDIR /src
ARG TARGETOS=linux
ARG TARGETARCH
ENV GOPROXY=https://proxy.golang.org|https://goproxy.io|direct
ENV GOSUMDB=sum.golang.org

COPY go.mod go.sum* ./
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    set -e; \
    if [ -f go.sum ]; then go mod download; else go mod download && go mod tidy; fi

COPY . .
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS="${TARGETOS}" GOARCH="${TARGETARCH:-$(go env GOARCH)}" go build -o /out/insight-core ./cmd/server

FROM debian:bookworm-slim

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

COPY --from=build /out/insight-core /usr/local/bin/insight-core

USER appuser
EXPOSE 3163
CMD ["insight-core"]
