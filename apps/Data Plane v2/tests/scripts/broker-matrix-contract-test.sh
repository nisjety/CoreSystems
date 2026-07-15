#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
runner="$root/tests/scripts/run-isolated-broker-matrix.sh"
matrix="$root/tests/e2e/broker-delivery-matrix.sh"
compose="$root/tests/e2e/isolated/broker-compose.yml"

for file in "$runner" "$matrix" "$compose"; do
  [ -f "$file" ] || { echo "FAIL: missing $file" >&2; exit 1; }
done
bash -n "$runner"
bash -n "$matrix"

for stream in DATAPLANE_DOCUMENTS DATAPLANE_SOURCE_OBJECTS DATAPLANE_COST; do
  rg -Fq "$stream" "$matrix"
done
for subject in dataplane.documents.created dataplane.source_objects.changed dataplane.cost.ledger; do
  rg -Fq "$subject" "$matrix"
done
for shape in valid_signed raw forged wrong_tenant wrong_scope replay zdr; do
  rg -Fq "$shape" "$runner"
done

rg -Fq -- '--retention=work' "$matrix"
rg -Fq -- '--max-age=5m' "$matrix"
rg -Fq -- '--max-msgs=64' "$matrix"
rg -Fq -- '--max-bytes=1048576' "$matrix"
rg -Fq -- '--max-deliver=3' "$matrix"
rg -Fq -- '--nak' "$matrix"
rg -Fq -- '--ack' "$matrix"
rg -Fq 'dpv2-broker-e2e-' "$runner"
rg -Fq 'down --volumes --remove-orphans' "$runner"
! rg -qi 'stream[[:space:]]+(purge|delete)|consumer[[:space:]]+delete' "$runner" "$matrix"

echo "PASS: isolated broker matrix contract"
