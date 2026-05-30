FROM node:22-slim AS deps

WORKDIR /app
COPY package.json package-lock.json* ./
RUN npm install

FROM node:22-slim AS build

WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY package.json tsconfig.json vitest.config.ts ./
COPY migrations ./migrations
COPY scripts ./scripts
COPY src ./src
RUN npm run build \
 && npx tsc --module NodeNext --moduleResolution NodeNext --target ES2022 --strict --esModuleInterop --types node --outDir dist/scripts scripts/seed-nango-providers.ts

FROM node:22-slim AS runtime

WORKDIR /app
ENV NODE_ENV=production
COPY package.json package-lock.json* ./
RUN npm install --omit=dev
COPY --from=build /app/dist ./dist
COPY migrations ./migrations
EXPOSE 3026
CMD ["node", "dist/server.js"]