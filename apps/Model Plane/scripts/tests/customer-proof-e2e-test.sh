#!/usr/bin/env bash
set -euo pipefail

# These are the four release-gate scenarios that make a Model Plane claim
# customer-testable. They run the real gateway HTTP stack against in-process
# session/inference mocks, so this is an integration harness rather than a
# production deployment claim. A staging run must repeat the same scenarios
# with the release artifact and real cross-plane services.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR/rust"

run_case() {
  local target="$1" name="$2"
  echo "customer-proof: ${name}"
  cargo test -p model-gateway --test "$target" "$name" -- --exact
}

# Grounded answer: context assembly reaches inference and is retained in the
# resulting request instead of silently falling back to an ungrounded answer.
run_case e2e_invoke_chain_test invoke_stream_uses_context_assembly_segments_before_inference

# Reconnect durability: a client disconnect does not lose the completed
# assistant message or terminal run state.
run_case e2e_invoke_chain_test invoke_stream_completes_and_persists_after_the_client_disconnects_mid_stream

# ZDR safety: a signed ZDR posture cannot be downgraded by a request or route
# override before provider I/O.
run_case e2e_invoke_chain_test ai_unary_routes_cannot_downgrade_signed_zdr_posture

# Approval boundary: a cross-tenant caller cannot decide another tenant's
# approval, while the owning tenant remains able to make the decision.
run_case orchestration_http_test decide_approval_is_scoped_to_the_callers_org

echo "customer-proof e2e scenarios: ok"
