# ── Build stage ──────────────────────────────────────────────────────────────
# Use full Debian-based node so that @temporalio/worker's Rust native bindings
# can compile via node-gyp (Alpine is missing required glibc and build tools).
FROM node:22.23.1-bookworm-slim AS builder

WORKDIR /app
ENV NPM_CONFIG_AUDIT=false \
    NPM_CONFIG_FUND=false \
    NPM_CONFIG_UPDATE_NOTIFIER=false

COPY package*.json ./
RUN npm ci --silent

COPY tsconfig.json ./
COPY src ./src
RUN npm run build

# Prune to production deps only after the build is done.
RUN npm prune --omit=dev --silent

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM node:22.23.1-bookworm-slim AS runtime
ARG SOURCE_REVISION=unverified
ARG BUILD_DATE=unknown
LABEL org.opencontainers.image.title="CoreSystem support-worker" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.created="${BUILD_DATE}"

WORKDIR /app

# Copy production node_modules (already pruned) and compiled output.
COPY --from=builder --chown=node:node /app/node_modules ./node_modules
COPY --from=builder --chown=node:node /app/dist ./dist
COPY --from=builder --chown=node:node /app/package.json ./package.json

ENV NODE_ENV=production
USER node
CMD ["node", "dist/index.js"]
