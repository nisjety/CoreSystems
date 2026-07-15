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
if rg -Fq 'ISOLATED_E2E_SUFFIX' "$harness"; then
  echo "FAIL: isolated acceptance harness permits caller-selected project identity" >&2
  exit 1
fi
if rg -Fq 'ISOLATED_E2E_SKIP_BUILD' "$harness"; then
  echo "FAIL: isolated acceptance harness permits source-build bypass" >&2
  exit 1
fi
rg -Fq 'refusing collision with existing isolated project resources' "$harness"
rg -Fq 'trap cleanup_runtime EXIT' "$harness"
if rg -Fq 'trap cleanup_runtime EXIT INT TERM' "$harness"; then
  echo "FAIL: early cleanup signal trap can resume after deleting keys" >&2
  exit 1
fi
rg -Fq '[ "$bus_network" != "inter-plane-bus" ]' "$harness"
rg -Fq 'down --volumes --remove-orphans' "$harness"
rg -Fq 'DPV2_COMPOSE_PROJECT="$project"' "$harness"
rg -Fq 'export DATAPLANE_NATS_TOKEN=' "$harness"
rg -Fq 'export DATAPLANE_DRAGONFLY_PASSWORD=' "$harness"
if rg -Fq 'allocate_loopback_port' "$harness"; then
  echo "FAIL: isolated harness races for unnecessary host-published ports" >&2
  exit 1
fi
[ "$(rg -Fc 'ports: !reset []' "$override")" -ge 2 ] || {
  echo "FAIL: isolated Postgres/Qdrant host ports are not reset" >&2
  exit 1
}
rg -Fq 'logs --no-color --tail 120 migrate' "$harness"
rg -Fq 'build_services_sequentially' "$harness"
rg -Fq 'for service in "$@"' "$harness"
if rg -Fq 'up -d --build' "$harness"; then
  echo "FAIL: isolated harness starts a resource-unbounded parallel build" >&2
  exit 1
fi
rg -Fq 'wiki.read,wiki.write' "$harness"
rg -Fq 'snapshot_content' "$harness"
rg -Fq 'positive write authorization control failed' "$harness"
rg -Fq 'JWT_ZDR=false' "$harness"
rg -Fq 'documents-events.pem' "$harness"
rg -Fq 'ISOLATED_PRIVATE_KEY_FILE="$runtime_dir/private.pem"' "$harness"
rg -Fq '${ISOLATED_PRIVATE_KEY_FILE:?required}:/fixtures/private.pem:ro' "$override"
! rg -Fq '${DOCUMENTS_EVENT_SIGNING_PRIVATE_KEY_PATH:?required}:/fixtures/private.pem:ro' "$override"
rg -Fq 'index-events.pem' "$harness"
rg -Fq 'embedding-events.pem' "$harness"
rg -Fq 'wiki-events.pem' "$harness"
rg -Fq 'retrieval-events.pem' "$harness"
rg -Fq 'wait_for_stable_snapshot' "$harness"
rg -Fq 'verified token zdr=true forbids durable document persistence' "$harness"
rg -Fq 'verified token zdr=true forbids durable source-object persistence' "$harness"
rg -Fq 'ingest_policy.zdr_mode=on or ephemeral_only=true forbids durable document persistence' "$harness"
rg -Fq 'isolated ZDR content snapshot changed after quiescence' "$harness"
rg -Fq 'ISOLATED_E2E: "1"' "$override"
rg -Fq 'EMBEDDING_PROVIDER: deterministic_test' "$override"
rg -Fq 'membershipRevision' "$mock"
rg -Fq "reason: 'member'" "$mock"
rg -Fq "const policyAudience = 'control-policy'" "$mock"
rg -Fq 'verifyPolicyBearer' "$mock"

if rg -q -- 'cleanup/orphans|bulk delete|index reset|purge|"global":true|"clear":true|"break_glass":true' "$harness"; then
  echo "FAIL: isolated harness contains destructive operations" >&2
  exit 1
fi

node --check "$mock"
node --check "$root/tests/e2e/isolated/fixture.mjs"
fixture_dir=$(mktemp -d)
trap 'rm -rf "$fixture_dir"' EXIT
node "$root/tests/e2e/isolated/fixture.mjs" prepare "$fixture_dir"
for producer in documents index embedding wiki retrieval; do
  [ -s "$fixture_dir/$producer-events.pem" ]
  [ -s "$fixture_dir/$producer-events.pub" ]
done
for first in documents index embedding wiki retrieval; do
  for second in documents index embedding wiki retrieval; do
    [ "$first" = "$second" ] && continue
    if cmp -s "$fixture_dir/$first-events.pem" "$fixture_dir/$second-events.pem"; then
      echo "FAIL: isolated event producers share a signing key" >&2
      exit 1
    fi
  done
done
bash -n "$harness"
echo "PASS: isolated MVP harness contract"
