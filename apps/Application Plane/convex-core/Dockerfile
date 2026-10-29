# syntax=docker/dockerfile:1.7
# Convex Gateway Service - Local SQLite backend
FROM node:20-alpine@sha256:fb4cd12c85ee03686f6af5362a0b0d56d50c58a04632e6c0fb8363f609372293
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"

WORKDIR /app

ENV NPM_CONFIG_AUDIT=false \
    NPM_CONFIG_FUND=false \
    NPM_CONFIG_UPDATE_NOTIFIER=false

# Install required tools
RUN set -e; \
    if ! apk add --no-cache --quiet wget curl bash procps > /tmp/apk-add.log 2>&1; then \
      cat /tmp/apk-add.log; \
      exit 1; \
    fi; \
    rm -f /tmp/apk-add.log

# Copy package files
COPY package*.json ./

# Install dependencies
RUN --mount=type=cache,target=/root/.npm \
    npm ci --omit=dev --no-audit --no-fund --loglevel=error

# Copy convex schema and functions
COPY convex/ ./convex/
COPY convex.config.ts ./
COPY tsconfig.json ./
COPY convex.json ./
COPY startup.sh ./
COPY nats-subscriber.js ./

# Make startup script executable
RUN chmod +x startup.sh

# Create data directory for SQLite
RUN mkdir -p /data

# This container is a schema/function deployer and watcher, not an HTTP proxy.
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
  CMD pgrep -f "convex dev" >/dev/null && wget --quiet --tries=1 -O /dev/null http://convex-backend:3210/version || exit 1

# Start Convex dev server using the startup script
# This ensures it's configured for local deployment to bypass auth
CMD ["./startup.sh"]
