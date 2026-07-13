# syntax=docker/dockerfile:1.7
# =============================================================
# auth-core — Node.js / NestJS service
# Build: Node.js 22 LTS (builder) → Node.js 22 LTS (runtime)
# =============================================================

# ── Builder ──────────────────────────────────────────────────
FROM node:22.23.1-alpine3.23@sha256:8516dce0483394d5708d4b2ee6cacb79fb1d617ea4e2787c2120bcca92ce372e AS builder
ENV PNPM_CONFIG_UPDATE_NOTIFIER=false \
    NPM_CONFIG_UPDATE_NOTIFIER=false \
    CI=true

# Use corepack for zero-install pnpm (faster, cached by Docker layer)
RUN corepack enable && corepack prepare pnpm@11.3.0 --activate

WORKDIR /app

# Install deps first — only invalidated when lockfile changes
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY patches ./patches
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile --loglevel=error

# Copy source and build
COPY . .
RUN pnpm exec nest build
RUN test -f "dist/src/main.js"

# Prune dev-dependencies so we only copy prod deps to runtime
RUN pnpm prune --prod --ignore-scripts --loglevel=error

# ── Runtime ──────────────────────────────────────────────────
FROM node:22.23.1-alpine3.23@sha256:8516dce0483394d5708d4b2ee6cacb79fb1d617ea4e2787c2120bcca92ce372e
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.revision=$SOURCE_REVISION \
      org.opencontainers.image.created=$BUILD_DATE
ENV SOURCE_REVISION=$SOURCE_REVISION BUILD_DATE=$BUILD_DATE

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

# Copy only what is needed at runtime. Assign ownership while copying so image
# assembly does not recursively rewrite the full pnpm dependency tree.
COPY --chown=appuser:appgroup --from=builder /app/dist         ./dist
COPY --chown=appuser:appgroup --from=builder /app/proto        ./proto
COPY --chown=appuser:appgroup --from=builder /app/drizzle      ./drizzle
COPY --chown=appuser:appgroup --from=builder /app/node_modules ./node_modules
COPY --chown=appuser:appgroup --from=builder /app/package*.json ./
COPY --chown=appuser:appgroup --from=builder /app/migrations   ./migrations
COPY --chown=appuser:appgroup --chmod=755 docker-entrypoint.sh /app/docker-entrypoint.sh

ENV NODE_ENV=production

# HTTP + gRPC
EXPOSE 3011 50011

USER appuser

ENTRYPOINT ["/bin/sh", "/app/docker-entrypoint.sh"]
CMD ["node", "dist/src/main.js"]
