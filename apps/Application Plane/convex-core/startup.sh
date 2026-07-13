#!/bin/bash

# Convex startup script for Docker
# Handles non-interactive configuration and starts the dev server

set -e

echo "[Convex] Starting gateway service..."

# Never deploy a predictable shared key into Convex. Compose injects this at
# runtime from the ignored environment file; it is intentionally not baked into
# the image.
: "${CONVEX_INTERNAL_SERVICE_KEY:?CONVEX_INTERNAL_SERVICE_KEY must be set}"
: "${CONVEX_RECONCILIATION_KEY:?CONVEX_RECONCILIATION_KEY must be set}"

# Force Convex self-hosted bypass using official env vars
export CONVEX_SELF_HOSTED_URL="http://convex-backend:3210"
# Use the generated admin key if present in .env.local
export CONVEX_SELF_HOSTED_ADMIN_KEY="${CONVEX_ADMIN_KEY:-}"

retry_convex_cli() {
  local description="$1"
  shift
  local attempts="${CONVEX_CLI_RETRIES:-6}"
  local delay="${CONVEX_CLI_RETRY_DELAY_SECONDS:-2}"
  local output_file
  local attempt

  for attempt in $(seq 1 "$attempts"); do
    output_file="$(mktemp)"
    if "$@" >"$output_file" 2>&1; then
      rm -f "$output_file"
      return 0
    fi

    if [ "$attempt" -ge "$attempts" ]; then
      cat "$output_file" >&2
      rm -f "$output_file"
      echo "[Convex] ERROR: $description failed after $attempts attempts" >&2
      return 1
    fi

    rm -f "$output_file"
    echo "[Convex] Retrying $description ($attempt/$attempts)"
    sleep "$delay"
    if [ "$delay" -lt 10 ]; then
      delay=$((delay * 2))
    fi
  done
}

echo "[Convex] Waiting for convex-backend to be reachable..."
RETRY=0
while ! curl -sf "$CONVEX_SELF_HOSTED_URL/version" >/dev/null 2>&1; do
  RETRY=$((RETRY + 1))
  if [ "$RETRY" -ge 30 ]; then
    echo "[Convex] ERROR: backend did not become reachable after 30 attempts" >&2
    exit 1
  fi
  sleep 2
done

set_convex_env() {
  local name="$1"
  local value="$2"

  if [ -z "$value" ]; then
    return
  fi

  echo "[Convex] Setting env $name"
  retry_convex_cli "setting env $name" npx convex env set "$name" "$value"
}

set_convex_env "CONVEX_AUTH_ISSUER" "${CONVEX_AUTH_ISSUER:-http://localhost:3011/api/convex-auth}"
set_convex_env "CONVEX_AUTH_JWKS_URL" "${CONVEX_AUTH_JWKS_URL:-http://auth-core:3011/api/convex-auth/jwks}"
set_convex_env "CONVEX_AUTH_AUDIENCE" "${CONVEX_AUTH_AUDIENCE:-coresystem-convex}"
set_convex_env "CONVEX_INTERNAL_SERVICE_KEY" "$CONVEX_INTERNAL_SERVICE_KEY"
set_convex_env "CONVEX_RECONCILIATION_KEY" "$CONVEX_RECONCILIATION_KEY"
set_convex_env "AI_CORE_URL" "${AI_CORE_URL:-http://ai-core:8000}"
# Canonical Control Plane service names are `org-core` + `auth-core`
# (velion-gap.md G7). The legacy `org-core-service` / `auth-service`
# aliases still resolve in docker-compose but are being phased out.
set_convex_env "ORG_CORE_URL" "${ORG_CORE_URL:-http://org-core:8080}"
set_convex_env "AUTH_SERVER_URL" "${AUTH_SERVER_URL:-http://auth-core:3011}"

# ─────────────────────────────────────────────────────────────────────────
# Force-deploy functions before starting dev mode.
#
# Rationale (velion ui-ux-velion-gap.md §10 + this fix):
# `npx convex dev` only pushes changes when source files mutate. When the
# `convex-backend` container is recreated (or its SQLite volume is wiped)
# its function registry comes up empty — `dev`'s file-watcher won't see
# any changes to push, so velion gets "Could not find public function for
# 'controlSessions:byUser'" until something edits a file under convex/.
#
# Running `convex deploy` first guarantees the backend's registry matches
# the on-disk source on every cold start of this container. Idempotent —
# no-ops when the deployment is already in sync.
echo "[Convex] Deploying functions to $CONVEX_SELF_HOSTED_URL (force, idempotent)..."
retry_convex_cli "deploying functions" npx convex deploy --yes
echo "[Convex] Deploy succeeded."

echo "[Convex] Starting dev server attached to local backend..."
# Run dev server in foreground so container stays up
npx convex dev
