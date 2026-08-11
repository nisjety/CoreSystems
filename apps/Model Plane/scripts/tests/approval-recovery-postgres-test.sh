#!/usr/bin/env bash
set -euo pipefail

# Real Postgres proof for the governed approval crash/recovery path: a worker
# lease expires, another worker reclaims the delivery, and the start receipt
# remains idempotent (no duplicate effectful start).

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTAINER="model-plane-approval-pg-${RANDOM}-$$"
TARGET_DIR="${CARGO_TARGET_DIR:-/tmp/model-plane-crypto-target}"

cleanup() { docker rm -f "$CONTAINER" >/dev/null 2>&1 || true; }
trap cleanup EXIT

docker run -d --name "$CONTAINER" -P \
  -e POSTGRES_PASSWORD=postgres -e POSTGRES_DB=session_core \
  postgres:16-alpine >/dev/null
PORT="$(docker port "$CONTAINER" 5432/tcp | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
test -n "$PORT"
for _ in $(seq 1 60); do
  if docker exec "$CONTAINER" pg_isready -U postgres -d session_core >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
docker exec "$CONTAINER" pg_isready -U postgres -d session_core >/dev/null

(cd "$ROOT_DIR" && \
  DATABASE_URL="postgres://postgres:postgres@127.0.0.1:${PORT}/session_core" \
  CARGO_TARGET_DIR="$TARGET_DIR" \
  cargo test --manifest-path rust/Cargo.toml -p session-core --bin session-core \
    approval_delivery::tests::expired_worker_lease_reclaims_without_duplicate_start_receipt \
    -- --ignored --exact)

echo "approval crash/recovery: ok"
