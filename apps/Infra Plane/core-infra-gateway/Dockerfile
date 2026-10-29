# syntax=docker/dockerfile:1.7
FROM golang:1.24-alpine3.21 AS build

WORKDIR /src
COPY go.mod ./
RUN --mount=type=cache,target=/go/pkg/mod,sharing=locked \
    go mod download
COPY . ./
RUN --mount=type=cache,target=/go/pkg/mod,sharing=locked \
    --mount=type=cache,target=/root/.cache/go-build,sharing=locked \
    CGO_ENABLED=0 GOOS=linux go build -trimpath -ldflags='-s -w -X main.version=local' -o /out/core-infra-gateway ./cmd/core-infra-gateway

FROM alpine:3.21

RUN addgroup -S -g 10001 coreinfra \
    && adduser -S -D -H -u 10001 -G coreinfra coreinfra

COPY --from=build /out/core-infra-gateway /usr/local/bin/core-infra-gateway
USER coreinfra:coreinfra

EXPOSE 7500
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 CMD wget -q -O /dev/null http://127.0.0.1:7500/health || exit 1

ENTRYPOINT ["/usr/local/bin/core-infra-gateway"]
