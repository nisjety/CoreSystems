#!/usr/bin/env bash
set -euo pipefail

# Source/disposable proof for Application notification delivery. This never
# enables a provider, reads/creates/rotates deployment credentials, or mutates
# a shared database. The real-Postgres lease proof uses a short-lived local
# Postgres container and is always exercised when Docker is available.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
APP_DIR="$ROOT_DIR/apps/Application Plane/notification-core"
CONTAINER="coresystem-application-delivery-pg-${RANDOM}-$$"
TEST_PG_PASSWORD="local-proof-only"

cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

cd "$APP_DIR"
go test -count=1 ./internal/config ./internal/notification ./internal/database ./cmd/server \
  -run 'Test(Delivery|FeedProjection|AcceptEnqueues|ApplyMigration|TenantScopeMigration|RetentionMigration)' \
  -cover

# The config tests use the full package name because their cases are not
# prefixed with Delivery; run them separately so the disabled-by-default
# worker guard is part of this proof without broadening the disposable suite.
go test -count=1 ./internal/config -run 'TestLoad(DefaultsDeliveryWorkerDisabled|RejectsDeliveryWorkerWithoutProvider|RejectsDeliveryWorkerWithoutCallbackVerifier|AcceptsDeliveryWorkerWithExplicitProviderAndVerifier)'

command -v docker >/dev/null 2>&1 || {
  echo "BLOCKED docker is unavailable for the disposable delivery proof" >&2
  exit 2
}

docker create --pull=never --name "$CONTAINER" -P \
  -e POSTGRES_PASSWORD="$TEST_PG_PASSWORD" \
  -e POSTGRES_DB=notification_core_proof \
  postgres:16-alpine >/dev/null
docker start "$CONTAINER" >/dev/null

# Docker Desktop can spend more than a minute allocating the disposable
# database while the rest of the aggregate proof is compiling. Keep the
# readiness wait bounded, but avoid turning that infrastructure race into a
# false product-proof failure.
port=""
for _ in $(seq 1 120); do
  port="$(docker port "$CONTAINER" 5432/tcp 2>/dev/null | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
  if [[ -n "$port" ]] && docker exec "$CONTAINER" pg_isready -U postgres -d notification_core_proof >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if [[ -z "$port" ]] || ! docker exec "$CONTAINER" pg_isready -U postgres -d notification_core_proof >/dev/null 2>&1; then
  echo "BLOCKED disposable Postgres did not become ready" >&2
  exit 2
fi

(cd "$APP_DIR" && \
  NOTIFICATION_CORE_TEST_DATABASE_URL="postgres://postgres:${TEST_PG_PASSWORD}@127.0.0.1:${port}/notification_core_proof?sslmode=disable" \
  NOTIFICATION_CORE_ALLOW_DB_MUTATION=1 \
  go test -count=1 ./internal/notification -run 'TestDeliveryAttemptsLeaseAndReceiptAgainstRealPostgres' -v)

printf 'application delivery source proof: ok\n'
printf 'Disposable Postgres lease/receipt and feed projection proof passed.\n'
printf 'Provider callbacks, live HA replay, and candidate evidence remain release gates.\n'
