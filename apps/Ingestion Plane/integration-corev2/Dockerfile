# syntax=docker/dockerfile:1.7
FROM golang:1.25-alpine AS build

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
    CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} go build -o /out/finspo-worker ./cmd/finspo-worker

FROM alpine:3.21
RUN --mount=type=cache,target=/var/cache/apk \
    addgroup -S app && adduser -S app -G app
USER app
WORKDIR /app
COPY --from=build /out/integration-corev2 /app/integration-corev2
COPY --from=build /out/finspo-worker /app/finspo-worker
EXPOSE 3026
ENTRYPOINT ["/app/integration-corev2"]
