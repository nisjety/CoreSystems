# Convex Gateway Service - Local SQLite backend
FROM node:20-alpine

WORKDIR /app

# Install required tools
RUN apk add --no-cache wget curl bash

# Copy package files
COPY package*.json ./

# Install dependencies
RUN npm install

# Copy convex schema and functions
COPY convex/ ./convex/
COPY convex.config.ts ./
COPY tsconfig.json ./
COPY convex.json ./
COPY .env.local ./
COPY startup.sh ./
COPY nats-subscriber.js ./

# Make startup script executable
RUN chmod +x startup.sh

# Create data directory for SQLite
RUN mkdir -p /data

# Expose Convex dev server ports
# 3000 - HTTP API & Dashboard
EXPOSE 3000

# Health check - check if port 3000 is responding
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=3 \
  CMD wget --quiet --tries=1 --spider http://localhost:3000 || exit 1

# Start Convex dev server using the startup script
# This ensures it's configured for local deployment to bypass auth
CMD ["./startup.sh"]
