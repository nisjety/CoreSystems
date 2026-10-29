# syntax=docker/dockerfile:1.7
FROM golang:1.25.11-alpine3.24 AS builder
WORKDIR /workspace
COPY . .
WORKDIR /workspace/services/bridge-core
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build go mod download
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 go build -trimpath -ldflags="-s -w" -o /service ./cmd/

FROM alpine:3.24
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
RUN --mount=type=cache,target=/var/cache/apk apk add ca-certificates curl \
    && addgroup -S -g 10001 modelplane \
    && adduser -S -D -H -u 10001 -G modelplane modelplane
COPY --from=builder /service /usr/local/bin/service
USER modelplane
ENTRYPOINT ["service"]
