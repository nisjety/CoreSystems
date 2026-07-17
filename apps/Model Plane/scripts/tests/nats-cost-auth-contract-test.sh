#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
NATS_CONFIG="$ROOT_DIR/deploy/nats.conf"
COMPOSE="$ROOT_DIR/deploy/docker-compose.yml"

gateway_block="$(sed -n '/user: "model-gateway-runtime"/,/user: "session-core-runtime"/p' "$NATS_CONFIG")"
cost_block="$(sed -n '/user: "cost-core-runtime"/,/user: "application-convex-model"/p' "$NATS_CONFIG")"
cost_compose="$(sed -n '/^  cost-core:/,/^  bridge-core:/p' "$COMPOSE")"

if grep -Fq 'user: "model-runtime"' "$NATS_CONFIG"; then
  printf 'generic model-runtime principal must not remain configured\n' >&2
  exit 1
fi
grep -F '"mp.v1.usage.*"' <<<"$gateway_block" >/dev/null
grep -F 'publish: []' <<<"$cost_block" >/dev/null
grep -F '"mp.v1.usage.*"' <<<"$cost_block" >/dev/null
grep -F '_INBOX.COST_CORE_RUNTIME.>' <<<"$cost_block" >/dev/null
grep -F 'NATS_USER: cost-core-runtime' <<<"$cost_compose" >/dev/null
grep -F 'NATS_PASSWORD: ${MODEL_COST_CORE_NATS_PASSWORD:?MODEL_COST_CORE_NATS_PASSWORD is required}' <<<"$cost_compose" >/dev/null

echo "cost usage NATS authorization contract: ok"
