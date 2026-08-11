#!/usr/bin/env bash
set -euo pipefail

# Real Postgres proof that canonical thread replay includes thread-owned
# MESSAGE_APPENDED events emitted before a run exists.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTAINER="model-plane-thread-replay-pg-${RANDOM}-$$"
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
    grpc::tests::replay_thread_includes_thread_owned_events_against_real_pg \
    -- --ignored --exact)

echo "canonical thread replay: ok"
