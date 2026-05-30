# syntax=docker/dockerfile:1.7

FROM golang:1.25-alpine AS builder
WORKDIR /src

RUN apk add --no-cache git ca-certificates

COPY . .

RUN CGO_ENABLED=0 GOOS=linux go build -mod=vendor -trimpath -ldflags='-s -w' -o /out/quarry-api ./cmd/api
RUN CGO_ENABLED=0 GOOS=linux go build -mod=vendor -trimpath -ldflags='-s -w' -o /out/quarry-worker ./cmd/worker
RUN CGO_ENABLED=0 GOOS=linux go build -mod=vendor -trimpath -ldflags='-s -w' -o /out/quarry-browser ./cmd/browser

# ── Runtime ──────────────────────────────────────────────────────────────────
FROM alpine:3.21 AS runtime
WORKDIR /app

# Chromium + fonts + non-root user in one merged layer
RUN apk add --no-cache ca-certificates tzdata chromium nss freetype harfbuzz ttf-freefont \
    curl openssl && \
    addgroup -S appgroup && adduser -S appuser -G appgroup

ENV ROD_BROWSER_BIN=/usr/bin/chromium
ENV ROD_CHROMIUM_BIN=/usr/bin/chromium

COPY --from=builder /out/quarry-api /usr/local/bin/quarry-api
COPY --from=builder /out/quarry-worker /usr/local/bin/quarry-worker
COPY --from=builder /out/quarry-browser /usr/local/bin/quarry-browser

# Chromium requires write access to /tmp for profile and cache dirs
RUN chown appuser:appgroup /app && chmod 1777 /tmp && \
    # some apps expect chromium-browser; make a reliable symlink
    if [ ! -e /usr/bin/chromium-browser ] && [ -e /usr/bin/chromium ]; then \
    ln -s /usr/bin/chromium /usr/bin/chromium-browser; \
    fi

USER appuser

CMD ["/usr/local/bin/quarry-api"]
