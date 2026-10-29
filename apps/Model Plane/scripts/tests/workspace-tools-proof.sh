#!/usr/bin/env bash
set -euo pipefail

# Reproducible source and disposable-Postgres proof for the Do-mode workspace tools (WS24-S3 reads,
# WS24-S4 overlay writes) and their zero-retention gates. This is a dev/test harness only. It never
# reads, creates, rotates, or injects a service credential, and it never sets WORKSPACE_TOOLS: the
# unit tests set the switch inside their own process, so no live stack gets a flag from here. The
# live proof (rebuilt images, a Space Do run, the overlay rows in sandbox-manager) is the lead's, and
# the closing lines list it; nothing here runs it.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUST_MANIFEST="$ROOT_DIR/rust/Cargo.toml"
CAPABILITY_CORE_DIR="$ROOT_DIR/go/services/capability-core"
TMP_DIR="$(mktemp -d)"
POSTGRES_CONTAINER="model-plane-workspace-tools-pg-${RANDOM}-$$"
POSTGRES_DATABASE="capability_core_test"

cleanup() {
  docker rm -f "$POSTGRES_CONTAINER" >/dev/null 2>&1 || true
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# A step that runs nothing must not pass. `cargo test` with a filter that matches no test reports
# "0 passed", and `go test -run` exits 0 with "no tests to run"; a skipped integration test exits 0
# too. Each step therefore requires at least one test to pass, and no test to fail or skip.
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

# A disposable Postgres bound to loopback only, with trust authentication so that no password is
# generated or stored. It is removed by the EXIT trap whatever happens.
start_postgres() {
  docker create --pull=never --name "$POSTGRES_CONTAINER" -p 127.0.0.1::5432 \
    -e POSTGRES_HOST_AUTH_METHOD=trust \
    -e POSTGRES_DB="$POSTGRES_DATABASE" \
    postgres:16-alpine >/dev/null || return 1
  docker start "$POSTGRES_CONTAINER" >/dev/null || return 1

  # Docker Desktop publishes the host port after the container is up, and the forwarder can lag
  # behind pg_isready. Wait for both, within two minutes.
  local port=""
  for _ in $(seq 1 120); do
    port="$(docker port "$POSTGRES_CONTAINER" 5432/tcp 2>/dev/null | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
    if [[ -n "$port" ]] && docker exec "$POSTGRES_CONTAINER" pg_isready -U postgres -d "$POSTGRES_DATABASE" >/dev/null 2>&1; then
      printf '%s\n' "$port"
      return 0
    fi
    sleep 1
  done
  return 1
}

# Execution Core: the edit engine (content versions, the closed EditError codes, the workspace lock),
# the tool gates (absent without a Space, absent under zero retention, absent while the switch is off,
# and no tool name that the permission substring rule would treat as a write), and the edit contract
# that replays a version chain end to end.
run_cargo_proof execution_core_workspace_tools \
  cargo test --manifest-path "$RUST_MANIFEST" -p execution-core --lib workspace
run_cargo_proof execution_core_workspace_edit_contract \
  cargo test --manifest-path "$RUST_MANIFEST" -p execution-core --test workspace_edit_contract

# Capability Core: cap.workspace.read and cap.workspace.write are seeded unattested, the read row is
# read-only, and the write row is a separate row an organisation can switch off on its own.
run_go_proof capability_core_workspace_registry "$CAPABILITY_CORE_DIR" 'Workspace' ./internal/registry/

PORT="$(start_postgres)" || {
  printf 'BLOCKED postgres_start\n'
  exit 2
}
export CAPABILITY_CORE_MIGRATION_TEST_DATABASE_URL="postgres://postgres@127.0.0.1:${PORT}/${POSTGRES_DATABASE}?sslmode=disable"
run_go_proof capability_core_workspace_migrations_postgres "$CAPABILITY_CORE_DIR" 'Workspace' -tags integration ./internal/registry/

# DEC-24 D1: nothing promotes a run's overlay into its Space until the owner accepts overlay-only
# durability. The sandbox-manager client is the only code that may name the PromoteWorkspace RPC, so
# any other caller is a change to this check, reviewed as a product decision.
promote_boundary() {
  local callers
  callers="$(grep -rln --include='*.rs' --exclude-dir=target -e 'promote_workspace(' -e 'PromoteWorkspace(' "$ROOT_DIR/rust" |
    grep -v 'services/execution-core/src/sandbox_manager_client.rs' || true)"
  if [[ -n "$callers" ]]; then
    printf 'PromoteWorkspace is called outside sandbox_manager_client.rs:\n%s\n' "$callers"
    return 1
  fi
}
run_check workspace_promote_boundary promote_boundary

printf 'workspace tools proof: edit engine, tool gates, capability-core registry and migrations, and the single PromoteWorkspace boundary passed\n'
printf 'Not run by this script (lead, on rebuilt images; the switch only on the proof stack, never on the product route):\n'
printf '  - a Space Do run that searches and edits the workspace, with SSE tool_call and tool_result and the step audit\n'
printf '  - the overlay rows in sandbox-manager workspace_files: run_id set, base_hash equal to the version token\n'
printf '  - a second run in the same Space that must not see the overlay, and a plan-mode run that must refuse the edit\n'
printf '  - a zero-retention Space run that must show no workspace tool and refuse any call with the workspace refusal\n'
printf '  - the rebuilt stack with WORKSPACE_TOOLS=read, then =all, and the capability health proof for cap.workspace.read and cap.workspace.write\n'
