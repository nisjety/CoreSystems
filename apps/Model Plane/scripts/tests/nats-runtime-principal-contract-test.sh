#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
NATS_CONFIG="$ROOT_DIR/deploy/nats.conf"
COMPOSE="$ROOT_DIR/deploy/docker-compose.yml"
COMPOSE_OVERRIDE="$ROOT_DIR/deploy/docker-compose.override.yml"

fail() {
  printf 'NATS runtime-principal contract: %s\n' "$1" >&2
  exit 1
}

user_block() {
  awk -v user="$1" '
    $0 == "      user: \"" user "\"" { found = 1 }
    found { print }
    found && $0 == "    }" { exit }
  ' "$NATS_CONFIG"
}

service_block() {
  awk -v service="$1" '
    $0 == "  " service ":" { found = 1; next }
    found && $0 ~ /^  [A-Za-z0-9_-]+:$/ { exit }
    found { print }
  ' "$COMPOSE"
}

require_contains() {
  local haystack="$1"
  local needle="$2"
  [[ "$haystack" == *"$needle"* ]] || fail "missing $needle"
}

require_absent() {
  local haystack="$1"
  local needle="$2"
  [[ "$haystack" != *"$needle"* ]] || fail "unexpected $needle"
}

require_absent "$(<"$NATS_CONFIG")" 'user: "model-runtime"'
require_absent "$(<"$COMPOSE")" 'NATS_USER: model-runtime'
require_absent "$(<"$COMPOSE")" 'MODEL_NATS_RUNTIME_PASSWORD'

declare -a principals=(
  'model-gateway:model-gateway-runtime:MODEL_GATEWAY_NATS_PASSWORD:_INBOX.MODEL_GATEWAY_RUNTIME.>'
  'session-core:session-core-runtime:MODEL_SESSION_CORE_NATS_PASSWORD:_INBOX.SESSION_CORE_RUNTIME.>'
  'capability-core:capability-core-runtime:MODEL_CAPABILITY_CORE_NATS_PASSWORD:_INBOX.CAPABILITY_CORE_RUNTIME.>'
  'orchestrator-core:orchestrator-core-runtime:MODEL_ORCHESTRATOR_CORE_NATS_PASSWORD:_INBOX.ORCHESTRATOR_CORE_RUNTIME.>'
  'cost-core:cost-core-runtime:MODEL_COST_CORE_NATS_PASSWORD:_INBOX.COST_CORE_RUNTIME.>'
)

for principal in "${principals[@]}"; do
  IFS=: read -r service user password inbox <<<"$principal"
  compose_block="$(service_block "$service")"
  config_block="$(user_block "$user")"
  [[ -n "$compose_block" ]] || fail "missing compose service $service"
  [[ -n "$config_block" ]] || fail "missing NATS user $user"
  require_contains "$compose_block" "NATS_USER: $user"
  require_contains "$compose_block" "NATS_PASSWORD: \${$password:?$password is required}"
  require_contains "$config_block" "password: \$$password"
  require_contains "$config_block" "$inbox"
done

gateway="$(user_block model-gateway-runtime)"
for required in \
  '"mp.v1.run.*.event"' \
  '"mp.v1.stream.opened"' \
  '"mp.v1.stream.closed"' \
  '"mp.v1.usage.*"' \
  '"mp.v1.ingress.accepted"' \
  '"mp.v1.orchestration.run"' \
  '"mp.v1.feedback.rated"' \
  '"mp.v1.finetune.>"' \
  '"mp.v1.capability.>"' \
  '"dataplane.documents.indexed"'; do
  require_contains "$gateway" "$required"
done
for forbidden in \
  '"agents.>"' \
  '"org.>"' \
  '"notify.>"' \
  '"mp.v1.>"' \
  '"verevon.agent.>"' \
  '"verevon.session.>"' \
  '"aqencia.reasoning.>"'; do
  require_absent "$gateway" "$forbidden"
done

capability="$(user_block capability-core-runtime)"
require_contains "$capability" '"mp.v1.capability.>"'
require_contains "$capability" '"mp.v1.run.*.event"'
require_absent "$capability" '"mp.v1.>"'

orchestrator="$(user_block orchestrator-core-runtime)"
for required in '"mp.v1.run.*.event"' '"mp.v1.orchestration.>"' '"mp.v1.feedback.rated"'; do
  require_contains "$orchestrator" "$required"
done
for forbidden in '"verevon.agent.>"' '"verevon.session.>"' '"aqencia.reasoning.>"'; do
  require_absent "$orchestrator" "$forbidden"
done

tool_completion="$(user_block tool-completion-producer)"
require_contains "$tool_completion" '"tools.completions.*"'
require_absent "$tool_completion" '"mp.v1.'

for service in inference-core execution-core bridge-core; do
  block="$(service_block "$service")"
  require_absent "$block" 'NATS_URL:'
  require_absent "$block" 'NATS_USER:'
  require_absent "$block" 'NATS_PASSWORD:'
done

gateway_compose="$(service_block model-gateway)"
orchestrator_compose="$(service_block orchestrator-core)"
require_contains "$gateway_compose" 'MP_COMPAT_MODE: v1_only'
require_contains "$orchestrator_compose" 'MP_COMPAT_MODE: v1_only'
require_contains "$orchestrator_compose" 'ENABLE_COMPAT_ADAPTER: "false"'
require_absent "$(<"$COMPOSE_OVERRIDE")" 'ENABLE_COMPAT_ADAPTER: "true"'
require_contains "$(<"$COMPOSE_OVERRIDE")" 'ENABLE_COMPAT_ADAPTER: "false"'
require_contains "$(<"$COMPOSE_OVERRIDE")" 'MP_COMPAT_MODE: v1_only'

printf 'NATS runtime-principal contract: ok\n'
