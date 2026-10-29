# syntax=docker/dockerfile:1.7
FROM golang:1.26.5-alpine3.24 AS builder

WORKDIR /src
COPY --from=studio-plane-client . /pkg/planeclient
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
# Build for the image's target platform, NOT a hardcoded amd64: an amd64 Go
# binary inside an arm64 image runs under Rosetta on Apple Silicon, whose AVX2
# emulation miscomputes Go's vectorised ChaCha20-Poly1305 and breaks TLS to
# endpoints that negotiate it (proven in shipping-core, 2026-07-04).
ARG TARGETOS TARGETARCH
RUN --mount=type=cache,target=/go/pkg/mod \
    --mount=type=cache,target=/root/.cache/go-build \
    CGO_ENABLED=0 GOOS=${TARGETOS} GOARCH=${TARGETARCH} go build -o /out/anbud-api ./cmd/api

FROM alpine:3.24.1
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.title="CoreSystem anbud-core" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"

RUN --mount=type=cache,target=/var/cache/apk \
    apk add --no-cache ca-certificates wget \
    && addgroup -S app \
    && adduser -S app -G app

WORKDIR /app
COPY --from=builder /out/anbud-api /usr/local/bin/anbud-api

USER app

EXPOSE 3138

CMD ["/usr/local/bin/anbud-api"]
