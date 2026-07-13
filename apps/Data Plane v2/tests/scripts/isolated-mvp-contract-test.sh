#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
harness="$root/tests/e2e/run-isolated-mvp.sh"
override="$root/tests/e2e/isolated/docker-compose.yml"
mock="$root/tests/e2e/isolated/authority-mock.mjs"

for file in "$harness" "$override" "$mock"; do
  [ -f "$file" ] || { echo "FAIL: missing isolated fixture $file" >&2; exit 1; }
done

rg -Fq 'dpv2-mvp-e2e-' "$harness"
rg -Fq '[ "$bus_network" != "inter-plane-bus" ]' "$harness"
rg -Fq 'down --volumes --remove-orphans' "$harness"
rg -Fq 'DPV2_COMPOSE_PROJECT="$project"' "$harness"
rg -Fq 'snapshot_content' "$harness"
rg -Fq 'ISOLATED_E2E: "1"' "$override"
rg -Fq 'EMBEDDING_PROVIDER: deterministic_test' "$override"

if rg -q -- 'cleanup/orphans|bulk delete|index reset|purge|"global":true|"clear":true|"break_glass":true' "$harness"; then
  echo "FAIL: isolated harness contains destructive operations" >&2
  exit 1
fi

node --check "$mock"
node --check "$root/tests/e2e/isolated/fixture.mjs"
bash -n "$harness"
echo "PASS: isolated MVP harness contract"
