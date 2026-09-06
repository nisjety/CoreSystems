# syntax=docker/dockerfile:1.7
FROM golang:1.26.5-alpine3.24 AS build

WORKDIR /src
COPY go.mod go.sum* ./
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    go mod download

COPY . .
# Build for the image's target platform, NOT hardcoded amd64: an amd64 Go
# binary inside an arm64 image executes under Rosetta on Apple Silicon, and
# Rosetta's AVX2 emulation miscomputes Go's vectorized ChaCha20-Poly1305 —
# every TLS connection to an endpoint that negotiates that cipher (e.g.
# FedEx's Akamai edge) fails with "tls: bad record MAC". Proven 2026-07-04
# in shipping-core: identical binary/network/target, amd64-under-Rosetta
# 8/8 fail, arm64 8/8 ok.
ARG TARGETOS TARGETARCH
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} go build -o /out/integration-corev2 ./cmd/api && \
    CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} go build -o /out/finspo-worker ./cmd/finspo-worker && \
    CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} go build -o /out/email-worker ./cmd/email-worker

FROM alpine:3.24.1 AS runtime-base
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.title="CoreSystem integration-corev2" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"
RUN --mount=type=cache,target=/var/cache/apk \
    apk add --no-cache ca-certificates wget && \
    addgroup -S app && adduser -S app -G app && \
    mkdir -p /var/lib/integration-core/codex-subscriptions && \
    chown -R app:app /var/lib/integration-core
USER app
WORKDIR /app

FROM runtime-base AS finspo-worker
COPY --from=build /out/finspo-worker /app/finspo-worker
ENTRYPOINT ["/app/finspo-worker"]

FROM runtime-base AS email-worker
COPY --from=build /out/email-worker /app/email-worker
ENTRYPOINT ["/app/email-worker"]

# Keep the HTTP API last so a direct `docker build` retains the historical
# default target, while Compose workers select their single-binary targets.
FROM runtime-base AS api
USER root
# Pin the official Codex CLI used by the ChatGPT subscription broker. Build
# automation may override this only after reviewing the release.
ARG CODEX_VERSION=0.153.4
RUN --mount=type=cache,target=/var/cache/apk \
    apk add --no-cache nodejs npm && \
    npm install --global --omit=dev "@openai/codex@${CODEX_VERSION}" && \
    codex --version
USER app
COPY --from=build /out/integration-corev2 /app/integration-corev2
EXPOSE 3026
ENTRYPOINT ["/app/integration-corev2"]
