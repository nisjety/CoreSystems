#!/usr/bin/env bash
# WS06-S7: the execution-core container limits and sandbox spawn bounds exist in the base
# compose file and the production overlay, with the documented defaults.
#
# Pure text contract (no docker engine needed), in the style of the other compose contract tests.
# Set COMPOSE_CONFIG=1 on a host with docker to ALSO run `docker compose config` on both files.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
BASE="$ROOT_DIR/deploy/docker-compose.yml"
PROD="$ROOT_DIR/deploy/docker-compose.production.yml"

# The body of one top-level service: from `  <name>:` to the next two-space-indented key.
service_block() {
  awk -v svc="$2" '
    $0 ~ "^  " svc ":[[:space:]]*$" { on = 1; next }
    on && /^  [A-Za-z0-9_-]+:/ { on = 0 }
    on && /^[^ ]/ { on = 0 }
    on { print }
  ' "$1" | tr -d '\r'
}

fail() { echo "execution-core limits compose contract: $*" >&2; exit 1; }

expect_line() {
  local block="$1" needle="$2" label="$3"
  grep -F -- "$needle" <<<"$block" >/dev/null || fail "$label is missing: $needle"
}

base_block="$(service_block "$BASE" execution-core)"
prod_block="$(service_block "$PROD" execution-core)"
[[ -n "$base_block" ]] || fail "no execution-core service in docker-compose.yml"
[[ -n "$prod_block" ]] || fail "no execution-core service in docker-compose.production.yml"

limit_lines=(
  'mem_limit: ${EXECUTION_CORE_MEM_LIMIT:-3g}'
  'memswap_limit: ${EXECUTION_CORE_MEM_LIMIT:-3g}'
  'cpus: ${EXECUTION_CORE_CPUS:-3.0}'
  'pids_limit: ${EXECUTION_CORE_PIDS_LIMIT:-1536}'
  'nofile:'
  'soft: 16384'
  'hard: 16384'
)
for line in "${limit_lines[@]}"; do
  expect_line "$base_block" "$line" "base compose limit"
  expect_line "$prod_block" "$line" "production overlay limit"
done

# memswap_limit equal to mem_limit means no swap: a runaway child cannot page out.
mem="$(grep -F 'mem_limit:' <<<"$base_block" | grep -v memswap)"
swap="$(grep -F 'memswap_limit:' <<<"$base_block")"
[[ "${mem#*mem_limit:}" == "${swap#*memswap_limit:}" ]] || fail "memswap_limit must equal mem_limit"

# The sandbox spawn bounds are environment of the service, in the base file (the overlay inherits).
env_lines=(
  'EXECUTION_SANDBOX_MAX_OUTPUT_BYTES: ${EXECUTION_SANDBOX_MAX_OUTPUT_BYTES:-1048576}'
  'EXECUTION_SANDBOX_MAX_CONCURRENT: ${EXECUTION_SANDBOX_MAX_CONCURRENT:-8}'
  'EXECUTION_SANDBOX_QUEUE_WAIT_MS: ${EXECUTION_SANDBOX_QUEUE_WAIT_MS:-5000}'
)
for line in "${env_lines[@]}"; do
  expect_line "$base_block" "$line" "base compose sandbox bound"
done

# The overlay must not loosen a bound by restating it with a different default.
for key in EXECUTION_SANDBOX_MAX_OUTPUT_BYTES EXECUTION_SANDBOX_MAX_CONCURRENT EXECUTION_SANDBOX_QUEUE_WAIT_MS; do
  if grep -F "${key}:" <<<"$prod_block" >/dev/null; then
    base_value="$(grep -F "${key}:" <<<"$base_block" | head -1 | sed 's/^[[:space:]]*//')"
    prod_value="$(grep -F "${key}:" <<<"$prod_block" | head -1 | sed 's/^[[:space:]]*//')"
    [[ "$base_value" == "$prod_value" ]] || fail "production overlay restates $key with a different default"
  fi
done

if [[ "${COMPOSE_CONFIG:-0}" == "1" ]]; then
  docker compose -f "$BASE" config >/dev/null || fail "docker compose config failed for the base file"
  docker compose -f "$BASE" -f "$PROD" config >/dev/null || fail "docker compose config failed for the production overlay"
fi

echo "execution-core limits compose contract passed"
