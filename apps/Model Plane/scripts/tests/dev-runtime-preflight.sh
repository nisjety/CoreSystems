#!/usr/bin/env bash
set -euo pipefail

# Read-only dev runtime preflight for the scheduled-step authority lane.
# It inspects container health and the presence of named configuration keys,
# but never prints values, creates credentials, or mutates a service.

usage() {
  cat <<'EOF'
Usage: scripts/tests/dev-runtime-preflight.sh

Checks the local Docker dev stack for the scheduled-run/step authority inputs.
Exit 0 means the named inputs are present; exit 2 means the lane must remain
fail-closed. No credential values are printed or changed.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi
if [[ $# -ne 0 ]]; then
  usage >&2
  exit 2
fi

command -v docker >/dev/null 2>&1 || {
  echo "BLOCKED docker: command is unavailable"
  exit 2
}

expiry=""
if expiry="$(date -u -v+15M '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null)"; then
  :
else
  expiry="$(date -u -d '+15 minutes' '+%Y-%m-%dT%H:%M:%SZ')"
fi

find_container() {
  local service="$1" fallback="$2" name
  name="$(docker ps --filter "label=com.docker.compose.service=$service" --format '{{.Names}}' | head -n 1)"
  if [[ -n "$name" ]]; then
    printf '%s\n' "$name"
  else
    printf '%s\n' "$fallback"
  fi
}

container_exists() {
  docker inspect "$1" >/dev/null 2>&1
}

container_health() {
  local name="$1" state health
  state="$(docker inspect --format '{{.State.Status}}' "$name" 2>/dev/null || printf 'missing')"
  health="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}no-healthcheck{{end}}' "$name" 2>/dev/null || printf 'missing')"
  printf '%s/%s\n' "$state" "$health"
}

env_state() {
  local name="$1" key
  key="$2"
  docker inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$name" 2>/dev/null |
    awk -F= -v wanted="$key" '$1 == wanted { found=1; print (length($0) > length($1) + 1 ? "set" : "empty"); exit } END { if (!found) print "missing" }'
}

blockers=()
check_container() {
  local label="$1" name="$2" health
  if ! container_exists "$name"; then
    blockers+=("$label container missing ($name)")
    printf 'BLOCKED %-12s container=%s state=missing\n' "$label" "$name"
    return
  fi
  health="$(container_health "$name")"
  printf 'CHECKED  %-12s container=%s state=%s\n' "$label" "$name" "$health"
  if [[ "$health" != running/healthy && "$health" != running/no-healthcheck ]]; then
    blockers+=("$label container is not healthy ($name: $health)")
  fi
}

check_key() {
  local label="$1" name="$2" key="$3" state
  state="$(env_state "$name" "$key")"
  printf 'CONFIG   %-12s key=%s state=%s\n' "$label" "$key" "$state"
  if [[ "$state" != set ]]; then
    blockers+=("$label requires $key ($state)")
  fi
}

capability="$(find_container capability-core model-plane-capability-core-1)"
orchestrator="$(find_container orchestrator-core model-plane-orchestrator-core-1)"
session="$(find_container session-core model-plane-session-core-1)"
execution="$(find_container execution-core model-plane-execution-core-1)"
control="$(find_container user-core user-service)"

printf 'dev scheduled-runtime preflight; expires=%s\n' "$expiry"
check_container capability-core "$capability"
check_container orchestrator-core "$orchestrator"
check_container session-core "$session"
check_container execution-core "$execution"
check_container control-user-core "$control"

check_key capability-core "$capability" CONTROL_USER_CORE_URL
check_key capability-core "$capability" CAPABILITY_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN
check_key capability-core "$capability" CONTROL_SPACE_DECISION_KEY_ID
check_key capability-core "$capability" CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64

check_key orchestrator-core "$orchestrator" CONTROL_USER_CORE_URL
check_key orchestrator-core "$orchestrator" ORCHESTRATOR_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN
check_key orchestrator-core "$orchestrator" ORCHESTRATOR_CORE_CONTROL_SCHEDULE_STEP_SERVICE_TOKEN
check_key orchestrator-core "$orchestrator" CONTROL_SPACE_DECISION_KEY_ID
check_key orchestrator-core "$orchestrator" CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64

check_key session-core "$session" CONTROL_SPACE_DECISION_KEY_ID
check_key session-core "$session" CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64
check_key execution-core "$execution" EXECUTION_CORE_SCHEDULED_STEP_DECISION_KEY_ID
check_key execution-core "$execution" EXECUTION_CORE_SCHEDULED_STEP_DECISION_PUBLIC_KEY_BASE64

if ((${#blockers[@]} > 0)); then
  printf 'STATUS blocked; scheduled effects remain fail-closed\n'
  printf 'BLOCKER %s\n' "${blockers[@]}"
  exit 2
fi

printf 'STATUS ready-for-disposable-proof; this does not establish candidate or provider/ZDR evidence\n'
