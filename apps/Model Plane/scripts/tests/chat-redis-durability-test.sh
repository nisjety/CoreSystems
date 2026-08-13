#!/usr/bin/env bash
set -euo pipefail

# Release-shaped proof for the gateway's cross-replica chat resume contract.
# The Rust test is intentionally ignored in normal unit runs; this harness
# supplies Redis, writes a completed stream, restarts the same Redis process,
# and proves another gateway instance can resume it without crossing tenant
# scope.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTAINER="model-plane-chat-redis-${RANDOM}-$$"
PORT="${MODEL_PLANE_CHAT_REDIS_PORT:-6397}"
REQUEST_ID="redis-e2e-$(date +%s)-$$"
TARGET_DIR="${CARGO_TARGET_DIR:-/Volumes/Applikasjon/Triodelab/CoreSystem-build-cache/cargo/model-plane}"

cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

docker run -d --name "$CONTAINER" -p "127.0.0.1:${PORT}:6379" redis:7-alpine \
  redis-server --appendonly yes --save 60 1 >/dev/null

for _ in $(seq 1 30); do
  if docker exec "$CONTAINER" redis-cli ping 2>/dev/null | grep -q '^PONG$'; then
    break
  fi
  sleep 1
done
docker exec "$CONTAINER" redis-cli ping | grep -q '^PONG$'

run_phase() {
  local phase="$1"
  (cd "$ROOT_DIR" && \
    REDIS_URL="redis://127.0.0.1:${PORT}" \
    REDIS_DURABILITY_PHASE="$phase" \
    REDIS_DURABILITY_REQUEST_ID="$REQUEST_ID" \
    CARGO_TARGET_DIR="$TARGET_DIR" \
    cargo test --manifest-path rust/Cargo.toml -p model-gateway --lib \
      stream_buffer::tests::redis_resume_survives_store_restart_and_preserves_identity_scope -- \
      --ignored --exact)
}

run_phase write
docker restart "$CONTAINER" >/dev/null
for _ in $(seq 1 30); do
  if docker exec "$CONTAINER" redis-cli ping 2>/dev/null | grep -q '^PONG$'; then
    break
  fi
  sleep 1
done
docker exec "$CONTAINER" redis-cli ping | grep -q '^PONG$'
run_phase read

echo "chat Redis durability: ok (request ${REQUEST_ID})"
