# ── Build stage ──────────────────────────────────────────────────────────────
# Use full Debian-based node so that @temporalio/worker's Rust native bindings
# can compile via node-gyp (Alpine is missing required glibc and build tools).
FROM node:20-slim AS builder

WORKDIR /app

COPY package*.json ./
RUN npm ci

COPY tsconfig.json ./
COPY src ./src
RUN npm run build

# Prune to production deps only after the build is done.
RUN npm ci --omit=dev

# ── Runtime stage ─────────────────────────────────────────────────────────────
FROM node:20-slim AS runtime

WORKDIR /app

# Copy production node_modules (already pruned) and compiled output.
COPY --from=builder /app/node_modules ./node_modules
COPY --from=builder /app/dist ./dist
COPY --from=builder /app/package.json ./package.json

CMD ["node", "dist/index.js"]
