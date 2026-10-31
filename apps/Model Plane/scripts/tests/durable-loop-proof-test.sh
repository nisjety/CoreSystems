#!/usr/bin/env bash
set -euo pipefail

# Contract of the two durable-loop proof runners. It reads their source and runs
# their argument handling; it starts no container and runs no cargo test.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROOF="$ROOT_DIR/scripts/tests/durable-loop-proof.sh"
LIVE="$ROOT_DIR/scripts/tests/durable-loop-live-proof.sh"

for script in "$PROOF" "$LIVE"; do
  test -f "$script"
  bash -n "$script"
done

# The source-level proof delegates the database to the one harness that owns the
# disposable container, and names every stage the plan's S9 row lists.
proof_source="$(cat "$PROOF")"
grep -F 'session-core-db-tests.sh' <<<"$proof_source" >/dev/null
grep -F 'loop_fold_zdr_matrix' <<<"$proof_source" >/dev/null
grep -F 'durable_loop_contract' <<<"$proof_source" >/dev/null
grep -F 'cross_service_loop_contract' <<<"$proof_source" >/dev/null
grep -F 'does not prove a rebuilt image' <<<"$proof_source" >/dev/null

# It must not grow a path to the running databases, a container of its own, or a
# credential.
if grep -n -E 'model-plane-postgres|verevon-proof-pg|docker (run|exec|kill|pause|stop)|EXECUTION_CORE_SERVICE_API_KEY|CONTROL_.*TOKEN' "$PROOF" >/dev/null; then
  echo 'durable loop proof must not start a container or target a running database' >&2
  exit 1
fi

# Bad arguments end BLOCKED before anything runs.
status=0
bash "$PROOF" nonsense >/dev/null 2>&1 || status=$?
test "$status" -eq 2

# The live proof needs the operator's session and says so before it touches Docker.
status=0
env -u MP_COOKIE bash "$LIVE" >/dev/null 2>&1 || status=$?
test "$status" -eq 2

live_source="$(cat "$LIVE")"
grep -F 'NOT CI' <<<"$(tr 'a-z' 'A-Z' <<<"$live_source")" >/dev/null
grep -F 'Skipped stages are NOT proven' <<<"$live_source" >/dev/null

# It reads the cookie from the environment only, never prints it, and does not
# change the stack: no kill, pause, stop, restart, compose, trigger or switch flip.
if grep -n -E 'echo[^#]*session_cookie|printf[^#]*session_cookie[^>]*$|docker (kill|pause|unpause|stop|restart|rm|compose|run)|compose\.sh|CREATE TRIGGER|DROP TRIGGER|ALTER TABLE|UPDATE |DELETE FROM|INSERT INTO' "$LIVE" | grep -v -E '^[0-9]+:[[:space:]]*#' >/dev/null; then
  echo 'durable loop live proof must stay read-only toward the stack and must not print the cookie' >&2
  exit 1
fi

# session-core broadcasts the resume event in memory and never writes it to the events
# table, so counting it there fails on a correct stack.
if grep -n -E 'events.*RUN_RESUMED_AFTER_APPROVAL|RUN_RESUMED_AFTER_APPROVAL.*events' "$LIVE" | grep -v -E '^[0-9]+:[[:space:]]*#' >/dev/null; then
  echo 'durable loop live proof must not count RUN_RESUMED_AFTER_APPROVAL in the events table' >&2
  exit 1
fi

# The evidence it writes carries no body: the response captures are never printed
# or copied into a line.
if grep -n -E 'cat "\$WORK/[a-z0-9]+\.body"|cat "\$[a-z]+\.body"' "$LIVE" >/dev/null; then
  echo 'durable loop live proof must not print a response body' >&2
  exit 1
fi

printf 'durable-loop proof runners contract: ok\n'
