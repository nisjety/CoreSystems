#!/usr/bin/env bash
set -euo pipefail

# Source-only deployment contract for the governed tickets.create adapter.
# This test intentionally checks names/defaults, never values. It prevents a
# source implementation from being silently unreachable in the dev Compose
# stack while keeping the adapter fail-closed until credentials are provisioned.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/deploy/docker-compose.yml"
ENV_EXAMPLE="$ROOT_DIR/deploy/.env.example"

keys=(
  CONTROL_PLANE_USER_CORE_URL
  EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN
  CONVERSATION_CORE_AGENT_ACTION_URL
  CONVERSATION_EXECUTION_CORE_SERVICE_TOKEN
  EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN
  EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK
)

for key in "${keys[@]}"; do
  env_pattern="^${key}=$"
  if [[ "$key" == "EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK" ]]; then
    env_pattern="^${key}=false$"
  fi
  rg -q "$env_pattern" "$ENV_EXAMPLE" || {
    echo "missing empty .env.example declaration: $key" >&2
    exit 1
  }
  compose_mapping="${key}: \${${key}:-}"
  if [[ "$key" == "EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK" ]]; then
    compose_mapping="${key}: \${${key}:-false}"
  fi
  rg -Fq "$compose_mapping" "$COMPOSE_FILE" || {
    echo "missing empty-default Compose mapping: $key" >&2
    exit 1
  }
done

rg -q '^EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK=false$' "$ENV_EXAMPLE" || {
  echo "ticket transport loopback exception must default to false" >&2
  exit 1
}

if rg -n 'CONTROL_PLANE_USER_CORE_URL: [^$]|EXECUTION_CORE_CONTROL_RUN_ACTION_TOKEN: [^$]|CONVERSATION_CORE_AGENT_ACTION_URL: [^$]|CONVERSATION_EXECUTION_CORE_SERVICE_TOKEN: [^$]|EXECUTION_CORE_CONTROL_MODEL_ACTION_VIEW_TOKEN: [^$]' "$COMPOSE_FILE"; then
  echo "ticket adapter Compose mapping contains a literal value" >&2
  exit 1
fi

source_file="$ROOT_DIR/rust/services/execution-core/src/ticket_tools.rs"
for key in "${keys[@]}"; do
  rg -q "\"${key}\"" "$source_file" || {
    echo "Execution Core source no longer reads expected binding: $key" >&2
    exit 1
  }
done
rg -q 'EXECUTION_CORE_ALLOW_INSECURE_TICKET_LOOPBACK' "$source_file" || {
  echo "Execution Core source no longer reads the explicit loopback transport flag" >&2
  exit 1
}

echo "tickets.create config contract: exact bindings declared, empty by default, and fail-closed"
