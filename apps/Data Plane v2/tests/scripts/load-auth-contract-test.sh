#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
GRPC="$ROOT/tests/load/grpc-retrieve.sh"
HTTP="$ROOT/tests/load/http-retrieve.js"

fail() {
  printf 'FAIL: %s\n' "$1" >&2
  exit 1
}

for file in "$GRPC" "$HTTP"; do
  if grep -Eq 'INTERNAL_API_KEY|(^|[^A-Z_])API_KEY([^A-Z_]|$)|x-api-key|org-loadtest|localhost:(50052|8014)' "$file"; then
    fail "$(basename "$file") contains shared-key, fixed-tenant, or implicit published-port assumptions"
  fi
  grep -Fq 'DPV2_USER_BEARER' "$file" || fail "$(basename "$file") does not require a user bearer"
  grep -Fq 'DPV2_TEST_ORG_ID' "$file" || fail "$(basename "$file") does not require a disposable organization"
  grep -Fq 'ephemeral' "$file" || fail "$(basename "$file") does not force ephemeral ZDR posture"
done

if DPV2_USER_BEARER=header.payload.signature DPV2_TEST_ORG_ID=matrix-own-org \
  "$GRPC" >"${TMPDIR:-/tmp}/dpv2-load-contract.out" 2>&1; then
  fail "gRPC load test accepted a missing explicit endpoint"
fi

printf 'PASS: load test authentication contract\n'
