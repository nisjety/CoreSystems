#!/usr/bin/env bash
set -euo pipefail

# Run the real Session Core approval-delivery lease/receipt proof against a
# disposable Postgres. This is integration evidence, not deployed approval or
# provider evidence. It never reads, creates, rotates, or prints a service
# credential and it never touches the running CoreSystem databases.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CONTAINER="coresystem-approval-proof-$$"
POSTGRES_PASSWORD="local-proof-only"

cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

command -v docker >/dev/null 2>&1 || {
  echo "BLOCKED docker is unavailable" >&2
  exit 2
}

docker run \
  --name "$CONTAINER" \
  -e POSTGRES_PASSWORD="$POSTGRES_PASSWORD" \
  -e POSTGRES_DB=session_core_proof \
  -P \
  -d postgres:16-alpine >/dev/null

port=""
for attempt in $(seq 1 30); do
  port="$(docker port "$CONTAINER" 5432/tcp 2>/dev/null | sed -n '1p' | sed 's/.*://')"
  if [[ -n "$port" ]] && docker exec "$CONTAINER" pg_isready -U postgres -d session_core_proof >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if [[ -z "$port" ]] || ! docker exec "$CONTAINER" pg_isready -U postgres -d session_core_proof >/dev/null 2>&1; then
  echo "BLOCKED disposable Postgres did not become ready" >&2
  exit 2
fi

DATABASE_URL="postgres://postgres:${POSTGRES_PASSWORD}@127.0.0.1:${port}/session_core_proof?sslmode=disable" \
  cargo test \
    --manifest-path "$ROOT_DIR/rust/Cargo.toml" \
    -p session-core \
    expired_worker_lease_reclaims_without_duplicate_start_receipt \
    -- --ignored --nocapture

printf 'approval-continuation disposable Postgres lease proof: ok\n'
printf 'This proves lease recovery and immutable start-receipt idempotency only; deployed signer, worker, provider, ZDR, candidate, and rollback evidence remain open.\n'
