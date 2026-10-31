#!/usr/bin/env bash
# WS04-S9 compose contract for the model-gateway run-control and replica switches.
#
# Since the WS04-S9 flip (two-replica live proof 2026-10-27) four switches default to the literal `true`
# (MODEL_GATEWAY_RUN_CONTROL_BUS, MODEL_GATEWAY_DURABLE_PLAN_GRANT, MODEL_GATEWAY_SHARED_TURN_RECORD,
# MODEL_GATEWAY_SEND_RECEIPTS) and MODEL_GATEWAY_SHARED_IDEMPOTENCY stays `false` until its own two-replica
# duplicate-send proof. The switch parser accepts only `true` (any case), so a default of `on`, `1` or `yes`
# would read as enabled in the file and stay off at runtime; this script refuses those defaults.
# This script reads only the compose file: it reads no .env file and runs no docker command.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE="$SCRIPT_DIR/../../deploy/docker-compose.yml"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

[ -f "$COMPOSE" ] || fail "compose file not found: $COMPOSE"
echo "ok: compose file found"

# switch=expected default
switches=(
  MODEL_GATEWAY_RUN_CONTROL_BUS=true
  MODEL_GATEWAY_DURABLE_PLAN_GRANT=true
  MODEL_GATEWAY_SHARED_TURN_RECORD=true
  MODEL_GATEWAY_SEND_RECEIPTS=true
  MODEL_GATEWAY_SHARED_IDEMPOTENCY=false
)

for entry in "${switches[@]}"; do
  switch="${entry%%=*}"
  want="${entry#*=}"
  environment_line="${switch}: \${${switch}:-${want}}"
  count="$(grep -F -c -- "$environment_line" "$COMPOSE" || true)"
  [ "$count" = "1" ] || fail "$switch: expected exactly one '$environment_line', found ${count:-0}"
  echo "ok: $switch defaults to $want, once"

  for other_default in "\${${switch}:-on}" "\${${switch}:-1}" "\${${switch}:-yes}"; do
    if grep -F -q -- "$other_default" "$COMPOSE"; then
      fail "$switch must not default to $other_default"
    fi
  done
  echo "ok: $switch has no on, 1 or yes default"
done

echo "WS04-S9 run-control switch compose contract passed"
