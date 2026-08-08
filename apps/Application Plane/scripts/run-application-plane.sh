#!/usr/bin/env bash
set -euo pipefail

# Local-development runner for the Application Plane — mirrors run-control-plane.sh
# and run-ingestion-plane.sh. No tracked root .env: each core owns its own
# <core>/.env (also injected per-service via `env_file:` in compose), and shared/
# infra + cross-plane secrets live in a local, gitignored, 0600 store
# ($root/.env.generated-secrets). Compose interpolation runs before env_file
# injection, so this runner merges every service-local file plus the secrets store
# into a temp env-file passed via --env-file, and supplies development values for
# any missing required interpolation variables. Generated credentials are persisted
# once and reused (stable across bring-ups + per-service recreates). Never writes
# credentials to a tracked file; must not be used with a production overlay.

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"

# Per-core environment files (each core also loads its own via `env_file:`).
env_files=(
  "$root/convex-core/.env"
  "$root/conversation-core/.env"
  "$root/notification-core/.env"
  "$root/social-core/.env"
  "$root/insight-core/.env"
  "$root/leads-core/.env"
  "$root/information-core/.env"
)
# convex services also keep a local overlay if present.
[[ -f "$root/convex-core/.env.local" ]] && env_files+=("$root/convex-core/.env.local")
for file in "${env_files[@]}"; do
  [[ -f "$file" ]] || { printf 'missing service environment file: %s\n' "$file" >&2; exit 1; }
done

tmp_env=$(mktemp "${TMPDIR:-/tmp}/application-plane-compose.XXXXXX")
cleanup() { rm -f "$tmp_env"; }
trap cleanup EXIT INT TERM
chmod 600 "$tmp_env"

secrets_file="${APPLICATION_PLANE_SECRETS_FILE:-$root/.env.generated-secrets}"
if [[ ! -f "$secrets_file" ]]; then umask 077; : >"$secrets_file"; fi
chmod 600 "$secrets_file"
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
set_if_missing() { local k=$1 v=$2; lookup_value "$k" >/dev/null 2>&1 || printf '%s=%s\n' "$k" "$v" >>"$tmp_env"; }
persist_if_missing() { local k=$1 v=$2; lookup_value "$k" >/dev/null 2>&1 || printf '%s=%s\n' "$k" "$v" >>"$secrets_file"; }
random_value() { openssl rand -hex 32; }

set_if_missing SOURCE_REVISION "$(git -C "$root" rev-parse HEAD 2>/dev/null || echo local)"
set_if_missing BUILD_DATE "$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# Safety net: generate + persist any app-internal required secret still missing so
# a bring-up never fails on an unset ${VAR:?}. Cross-plane shared creds are already
# provisioned (copied from Control/Model), so those are found by lookup_value.
required_generated=(
  APPLICATION_CONVERSATION_NATS_PASSWORD APPLICATION_INSIGHT_NATS_PASSWORD
  APPLICATION_LEADS_NATS_PASSWORD APPLICATION_NOTIFICATION_NATS_PASSWORD
  APPLICATION_SOCIAL_NATS_PASSWORD APPLICATION_DATA_PLANE_KNOWLEDGE_NATS_PASSWORD
  APPLICATION_PLANE_DB_PASSWORD
  APPLICATION_PLANE_DRAGONFLY_PASSWORD AFFINE_POSTGRES_PASSWORD GRAFANA_ADMIN_PASSWORD
  JWT_SECRET CONVEX_INSTANCE_SECRET CONVEX_INTERNAL_SERVICE_KEY CONVEX_RECONCILIATION_KEY
  CONVERSATION_CORE_INGEST_SERVICE_TOKEN CONVERSATION_EMAIL_INGEST_SERVICE_TOKEN
  CONVERSATION_GATEWAY_SERVICE_TOKEN CONVERSATION_INTEGRATION_SERVICE_API_KEY
  NOTIFICATION_GATEWAY_SERVICE_TOKEN
)
for key in "${required_generated[@]}"; do persist_if_missing "$key" "$(random_value)"; done

compose_env_args=(--env-file "$tmp_env")
for file in "${env_files[@]}"; do compose_env_args+=(--env-file "$file"); done

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
