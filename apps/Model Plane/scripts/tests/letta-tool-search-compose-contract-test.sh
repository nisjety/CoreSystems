#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMPOSE="$ROOT_DIR/deploy/docker-compose.yml"
ENV_EXAMPLE="$ROOT_DIR/deploy/.env.example"

capability_block="$(sed -n '/^  capability-core:/,/^  sandbox-manager:/p' "$COMPOSE")"

required_compose_variables=(
  LETTA_TOOL_SEARCH_ENABLED
  LETTA_TOOL_SEARCH_URL
  LETTA_API_KEY
  LETTA_TOOL_SEARCH_MODE
  LETTA_TOOL_SEARCH_TIMEOUT
  LETTA_TOOL_SEARCH_LIMIT
  LETTA_TOOL_SEARCH_MAX_RESPONSE_BYTES
  LETTA_TOOL_SEARCH_ALLOW_INSECURE_LOOPBACK
)

for variable in "${required_compose_variables[@]}"; do
  grep -F "${variable}:" <<<"$capability_block" >/dev/null
  grep -E "^${variable}=" "$ENV_EXAMPLE" >/dev/null
done

grep -F 'LETTA_TOOL_SEARCH_ENABLED: ${LETTA_TOOL_SEARCH_ENABLED:-false}' <<<"$capability_block" >/dev/null
grep -F 'LETTA_API_KEY: ${LETTA_API_KEY:-}' <<<"$capability_block" >/dev/null
grep -E '^LETTA_TOOL_SEARCH_ENABLED=false$' "$ENV_EXAMPLE" >/dev/null
grep -E '^LETTA_API_KEY=$' "$ENV_EXAMPLE" >/dev/null

echo "Letta tool-search compose contract passed"
