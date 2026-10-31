#!/usr/bin/env bash
set -euo pipefail

# Reproducible source proof for the agent action admission contract (WS24-S5 to WS24-S8): the one
# golden every plane vendors, the execution-core pins and offers for owner actions and owner reads,
# the Ask-loop read arms in model-gateway, the admission table in capability-core, the run-action
# target table in user-core, and the Frontend side map that generates the golden. This is a dev/test
# harness only. It never reads, creates, rotates, or injects a service credential, and it never sets
# MODEL_ACTION_ADMISSION or any other switch: the unit tests set their own switches in-process. The
# live proof (rebuilt images, the switch on the proof stack only) is the lead's, and the closing lines
# list it; nothing here runs it.
#
# The capability-core admission tests named with the integration tag start their own disposable
# Postgres through testcontainers, so Docker is needed for that one step and no DSN is passed here.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APPS_DIR="$ROOT_DIR/.."
RUST_MANIFEST="$ROOT_DIR/rust/Cargo.toml"
CAPABILITY_CORE_DIR="$ROOT_DIR/go/services/capability-core"
USER_CORE_DIR="$APPS_DIR/Control Plane/user-core"
FRONTEND_DIR="$APPS_DIR/Frontend Plane/verevonv3"
CANONICAL_CONTRACT="$ROOT_DIR/api/contracts/agent-action-admission.json"
TMP_DIR="$(mktemp -d)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# A step that runs nothing must not pass. `cargo test` with a filter that matches no test reports
# "0 passed", and `go test -run` exits 0 with "no tests to run"; a skipped integration test exits 0
# too. Each Go and Cargo step therefore requires at least one test to pass, and no test to fail or skip.
blocked() {
  local name="$1" log="$2"
  printf 'BLOCKED %s\n' "$name"
  sed -n '1,220p' "$log"
  exit 2
}

run_cargo_proof() {
  local name="$1"
  shift
  local log="$TMP_DIR/${name}.log"
  printf 'PROOF %s\n' "$name"
  if "$@" >"$log" 2>&1 && grep -Eq 'test result: ok\. [1-9][0-9]* passed' "$log"; then
    printf 'PASS  %s\n' "$name"
  else
    blocked "$name" "$log"
  fi
}

run_go_proof() {
  local name="$1" dir="$2" pattern="$3"
  shift 3
  local log="$TMP_DIR/${name}.log"
  printf 'PROOF %s\n' "$name"
  if (cd "$dir" && go test -count=1 -v -run "$pattern" "$@") >"$log" 2>&1 &&
    grep -q -- '--- PASS' "$log" &&
    ! grep -Eq -- '--- (FAIL|SKIP)' "$log"; then
    printf 'PASS  %s (%s tests)\n' "$name" "$(grep -c -- '--- PASS' "$log")"
  else
    blocked "$name" "$log"
  fi
}

run_check() {
  local name="$1"
  shift
  local log="$TMP_DIR/${name}.log"
  printf 'PROOF %s\n' "$name"
  if "$@" >"$log" 2>&1; then
    printf 'PASS  %s\n' "$name"
  else
    blocked "$name" "$log"
  fi
}

# The proof checks goldens; it must never regenerate one, or it would compare a golden with itself.
no_golden_rewrite() {
  local switch
  for switch in VEREVON_UPDATE_GOLDEN MP_UPDATE_GOLDEN UPDATE_CONTRACT_GOLDEN; do
    if [[ -n "${!switch:-}" ]]; then
      printf '%s is set: a golden test would rewrite the golden it checks\n' "$switch"
      return 1
    fi
  done
}

# The admission contract is generated once and vendored byte for byte by every plane that admits an
# agent action. Every row closes ZDR: no row may admit a zero-retention run.
ADMISSION_COPIES=(
  "$APPS_DIR/Frontend Plane/verevonv3/src/shared/api/contracts/agent-action-admission.json"
  "$CAPABILITY_CORE_DIR/internal/api/testdata/agent-action-admission.json"
  "$ROOT_DIR/rust/services/execution-core/tests/fixtures/agent-action-admission.json"
  "$ROOT_DIR/rust/services/model-gateway/tests/fixtures/agent-action-admission.json"
)
admission_contract() {
  local copy
  if [[ ! -f "$CANONICAL_CONTRACT" ]]; then
    printf 'the canonical admission contract is missing: %s\n' "$CANONICAL_CONTRACT"
    return 1
  fi
  if grep -Eq '"zdr_allowed": *true' "$CANONICAL_CONTRACT"; then
    printf 'a row of the admission contract admits zero retention: %s\n' "$CANONICAL_CONTRACT"
    return 1
  fi
  if [[ "$(grep -Ec '"zdr_allowed": *false' "$CANONICAL_CONTRACT")" -eq 0 ]]; then
    printf 'no row of the admission contract carries zdr_allowed false: %s\n' "$CANONICAL_CONTRACT"
    return 1
  fi
  for copy in "${ADMISSION_COPIES[@]}"; do
    if ! cmp -s "$CANONICAL_CONTRACT" "$copy"; then
      printf 'a vendored copy differs from the canonical admission contract: %s\n' "$copy"
      return 1
    fi
  done
}

# Frontend: the side map and the registry generate the contract, and the test compares the generated
# text with the golden. The test must run; a missing test file or no test found is a failure.
frontend_admission() {
  (cd "$FRONTEND_DIR" && pnpm vitest run src/shared/actions/admission-contract.test.ts src/shared/actions/model-eligibility.test.ts)
}

run_check no_golden_rewrite no_golden_rewrite
run_check admission_contract_vendored_and_zdr_closed admission_contract
run_check frontend_admission_contract frontend_admission

# Execution Core: the admission contract as the executor reads it, the owner pins (name, schema hash,
# parameters, adapter) that a server-resolved action must match, the Do-catalogue invariants that every
# offered tool has a capability binding and an attestor, and the one place a server-resolved owner action
# may be offered.
run_cargo_proof execution_core_admission_contract \
  cargo test --manifest-path "$RUST_MANIFEST" -p execution-core --test agent_action_admission_contract
run_cargo_proof execution_core_owner_pins_and_reads \
  cargo test --manifest-path "$RUST_MANIFEST" -p execution-core --lib owner_
run_cargo_proof execution_core_server_resolved_offer \
  cargo test --manifest-path "$RUST_MANIFEST" -p execution-core --lib only_the_server_resolved_view_may_offer_ticket_create
run_cargo_proof execution_core_offered_tool_invariants \
  cargo test --manifest-path "$RUST_MANIFEST" -p execution-core --lib every_offered_tool

# Model Gateway: every Ask-loop read arm is a contract row and every contract read has an arm.
run_cargo_proof model_gateway_ask_read_arms \
  cargo test --manifest-path "$RUST_MANIFEST" -p model-gateway --lib every_ask_read

# Capability Core: the admission table seeds only the enabled ticket row, the signed model action view
# offers only enabled rows and refuses hash mismatches and zero retention, and the integration seed
# applies every migration and checks that only tickets.create is ever offered.
run_go_proof capability_core_admission_view "$CAPABILITY_CORE_DIR" 'ModelActionView|AgentActionAdmission' ./internal/api/
run_go_proof capability_core_admission_table_postgres "$CAPABILITY_CORE_DIR" 'AgentActionAdmission' -tags integration ./internal/registry/

# User Core: the run-action target table is the only source of an action's audience, permission and
# hash basis, the issued view intersects that table with the entitlement, and zero retention is refused.
run_go_proof user_core_run_action_targets "$USER_CORE_DIR" 'RunActionTarget|IssueModelActionView|RunActionDecision' ./internal/spaces/ ./internal/http/

printf 'agent action admission proof: contract vendored and closed to ZDR, Frontend side map, execution-core pins and offers, model-gateway Ask reads, capability-core and user-core admission tables passed\n'
printf 'Not run by this script (lead, on rebuilt images; MODEL_ACTION_ADMISSION=table only on the proof stack, never on the product route):\n'
printf '  - tickets-create-proof.sh with MODEL_ACTION_ADMISSION=table: the same receipt and the same offered tool list as tickets_only\n'
printf '  - the decoded model action view claims (no secrets) and the offered tool list, from the rebuilt capability-core, user-core and execution-core\n'
printf '  - knowledge.list_documents in a Do chat: SSE tool_call and tool_result, the EvaluatePolicy evidence for cap.retrieval.query, the Data Plane access log naming the delegated user, and a second user who sees only the rows their access allows\n'
printf '  - a zero-retention run that must not be offered tickets.create or knowledge.list_documents, and a waived run that must not be offered them either\n'
