FROM node:22-slim AS deps

WORKDIR /app
ENV NPM_CONFIG_AUDIT=false \
    NPM_CONFIG_FUND=false \
    NPM_CONFIG_UPDATE_NOTIFIER=false
COPY package.json package-lock.json* ./
RUN npm ci --silent

FROM node:22-slim AS build

WORKDIR /app
ENV NPM_CONFIG_AUDIT=false \
    NPM_CONFIG_FUND=false \
    NPM_CONFIG_UPDATE_NOTIFIER=false
COPY --from=deps /app/node_modules ./node_modules
COPY package.json tsconfig.json vitest.config.ts ./
COPY migrations ./migrations
COPY scripts ./scripts
COPY src ./src
RUN npm run build \
 && ./node_modules/.bin/tsc --module NodeNext --moduleResolution NodeNext --target ES2022 --strict --esModuleInterop --types node --outDir dist/scripts scripts/seed-nango-providers.ts

FROM node:22-slim AS runtime

WORKDIR /app
ENV NODE_ENV=production \
    NPM_CONFIG_AUDIT=false \
    NPM_CONFIG_FUND=false \
    NPM_CONFIG_UPDATE_NOTIFIER=false
COPY package.json package-lock.json* ./
RUN npm ci --omit=dev --silent
COPY --from=build /app/dist ./dist
COPY migrations ./migrations
EXPOSE 3026
CMD ["node", "dist/server.js"]
