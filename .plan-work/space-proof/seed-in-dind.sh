#!/usr/bin/env bash
# Run the M0-T0 seed INSIDE the nested daemon so every `docker exec <container>` it performs hits the private stack, never the live one.
# Usage: seed-in-dind.sh [seed args...]   (default: print the plan)
set -u
ARGS="${*:-}"
docker exec space-proof-dind bash -c '
set -u
cd /src
sec() { grep -m1 "^$1=" "/src/apps/Control Plane/.env.generated-secrets" | cut -d= -f2- | tr -d "\r\"\047"; }
UIP=$(docker inspect user-service --format "{{(index .NetworkSettings.Networks \"controlplane-net\").IPAddress}}")
export E2E_BASE_URL=http://localhost:5173 AUTH_CORE_URL=http://localhost:3011 ORG_CORE_URL=http://127.0.0.1:18080 USER_CORE_URL=http://127.0.0.1:3021
export CONTROL_SPACE_POLICY_TOKEN="$(sec CONTROL_SPACE_POLICY_TOKEN)"
export ORG_CORE_SERVICE_TOKEN="$(sec GATEWAY_ORG_CORE_SERVICE_TOKEN)"
bash scripts/space-e2e-seed.sh '"$ARGS"' 2>&1 | sed -E "s/(token|secret|password|key)=[^ ]+/\1=<redacted>/Ig" | tail -60
'
