#!/usr/bin/env bash
set -euo pipefail

# Real-Postgres proof for the scheduled-step claim/receipt contract. This is
# intentionally disposable: it exercises Session Core's migrations and SQL
# locks without changing the running dev stack or any service credentials.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTAINER="model-plane-scheduled-step-pg-${RANDOM}-$$"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/rust/target}"

cleanup() { docker rm -f "$CONTAINER" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker create --pull=never --name "$CONTAINER" -P \
  -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=session_core \
  postgres:16-alpine >/dev/null
docker start "$CONTAINER" >/dev/null
PORT="$(docker port "$CONTAINER" 5432/tcp | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
test -n "$PORT"
for _ in $(seq 1 60); do
  if docker exec "$CONTAINER" pg_isready -U postgres -d session_core >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
docker exec "$CONTAINER" pg_isready -U postgres -d session_core >/dev/null

run_session_test() {
  local test_name="$1"
  local attempt log_file status
  for attempt in $(seq 1 5); do
    log_file="$(mktemp)"
    set +e
    (cd "$ROOT_DIR" && \
      DATABASE_URL="postgres://postgres:postgres@127.0.0.1:${PORT}/session_core?sslmode=disable" \
      CARGO_TARGET_DIR="$TARGET_DIR" \
      cargo test --manifest-path rust/Cargo.toml -p session-core --bin session-core \
        "grpc::tests::${test_name}" -- --ignored --exact) >"$log_file" 2>&1
    status=$?
    set -e
    cat "$log_file"
    if [[ "$status" -eq 0 ]]; then
      rm -f "$log_file"
      return 0
    fi
    # Docker Desktop can publish the port before its host-side forwarder is
    # ready. Retry only that transport-startup failure; assertion/build
    # failures remain hard failures and are never hidden by retries.
    if ! grep -Eiq 'connect pg:|connection reset|unexpected eof|connection refused|failed to receive message' "$log_file"; then
      rm -f "$log_file"
      return "$status"
    fi
    rm -f "$log_file"
    sleep 2
  done
  return 1
}

run_session_test scheduled_step_claim_and_unknown_receipt_are_idempotent_against_real_pg

# A successful preparation can lose its response before Capability Core records
# the handoff. A fresh Control decision for the same fire must reuse the
# service-owned thread rather than create a second one.
run_session_test scheduled_run_prepare_retry_with_fresh_decision_reuses_thread_against_real_pg

echo "scheduled-step receipt Postgres proof: ok"
