#!/usr/bin/env bash
set -euo pipefail

# Source-only deployment contract for model.schedule.step v1. This checks the
# exact empty-default bindings and scope names required by the source lanes;
# it never reads or prints credential values and never enables the lane.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMPOSE_FILE="$ROOT_DIR/deploy/docker-compose.yml"
ENV_EXAMPLE="$ROOT_DIR/deploy/.env.example"

keys=(
  CONTROL_USER_CORE_URL
  CONTROL_SPACE_DECISION_KEY_ID
  CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64
  CAPABILITY_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN
  ORCHESTRATOR_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN
  ORCHESTRATOR_CORE_CONTROL_SCHEDULE_STEP_SERVICE_TOKEN
)

for key in "${keys[@]}"; do
  rg -q "^${key}=$" "$ENV_EXAMPLE" || {
    echo "missing empty .env.example declaration: $key" >&2
    exit 1
  }
  rg -Fq "${key}: \${${key}:-}" "$COMPOSE_FILE" || {
    echo "missing empty-default Compose mapping: $key" >&2
    exit 1
  }
done

scope_contracts=(
  'session:schedule-prepare'
  'session:runs:system-owner'
  'session:scheduled-step'
  'session:scheduled-step-authority'
  'model:schedule:step'
)
for scope in "${scope_contracts[@]}"; do
  rg -q "$scope" "$ENV_EXAMPLE" || {
    echo "missing documented Auth Core scheduled scope: $scope" >&2
    exit 1
  }
done

rg -q 'sessionCoreScopes = \[\].*session:schedule-prepare' \
  "$ROOT_DIR/go/services/capability-core/cmd/main.go" || {
  echo "Capability Core source no longer requests session:schedule-prepare" >&2
  exit 1
}
rg -q 'session:runs:system-owner' \
  "$ROOT_DIR/go/services/orchestrator-core/internal/servicecred/minters.go" || {
  echo "Orchestrator source no longer requests system-owned Session runs" >&2
  exit 1
}
rg -q 'SCHEDULED_STEP_SCOPE.*session:scheduled-step' \
  "$ROOT_DIR/rust/services/session-core/src/auth.rs" || {
  echo "Session Core scheduled-step scope contract is missing" >&2
  exit 1
}
rg -q 'SCHEDULED_STEP_AUTHORITY_SCOPE.*session:scheduled-step-authority' \
  "$ROOT_DIR/rust/services/session-core/src/auth.rs" || {
  echo "Session Core scheduled-step authority scope contract is missing" >&2
  exit 1
}
rg -q 'model:schedule:step' \
  "$ROOT_DIR/go/services/orchestrator-core/internal/servicecred/minters.go" || {
  echo "Orchestrator source no longer requests the dedicated Execution scope" >&2
  exit 1
}
for source in \
  "$ROOT_DIR/proto/model_plane/v1/runs.proto" \
  "$ROOT_DIR/rust/services/session-core/src/run_service_grpc.rs" \
  "$ROOT_DIR/../Control Plane/user-core/internal/clients/session_run_action_authority.go" \
  "$ROOT_DIR/../Control Plane/user-core/internal/http/spaces.go"; do
  rg -q 'ResolveScheduledStepAuthority' "$source" || {
    echo "scheduled-step authority read is missing from $source" >&2
    exit 1
  }
done

echo "scheduled-step config contract: exact bindings and Auth Core scopes declared, empty by default, and fail-closed"
