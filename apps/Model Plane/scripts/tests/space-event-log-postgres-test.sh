#!/usr/bin/env bash
set -euo pipefail

# Real Postgres proof of the Space event log (WS02-S1): applies the session-core
# migrations to a disposable database and runs the ignored `space_event` tests
# (contiguous seq under concurrent writers, rollback leaves no hole, NOTIFY after
# commit, a swallowed trigger failure is counted, the sweeper moves min_seq, the
# suppress setting prevents rows during a purge, no row for a zero-retention run,
# erased rows replay as redacted).
#
# The database name contains "test": the tests refuse to write to any other name.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTAINER="model-plane-space-event-pg-${RANDOM}-$$"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/rust/target}"
MIN_TESTS="${SPACE_EVENT_MIN_TESTS:-8}"
LOG="$(mktemp)"

cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  rm -f "$LOG"
}
trap cleanup EXIT

docker run -d --name "$CONTAINER" -P \
  -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=session_core_test \
  postgres:16-alpine >/dev/null
PORT="$(docker port "$CONTAINER" 5432/tcp | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
test -n "$PORT"
for _ in $(seq 1 60); do
  if docker exec "$CONTAINER" pg_isready -U postgres -d session_core_test >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
docker exec "$CONTAINER" pg_isready -U postgres -d session_core_test >/dev/null

(cd "$ROOT_DIR" && \
  DATABASE_URL="postgres://postgres:postgres@127.0.0.1:${PORT}/session_core_test" \
  REQUIRE_CONTRACT=1 \
  CARGO_TARGET_DIR="$TARGET_DIR" \
  cargo test --manifest-path rust/Cargo.toml -p session-core --bin session-core \
    space_event -- --ignored --test-threads=1) 2>&1 | tee "$LOG"

passed="$(grep -oE "test result: ok\. [0-9]+ passed" "$LOG" | grep -oE "[0-9]+" | head -1 || true)"
if [ -z "${passed:-}" ] || [ "$passed" -lt "$MIN_TESTS" ]; then
  echo "expected at least ${MIN_TESTS} space event log tests to run, got ${passed:-0}"
  exit 1
fi

echo "space event log: ok (${passed} tests)"
