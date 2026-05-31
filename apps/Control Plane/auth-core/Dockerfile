# =============================================================
# auth-core — Node.js / NestJS service
# Build: node:20-alpine (builder) → node:20-alpine (runtime)
# =============================================================

# ── Builder ──────────────────────────────────────────────────
FROM node:20-alpine AS builder
ENV PNPM_CONFIG_UPDATE_NOTIFIER=false \
    NPM_CONFIG_UPDATE_NOTIFIER=false \
    CI=true

# Use corepack for zero-install pnpm (faster, cached by Docker layer)
RUN corepack enable && corepack prepare pnpm@10 --activate

WORKDIR /app

# Install deps first — only invalidated when lockfile changes
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile --loglevel=error

# Copy source and build
COPY . .
RUN pnpm exec nest build
RUN test -f "dist/src/main.js"

# Prune dev-dependencies so we only copy prod deps to runtime
RUN pnpm prune --prod --ignore-scripts --loglevel=error

# ── Runtime ──────────────────────────────────────────────────
FROM node:20-alpine

# Add postgresql-client BEFORE copying application files
# (keeps this layer cached when app code changes)
RUN set -e; \
    if ! apk add --no-cache --quiet postgresql-client > /tmp/apk-add.log 2>&1; then \
        cat /tmp/apk-add.log; \
        exit 1; \
    fi; \
    rm -f /tmp/apk-add.log; \
    addgroup -S appgroup && adduser -S appuser -G appgroup

WORKDIR /app

# Copy only what is needed at runtime
COPY --from=builder /app/dist        ./dist
COPY --from=builder /app/proto       ./proto
COPY --from=builder /app/drizzle     ./drizzle
COPY --from=builder /app/node_modules ./node_modules
COPY --from=builder /app/package*.json ./
COPY --from=builder /app/migrations  ./migrations
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh && \
    chown -R appuser:appgroup /app

ENV NODE_ENV=production

# HTTP + gRPC
EXPOSE 3011 50011

USER appuser

ENTRYPOINT ["/bin/sh", "/app/docker-entrypoint.sh"]
CMD ["node", "dist/src/main.js"]
