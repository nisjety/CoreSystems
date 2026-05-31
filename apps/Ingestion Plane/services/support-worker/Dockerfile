# ── Build stage ──────────────────────────────────────────────────────────────
# Use full Debian-based node so that @temporalio/worker's Rust native bindings
# can compile via node-gyp (Alpine is missing required glibc and build tools).
FROM node:20-slim AS builder

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
RUN npm ci --omit=dev --silent

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM node:20-slim AS runtime

WORKDIR /app

# Copy production node_modules (already pruned) and compiled output.
COPY --from=builder /app/node_modules ./node_modules
COPY --from=builder /app/dist ./dist
COPY --from=builder /app/package.json ./package.json

CMD ["node", "dist/index.js"]
