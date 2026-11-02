#!/usr/bin/env bash
set -uo pipefail

# Source-level proof of the durable agent loop (build plan WS03 S9): the shared
# types, the posture matrix held against the durable gate, the cross-service
# contract guards, the executor and gateway units of the fold, the closers, the
# resume and the park, and the session-core stores run against a disposable
# Postgres. It is integration evidence, not deployed evidence. It never reads,
# creates, rotates or prints a service credential, it starts no part of the
# stack and it never touches a running CoreSystem database: the Postgres stage
# is delegated to session-core-db-tests.sh, which owns the throwaway container.
#
# Usage: durable-loop-proof.sh [static | db]
#   (no argument)   both stages, static first
#   static          the cargo stages only; needs no Docker
#   db              the session-core DB stage only; needs a responding Docker engine
#
# Exit codes: 0 every selected stage passed; 1 a stage failed; 2 BLOCKED (the
# toolchain or the Docker engine is unavailable, or a bad argument). A BLOCKED
# stage proves nothing and the summary says so.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DB_HARNESS="$ROOT_DIR/scripts/tests/session-core-db-tests.sh"

mode="${1:-all}"
case "$mode" in
  all | static | db) ;;
  *)
    echo "BLOCKED usage: durable-loop-proof.sh [static | db]" >&2
    exit 2
    ;;
esac

passed=()
failed=()
blocked=()

# One stage: a name, then the command. A failure is recorded and the next stage
# still runs, so one red stage does not hide the others.
run_stage() {
  local name="$1"
  shift
  printf '\n=== %s\n' "$name"
  if "$@"; then
    passed+=("$name")
  else
    failed+=("$name")
  fi
}

# Run from the workspace, as the harness does, so cargo is never handed a converted path.
cargo_stage() {
  (cd "$ROOT_DIR/rust" && cargo test -q "$@")
}

if [[ "$mode" != db ]]; then
  command -v cargo >/dev/null 2>&1 || {
    echo "BLOCKED cargo is unavailable" >&2
    exit 2
  }
  [[ -f "$ROOT_DIR/rust/Cargo.toml" ]] || {
    echo "BLOCKED $ROOT_DIR/rust/Cargo.toml not found" >&2
    exit 2
  }

  # The shared loop-fold types, their golden fixtures and the retention matrix.
  run_stage "mp-contracts: loop fold types" \
    cargo_stage -p mp-contracts --lib loop_fold
  run_stage "mp-contracts: loop fold goldens" \
    cargo_stage -p mp-contracts --test loop_fold_golden
  run_stage "mp-contracts: retention matrix" \
    cargo_stage -p mp-contracts --test zdr_retention_matrix

  # The two files the S9 row names, then the guards the plan lists beside them.
  # Explicit --test targets: execution-core has non-lib targets that do not
  # build on Windows, so "--all-targets" would hide these behind them.
  run_stage "execution-core: posture matrix against the durable gate" \
    cargo_stage -p execution-core --test loop_fold_zdr_matrix
  run_stage "execution-core: durable loop contract" \
    cargo_stage -p execution-core --test durable_loop_contract
  run_stage "execution-core: two loops stay separate" \
    cargo_stage -p execution-core --test cross_service_loop_contract
  run_stage "execution-core: autonomy ladder, scrub, compaction parity" \
    cargo_stage -p execution-core --test autonomy_ladder_contract \
      --test scrub_checkpoint_test --test compaction_parity_contract

  # The executor's own units of the loop: fold writer, closers, effect ledger,
  # resume and park.
  run_stage "execution-core lib: fold writer" \
    cargo_stage -p execution-core --lib runtime_loop::fold
  run_stage "execution-core lib: closers and effect ledger" \
    cargo_stage -p execution-core --lib -- runtime_loop::closers runtime_loop::effect
  run_stage "execution-core lib: resume and park" \
    cargo_stage -p execution-core --lib -- runtime_loop::resume runtime_loop::park

  # The gateway side: the matrix driven over every bearer, the pre-consume and
  # the /continue preflight.
  run_stage "model-gateway: posture matrix and pre-consume" \
    cargo_stage -p model-gateway --test zdr_posture_matrix --test continue_preconsume_test
  run_stage "model-gateway lib: /continue" \
    cargo_stage -p model-gateway --lib continue_
fi

if [[ "$mode" != static ]]; then
  # The stores, on a throwaway Postgres the harness starts and removes. Each
  # filter carries the number of ignored DB tests that exist for it today, so a
  # renamed or removed test lowers the count and fails the stage instead of
  # leaving a green run that proves less.
  if [[ ! -f "$DB_HARNESS" ]]; then
    blocked+=("session-core stores on a disposable Postgres: harness not found")
  else
    printf '\n=== session-core stores on a disposable Postgres\n'
    bash "$DB_HARNESS" \
      approval_consume=7 \
      loop_checkpoint=24 \
      tool_ledger=13 \
      park=15
    db_status=$?
    if [[ "$db_status" -eq 0 ]]; then
      passed+=("session-core stores on a disposable Postgres")
    elif [[ "$db_status" -eq 2 ]]; then
      blocked+=("session-core stores on a disposable Postgres: BLOCKED (exit 2, see the harness output)")
    else
      failed+=("session-core stores on a disposable Postgres (exit $db_status)")
    fi
  fi
fi

printf '\n=== durable loop proof summary\n'
for name in "${passed[@]+"${passed[@]}"}"; do
  printf 'PASS    %s\n' "$name"
done
for name in "${failed[@]+"${failed[@]}"}"; do
  printf 'FAIL    %s\n' "$name"
done
for name in "${blocked[@]+"${blocked[@]}"}"; do
  printf 'BLOCKED %s\n' "$name"
done

printf '\nThis is source-level and disposable-Postgres evidence only. It proves the loop types, the\n'
printf 'posture gate, the cross-service contracts, the executor and gateway units and the session-core\n'
printf 'stores. It does not prove a rebuilt image, a deployed switch, a live pause, resume or park, or\n'
printf 'any provider effect: those are durable-loop-live-proof.sh, run by hand against a running stack.\n'

if [[ ${#failed[@]} -gt 0 ]]; then
  exit 1
fi
if [[ ${#blocked[@]} -gt 0 ]]; then
  exit 2
fi
exit 0
