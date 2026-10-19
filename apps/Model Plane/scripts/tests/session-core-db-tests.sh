#!/usr/bin/env bash
set -euo pipefail

# Run the Session Core Postgres-backed tests against a throwaway Postgres
# container that exists only for this run. It never touches the stack's own
# containers, never reads a stack credential, and never prints the per-run
# password or the DATABASE_URL. The container is bound to loopback on a random
# host port; the EXIT trap removes it with its anonymous volume on success, on
# failure and on Ctrl+C.
#
# Usage: session-core-db-tests.sh [NAME[=MIN_PASSED] ...]
#   session-core-db-tests.sh                       every session-core test, including the ignored DB tests
#   session-core-db-tests.sh loop_checkpoint=4     the filter must pass at least four tests
#   session-core-db-tests.sh loop_checkpoint=4 tool_ledger=3
#   session-core-db-tests.sh --check-log LOG_FILE MIN_PASSED   classify a saved cargo log; starts no container
# A filter without =MIN_PASSED must still pass at least one test: a filter that
# matches nothing reports "0 passed" and would otherwise exit 0.
#
# Exit codes: 0 the run proves the tests; 2 BLOCKED before or during setup, or a bad
# argument; 4 cargo succeeded but the run does not prove the tests (a test skipped
# itself, a panic reached the output, or fewer tests passed than required); any
# other value is cargo's own exit status.
#
# Most DB-backed tests skip silently when DATABASE_URL is missing, and
# REQUIRE_CONTRACT only makes a few of them fail. The log checks are the guard.
#
# Every docker call runs under timeout, so a wedged engine ends the run as BLOCKED
# instead of hanging. Cargo is not bounded, because a full run takes a long time.
# The run clears the variables that change what these tests do: DATABASE_URL, the
# SESSION_CORE_*_ENABLED switches, EXECUTION_*, AGENT_CONTINUE_*, NATS_*, REQUIRE_CONTRACT,
# CI, MODEL_PLANE_RESIDENCY and UPDATE_AUDIT_CORE_FIXTURES. Other environment reads are not
# cleared. The block is marked, and the contract test runs it under an empty environment.
# An unfiltered run prints its pass count in a banner. The last full run recorded for
# this harness, through a disposable Postgres, passed 658 tests with 0 failed (2026-10-19;
# it was 648 before the S8-0 recovery-expiry tests, 642 before the descriptor-reaper tests,
# and 588 before the Build 1b, 2 and 2b rounds).
#
# Needs a responding Docker engine and the image below present locally.
# If the engine is wedged, see apps/Model Plane/docs/runbooks/docker-desktop-recovery.md.

# Git Bash must not rewrite POSIX-looking arguments for native programs.
export MSYS_NO_PATHCONV=1

# BEGIN env scrub
# This run uses only its own database, never one inherited from the shell, and the
# switches below cannot be set from the shell. The DB tests read the environment
# directly, so every name the scrub matches is removed before anything runs.
# DATABASE_URL is exported again below, from the disposable container only.
unset DATABASE_URL
while IFS= read -r name; do
  unset "$name"
done < <(compgen -e | grep -E '^(SESSION_CORE_.*_ENABLED|EXECUTION_.*|AGENT_CONTINUE_.*|NATS_.*|REQUIRE_CONTRACT|CI|MODEL_PLANE_RESIDENCY|UPDATE_AUDIT_CORE_FIXTURES)$' || true)
# END env scrub

# Must match the image the stack runs (apps/Model Plane/deploy/docker-compose.yml).
IMAGE="postgres:16.14-alpine3.24"
PG_USER="session_core"
PG_DB="session_core_test"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUST_DIR="$ROOT_DIR/rust"
CONTAINER=""
PG_PASSWORD=""
LOG=""
CLEANUP_NEEDED=0

cleanup() {
  if [[ "$CLEANUP_NEEDED" == 1 ]]; then
    timeout -k 5 60 docker rm -f -v "$CONTAINER" >/dev/null 2>&1 || true
  fi
  if [[ -n "$LOG" ]]; then
    rm -f "$LOG"
  fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Sums "test result: ok. N passed" over every test binary in a cargo log.
count_passed() {
  local log="$1" total=0 n
  while IFS= read -r n; do
    total=$((total + 10#$n))
  done < <(sed -n 's/^test result: ok\. \([0-9][0-9]*\) passed.*/\1/p' "$log")
  printf '%s\n' "$total"
}

# Returns 0 only when the log proves the selection ran: no test skipped itself,
# no panic reached the output, and at least MIN_PASSED tests passed.
check_log() {
  local log="$1" min="$2" passed
  if grep -q -F 'skipping: DATABASE_URL unset' "$log"; then
    echo 'session-core DB tests: a test skipped itself because DATABASE_URL was not seen; the run proves nothing' >&2
    return 4
  fi
  if grep -q -F 'panicked' "$log"; then
    echo 'session-core DB tests: a panic reached the output; the run does not prove the tests' >&2
    return 4
  fi
  passed="$(count_passed "$log")"
  if (( passed < min )); then
    echo "session-core DB tests: $passed passed, fewer than the required $min" >&2
    return 4
  fi
  return 0
}

# A log check that starts no container. The contract test uses it on fixture logs.
if [[ "${1:-}" == "--check-log" ]]; then
  if [[ $# -ne 3 || ! -f "$2" || ! "$3" =~ ^[1-9][0-9]*$ ]]; then
    echo 'usage: session-core-db-tests.sh --check-log LOG_FILE MIN_PASSED' >&2
    exit 2
  fi
  check_log "$2" "$3" || exit $?
  exit 0
fi

# Each argument is a filter, optionally with its own minimum pass count. Parse them
# before anything touches Docker.
filters=()
minimums=()
for spec in "$@"; do
  if [[ "$spec" =~ ^([A-Za-z0-9_:]+)(=([1-9][0-9]*))?$ ]]; then
    filters+=("${BASH_REMATCH[1]}")
    minimums+=("${BASH_REMATCH[3]:-1}")
  else
    echo "BLOCKED invalid filter '$spec': use NAME or NAME=MIN_PASSED, with letters, digits, _ and :" >&2
    exit 2
  fi
done
if [[ ${#filters[@]} -eq 0 ]]; then
  filters=("")
  minimums=("1")
fi

for tool in docker cargo od tr sed seq sleep grep tee mktemp timeout; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "BLOCKED $tool is unavailable" >&2
    exit 2
  }
done
# Windows has its own timeout.exe, which does not take these arguments; only GNU timeout does.
timeout --version >/dev/null 2>&1 || {
  echo "BLOCKED the timeout on PATH is not GNU timeout; run the harness from Git Bash or Cygwin" >&2
  exit 2
}
[[ -f "$RUST_DIR/Cargo.toml" ]] || {
  echo "BLOCKED $RUST_DIR/Cargo.toml not found" >&2
  exit 2
}
# Every docker call runs under timeout: the documented failure mode is a docker command that hangs.
timeout -k 5 30 docker version --format '{{.Server.Version}}' >/dev/null 2>&1 || {
  echo "BLOCKED the Docker engine does not answer; see apps/Model Plane/docs/runbooks/docker-desktop-recovery.md" >&2
  exit 2
}
timeout -k 5 30 docker image inspect "$IMAGE" >/dev/null 2>&1 || {
  echo "BLOCKED $IMAGE is not present locally; pull it deliberately first" >&2
  exit 2
}

LOG="$(mktemp)"
PG_PASSWORD="$(od -An -N24 -tx1 /dev/urandom | tr -d ' \n')"
CONTAINER="session-core-dbtest-$$-${RANDOM}${RANDOM}"

# Set before docker run, so a container that was created but failed to start is removed too.
CLEANUP_NEEDED=1
timeout -k 5 60 docker run -d \
  --name "$CONTAINER" \
  -e POSTGRES_USER="$PG_USER" \
  -e POSTGRES_PASSWORD="$PG_PASSWORD" \
  -e POSTGRES_DB="$PG_DB" \
  -p 127.0.0.1::5432 \
  "$IMAGE" >/dev/null || {
  echo "BLOCKED the disposable Postgres did not start; see apps/Model Plane/docs/runbooks/docker-desktop-recovery.md" >&2
  exit 2
}

PORT_LINES="$(timeout -k 5 30 docker port "$CONTAINER" 5432/tcp)" || {
  echo "BLOCKED Docker published no host port for the disposable Postgres" >&2
  exit 2
}
PORT="$(printf '%s\n' "$PORT_LINES" | sed -n '1s/.*://p')"
[[ -n "$PORT" ]] || {
  echo "BLOCKED Docker published no host port for the disposable Postgres" >&2
  exit 2
}

# Probe over TCP inside the container: during first-time initialisation the image
# runs a socket-only server, which must not count as ready.
ready=0
for _ in $(seq 1 60); do
  if timeout -k 5 10 docker exec "$CONTAINER" pg_isready -h 127.0.0.1 -U "$PG_USER" -d "$PG_DB" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 1
done
if [[ "$ready" != 1 ]]; then
  echo "BLOCKED disposable Postgres did not become ready; last log lines follow" >&2
  timeout -k 5 30 docker logs --tail 20 "$CONTAINER" >&2 2>&1 || true
  exit 2
fi

# The password comes from the container's own environment, not from this host command line.
timeout -k 5 30 docker exec "$CONTAINER" sh -c 'PGPASSWORD="$POSTGRES_PASSWORD" psql -h 127.0.0.1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -tAc "select 1"' >/dev/null 2>&1 || {
  echo "BLOCKED disposable Postgres rejected the probe login" >&2
  exit 2
}

export DATABASE_URL="postgres://${PG_USER}:${PG_PASSWORD}@127.0.0.1:${PORT}/${PG_DB}?sslmode=disable"
# REQUIRE_CONTRACT makes only a few tests fail instead of skip when DATABASE_URL is missing.
export REQUIRE_CONTRACT=1

printf 'session-core DB tests: %s on 127.0.0.1:%s, database %s, container %s\n' \
  "$IMAGE" "$PORT" "$PG_DB" "$CONTAINER"

# Runs one cargo selection into LOG, then checks it. The password is redacted from
# the output before it reaches the terminal or the log.
run_selection() {
  local filter="$1" min="$2" status=0
  local -a selector=()
  if [[ -n "$filter" ]]; then
    selector=("$filter")
  fi
  (
    cd "$RUST_DIR"
    # --show-output: libtest hides a passing test's output, which is where a skip message would be.
    # These two tests dial a provisioned NATS broker through NATS_URL, which this run does not set.
    cargo test -p session-core --bins -- "${selector[@]}" --include-ignored --show-output --test-threads=1 \
      --skip outbox_drain_publishes_every_row_kind_through_a_provisioned_broker \
      --skip requeued_terminal_rows_are_delivered_once_the_cause_is_fixed \
      2>&1 | sed -u "s/${PG_PASSWORD}/<redacted>/g" | tee "$LOG"
  ) || status=$?
  if [[ "$status" -ne 0 ]]; then
    return "$status"
  fi
  check_log "$LOG" "$min" || return $?
  if [[ -z "$filter" ]]; then
    # The unfiltered run is the whole suite, so its count goes where a person will read it.
    printf '\n============================================================\n'
    printf 'session-core DB tests, FULL RUN: %s passed, 0 failed\n' "$(count_passed "$LOG")"
    printf 'last full run recorded for this harness: 658 passed, 0 failed; a different count means a test was added, removed or skipped\n'
    printf '============================================================\n\n'
  fi
  return 0
}

status=0
for index in "${!filters[@]}"; do
  run_selection "${filters[$index]}" "${minimums[$index]}" || {
    status=$?
    break
  }
done

if [[ "$status" -eq 0 ]]; then
  printf 'session-core DB tests: ok (the exit trap removes the container)\n'
else
  printf 'session-core DB tests: failed with exit %s (the exit trap removes the container)\n' "$status" >&2
fi
exit "$status"
