#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMMAND="$ROOT_DIR/scripts/coresystem-cross-plane-preflight.sh"

[[ -x "$COMMAND" ]] || {
  echo "missing executable: $COMMAND" >&2
  exit 1
}

help_output="$($COMMAND --help)"
grep -q "read-only" <<<"$help_output"
grep -q "credential" <<<"$help_output"

set +e
output="$($COMMAND 2>&1)"
status=$?
set -e

[[ "$status" -eq 2 ]] || {
  echo "expected current cross-plane preflight to be blocked (status 2), got $status" >&2
  printf '%s\n' "$output" >&2
  exit 1
}

grep -q "CHECKED control-user" <<<"$output"
grep -q "CHECKED application-conversation" <<<"$output"
grep -q "CHECKED frontend-gateway" <<<"$output"
grep -q "CHECKED model-execution" <<<"$output"
grep -q "CONFIG  application-conversation.*CONTROL_RUN_ACTION_DECISION_KEY_ID" <<<"$output"
grep -q "CONFIG  model-execution.*CONVERSATION_CORE_AGENT_ACTION_URL" <<<"$output"
grep -q "CONFIG  application-conversation.*CAPABILITY_CORE_HTTP_URL" <<<"$output"
grep -q "CONFIG  application-conversation.*CONVERSATION_CAPABILITY_HEALTH_SERVICE_API_KEY" <<<"$output"
grep -q "AUTHZ.*capability-core.*spaces:schedule:reauthorize" <<<"$output"
grep -q "AUTHZ.*orchestrator-core.*spaces:schedule:step" <<<"$output"
grep -q "AUTHZ.*conversation-core.*spaces:agent-action:reservation" <<<"$output"
grep -q "EVIDENCE provider_zdr.*state=open" <<<"$output"
grep -q "EVIDENCE approval_continuation.*state=open" <<<"$output"
grep -q "EVIDENCE application_delivery_ha.*state=open" <<<"$output"
grep -q "EVIDENCE capability_owner_reporter.*state=open" <<<"$output"
grep -q "EVIDENCE owner_effect_observation.*state=open" <<<"$output"
grep -q "STATUS blocked" <<<"$output"
grep -q "BLOCKER provider_zdr_evidence_open" <<<"$output"

# The report may expose service/key names and states, never secret-shaped
# assignments or values.
if grep -Eq '(TOKEN|SECRET|PRIVATE_KEY|BASE64)[A-Z_]*=[^[:space:]]+' <<<"$output"; then
  echo "cross-plane preflight leaked a secret-shaped assignment" >&2
  exit 1
fi

echo "CoreSystem cross-plane preflight contracts: ok"
