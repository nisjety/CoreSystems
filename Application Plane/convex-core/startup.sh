#!/bin/bash

# Convex startup script for Docker
# Handles non-interactive configuration and starts the dev server

set -e

echo "[Convex] Starting gateway service..."

# Force Convex self-hosted bypass using official env vars
export CONVEX_SELF_HOSTED_URL="http://convex-backend:3210"
# Use the generated admin key if present in .env.local
export CONVEX_SELF_HOSTED_ADMIN_KEY="${CONVEX_ADMIN_KEY:-}"

set_convex_env() {
  local name="$1"
  local value="$2"

  if [ -z "$value" ]; then
    return
  fi

  echo "[Convex] Setting env $name"
  npx convex env set "$name" "$value"
}

set_convex_env "CONVEX_AUTH_ISSUER" "${CONVEX_AUTH_ISSUER:-http://localhost:3011/api/convex-auth}"
set_convex_env "CONVEX_AUTH_JWKS_URL" "${CONVEX_AUTH_JWKS_URL:-http://auth-service:3011/api/convex-auth/jwks}"
set_convex_env "CONVEX_AUTH_AUDIENCE" "${CONVEX_AUTH_AUDIENCE:-coresystem-convex}"
set_convex_env "CONVEX_INTERNAL_SERVICE_KEY" "${CONVEX_INTERNAL_SERVICE_KEY:-change-me-internal-service-secret}"
set_convex_env "AI_CORE_URL" "${AI_CORE_URL:-http://ai-core:8000}"
set_convex_env "ORG_CORE_URL" "${ORG_CORE_URL:-http://org-core-service:8080}"
set_convex_env "AUTH_SERVER_URL" "${AUTH_SERVER_URL:-http://auth-service:3011}"

echo "[Convex] Starting dev server attached to local backend..."
# Run dev server in foreground so container stays up
npx convex dev
