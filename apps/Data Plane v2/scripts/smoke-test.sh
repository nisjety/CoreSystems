#!/usr/bin/env bash
set -euo pipefail
set +x

# Read-only smoke test for an isolated/local Data Plane Compose project.
# Default transport executes curl inside each private service container, so no
# application port needs to be published on the host. External transport is
# restricted to explicit loopback URLs.

PASS=0
FAIL=0
WITH_AUTH_MATRIX=false

case "${1:-}" in
  ""|--health-only|--http-only) ;;
  --with-auth-matrix) WITH_AUTH_MATRIX=true ;;
  *)
    printf 'usage: %s [--health-only|--with-auth-matrix]\n' "$0" >&2
    exit 2
    ;;
esac

pass() {
  printf '  PASS %-24s %-8s status=%s\n' "$1" "$2" "$3"
  PASS=$((PASS + 1))
}

fail() {
  printf '  FAIL %-24s %-8s status=%s\n' "$1" "$2" "$3" >&2
  FAIL=$((FAIL + 1))
}

configuration_error() {
  printf 'configuration error: %s\n' "$1" >&2
  exit 2
}

TRANSPORT=${DPV2_SMOKE_TRANSPORT:-compose}
case "$TRANSPORT" in
  compose)
    command -v docker >/dev/null 2>&1 || configuration_error "docker is required for compose transport"
    ;;
  external)
    command -v curl >/dev/null 2>&1 || configuration_error "curl is required for external transport"
    ;;
  *)
    configuration_error "DPV2_SMOKE_TRANSPORT must be compose or external"
    ;;
esac

is_loopback_url() {
  case "$1" in
    http://localhost:*|http://127.0.0.1:*|http://\[::1\]:*) return 0 ;;
    *) return 1 ;;
  esac
}

base_url() {
  local port=$1
  local env_name=$2

  if [ "$TRANSPORT" = "compose" ]; then
    printf 'http://127.0.0.1:%s' "$port"
    return
  fi

  local value=${!env_name:-}
  [ -n "$value" ] || configuration_error "$env_name is required for external transport"
  is_loopback_url "$value" || configuration_error "$env_name must be an explicit loopback HTTP endpoint"
  printf '%s' "${value%/}"
}

run_transport() {
  local service=$1
  if [ "$TRANSPORT" = "compose" ]; then
    docker compose exec -T "$service" curl --config - 2>/dev/null
  else
    curl --config - 2>/dev/null
  fi
}

status_for() {
  local service=$1
  local url=$2
  {
    printf 'silent\n'
    printf 'output = "/dev/null"\n'
    printf 'write-out = "%%{http_code}"\n'
    printf 'connect-timeout = 3\n'
    printf 'max-time = 15\n'
    printf 'request = "GET"\n'
    printf 'url = "%s"\n' "$url"
    printf 'header = "Accept: application/json"\n'
  } | run_transport "$service"
}

check() {
  local service=$1
  local base=$2
  local path=$3
  local status
  status=$(status_for "$service" "$base$path" || true)
  if [ "$status" = "200" ]; then
    pass "$service" "$path" "$status"
  else
    [ -n "$status" ] || status=transport-error
    fail "$service" "$path" "$status"
  fi
}

printf 'Data Plane v2 isolated read-only smoke test\n'
printf 'Transport: %s (response bodies suppressed)\n\n' "$TRANSPORT"

while IFS='|' read -r service port env_name; do
  base=$(base_url "$port" "$env_name")
  check "$service" "$base" /health
  check "$service" "$base" /readyz
done <<'SERVICES'
retrieval-engine|8004|DPV2_RETRIEVAL_URL
documents-api|8010|DPV2_DOCUMENTS_URL
wiki-store|8011|DPV2_WIKI_URL
data-orchestrator|8012|DPV2_ORCHESTRATOR_URL
data-quality|8013|DPV2_QUALITY_URL
graph-index|9203|DPV2_GRAPH_URL
quickwit-adapter|9204|DPV2_QUICKWIT_URL
SERVICES

printf '\nResults: %s passed, %s failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ] || exit 1

if $WITH_AUTH_MATRIX; then
  printf '\n'
  DPV2_MATRIX_TRANSPORT="$TRANSPORT" "$(dirname "$0")/auth-regression-matrix.sh"
fi
