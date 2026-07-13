# syntax=docker/dockerfile:1.7
# Production image for shipping-core (migrated from Suplayer-System into the
# Ingestion Plane). Mirrors integration-corev2's build pattern; Dockerfile.dev
# (air hot-reload) remains for standalone development.
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
# FedEx's Akamai edge) fails with "tls: bad record MAC". Proven 2026-07-04:
# identical binary/network/target, amd64-under-Rosetta 8/8 fail, arm64 8/8 ok.
ARG TARGETOS TARGETARCH
RUN --mount=type=cache,target=/go/pkg/mod --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} go build -o /out/shipping-core ./cmd/shipping-core

FROM alpine:3.24.1
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.title="CoreSystem shipping-core" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"
RUN --mount=type=cache,target=/var/cache/apk \
    apk add --no-cache ca-certificates wget && \
    addgroup -S app && adduser -S app -G app
USER app
WORKDIR /app
COPY --from=build /out/shipping-core /app/shipping-core
EXPOSE 8080
ENTRYPOINT ["/app/shipping-core"]
