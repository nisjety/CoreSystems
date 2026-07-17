#!/usr/bin/env bash
set -euo pipefail

# Local-development runner for the Ingestion Plane — mirrors the Control Plane's
# run-control-plane.sh model. There is NO tracked root .env: each core owns its
# own <core>/.env (also injected per-service via `env_file:` in compose), and
# shared/infra + cross-plane secrets live in a local, gitignored, 0600 store
# ($root/.env.generated-secrets). Compose interpolation happens before env_file
# injection, so this runner merges every service-local file plus the secrets
# store into a temp env-file passed via --env-file, and supplies development
# values for any missing required interpolation variables. Generated credentials
# are persisted once and reused, so they stay stable across bring-ups and
# per-service recreates. Never writes credentials to a tracked file; must not be
# used with the production overlay.

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

if [[ " ${*:-} " == *"docker-compose.production.yml"* ]]; then
  printf 'refusing production overlay: use the external secret-manager rollout\n' >&2
  exit 2
fi

# Per-core environment files (each core also loads its own via `env_file:`).
env_files=(
  "$root/Quarry-v2/.env"
  "$root/autocomplete-core/.env"
  "$root/finspo-core/.env"
  "$root/imports-core/.env"
  "$root/integration-corev2/.env"
  "$root/shipping-core/.env"
)
for file in "${env_files[@]}"; do
  [[ -f "$file" ]] || { printf 'missing service environment file: %s\n' "$file" >&2; exit 1; }
done

tmp_env=$(mktemp "${TMPDIR:-/tmp}/ingestion-plane-compose.XXXXXX")
cleanup() { rm -f "$tmp_env"; }
trap cleanup EXIT INT TERM
chmod 600 "$tmp_env"

# Persisted local-development secret store (gitignored via .env.*). Keeps
# generated values STABLE across runs and safe for per-service recreates.
secrets_file="${INGESTION_PLANE_SECRETS_FILE:-$root/.env.generated-secrets}"
if [[ ! -f "$secrets_file" ]]; then
  umask 077
  : >"$secrets_file"
fi
chmod 600 "$secrets_file"
# Read back last: later --env-file wins in compose interpolation, and appearing
# in env_files lets lookup_value/persist_if_missing find already-persisted keys.
env_files+=("$secrets_file")

lookup_value() {
  local key=$1 file line value
  for file in "${env_files[@]}"; do
    line=$(grep -E "^${key}=" "$file" 2>/dev/null | tail -n 1 || true)
    [[ -n "$line" ]] || continue
    value=${line#*=}
    if [[ -n "$value" ]]; then printf '%s' "$value"; return 0; fi
  done
  return 1
}

set_if_missing() {
  local key=$1 value=$2
  if ! lookup_value "$key" >/dev/null 2>&1; then
    printf '%s=%s\n' "$key" "$value" >>"$tmp_env"
  fi
}

# Persist a generated value to the secrets store so it survives across runs.
persist_if_missing() {
  local key=$1 value=$2
  if ! lookup_value "$key" >/dev/null 2>&1; then
    printf '%s=%s\n' "$key" "$value" >>"$secrets_file"
  fi
}

random_value() { openssl rand -hex 32; }

# Build metadata (disposable per-run).
set_if_missing SOURCE_REVISION "$(git -C "$root" rev-parse HEAD 2>/dev/null || echo local)"
set_if_missing BUILD_DATE "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# Safety net: every compose-required (${VAR:?}) interpolation must resolve.
# All known ones are provisioned in the per-core/secrets files; generate + persist
# any that are still missing so a bring-up never fails on an unset required var.
required_generated=(
  AUTH_CORE_INTERNAL_API_KEY AUTOCOMPLETE_INTERNAL_TOKEN CONNECTOR_RUNTIME_SECRET
  CONVERSATION_EMAIL_INGEST_SERVICE_TOKEN FINSPO_SERVICE_API_KEY IMPORTS_SERVICE_API_KEY
  INGESTION_DRAGONFLY_PASSWORD INGESTION_NATS_TOKEN INGESTION_PG_PASSWORD
  INTEGRATION_AUDIT_CORE_SERVICE_TOKEN INTEGRATION_BILLING_CORE_SERVICE_TOKEN
  INTEGRATION_CREDENTIALS_ENCRYPTION_KEY INTEGRATION_ORG_CORE_SERVICE_TOKEN
  INTEGRATION_SERVICE_API_KEY INTERNAL_API_KEY QDRANT_API_KEY QUARRY_CONTROL_API_KEY
  QUARRY_INTERNAL_SECRET QUARRY_RUNTIME_AUTH_TOKEN QUARRY_SERVICE_API_KEY
  SEARXNG_SECRET SONIC_PASSWORD
)
for key in "${required_generated[@]}"; do
  persist_if_missing "$key" "$(random_value)"
done

compose_env_args=(--env-file "$tmp_env")
for file in "${env_files[@]}"; do
  compose_env_args+=(--env-file "$file")
done

# Ensure the cross-plane shared network exists for up/start (Control/Model own it
# in a full stack; create a local bridge if it is not already present).
case " ${*:-} " in
  *" up "*|*" start "*)
    if ! docker network inspect inter-plane-bus >/dev/null 2>&1; then
      docker network create --driver bridge \
        --label com.docker.compose.network=inter-plane-bus \
        --label com.docker.compose.project=triodelab \
        inter-plane-bus >/dev/null
    fi
    ;;
esac

exec docker compose "${compose_env_args[@]}" -f docker-compose.yml "$@"
