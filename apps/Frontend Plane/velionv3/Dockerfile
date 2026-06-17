# Development Dockerfile with hot reload
FROM node:20-alpine
WORKDIR /app

RUN npm install -g pnpm

# Install curl for healthcheck
RUN apk add --no-cache curl

COPY package.json pnpm-lock.yaml ./
RUN pnpm install

# Copy source code (will be overridden by volume mount in dev)
COPY . .

# Expose Vite dev server port
EXPOSE 5173

# Run Vite dev server with HMR enabled
CMD ["pnpm", "dev", "--host", "0.0.0.0"]
