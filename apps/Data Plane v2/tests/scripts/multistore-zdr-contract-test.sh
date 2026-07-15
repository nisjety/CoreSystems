#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
SNAPSHOT="$ROOT/tests/e2e/multistore-zdr-snapshot.sh"
RUNNER="$ROOT/tests/e2e/run-isolated-mvp.sh"
OVERLAY="$ROOT/tests/e2e/isolated/docker-compose.yml"

test -f "$SNAPSHOT" || { echo "missing multi-store ZDR snapshot harness" >&2; exit 1; }
test -x "$SNAPSHOT" || { echo "multi-store ZDR snapshot harness must be executable" >&2; exit 1; }
rg -Fq 'export DOCUMENTS_GDPR_NATS_PASSWORD=' "$RUNNER"
rg -Fq 'GDPR_DURABLE_CONSUMER_REQUIRED: "0"' "$OVERLAY"

for store in postgres qdrant dragonfly quickwit minio nats; do
  rg -Fq "$store" "$SNAPSHOT" || { echo "missing $store snapshot" >&2; exit 1; }
done

rg -Fq 'dpv2-mvp-e2e-' "$SNAPSHOT"
rg -Fq 'redis-cli --no-auth-warning' "$SNAPSHOT"
rg -Fq -- '--scan' "$SNAPSHOT"
rg -Fq 'INFO commandstats' "$SNAPSHOT"
rg -Fq 'cmdstat_' "$SNAPSHOT"
rg -Fq 'points_count' "$SNAPSHOT"
rg -Fq '"max_hits":0' "$SNAPSHOT"
rg -Fq 'mc ls --recursive --json' "$SNAPSHOT"
rg -Fq '/jsz?streams=true' "$SNAPSHOT"
rg -Fq 'account_details' "$SNAPSHOT"
rg -Fq 'last_seq' "$SNAPSHOT"
rg -Fq 'openssl dgst -sha256' "$SNAPSHOT"

if rg -qi 'redis-cli.*\b(get|mget|dump)\b|/points/scroll|"max_hits":[1-9]|mc (cat|cp|get)|nats .* (get|view)' "$SNAPSHOT"; then
  echo "multi-store snapshot may read persisted content" >&2
  exit 1
fi

rg -Fq 'wait_for_stable_multistore_snapshot' "$RUNNER"
rg -Fq 'multistore-zdr-snapshot.sh' "$RUNNER"
rg -Fq 'signed ZDR retrieval guard failed' "$RUNNER"
rg -Fq 'signed ZDR wiki HTTP mutation guard failed' "$RUNNER"
rg -Fq 'isolated multi-store ZDR snapshot changed' "$RUNNER"
rg -Fq 'grpc-auth-matrix.sh' "$RUNNER"

bash -n "$SNAPSHOT"
bash -n "$RUNNER"
echo "PASS: multi-store ZDR harness static contract"
