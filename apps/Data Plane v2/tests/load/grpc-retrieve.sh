#!/usr/bin/env bash
set -euo pipefail
set +x

# Isolated-only gRPC retrieval load test. The short-lived bearer and disposable
# organization are mandatory. A mode-0600 ghz config prevents the bearer from
# appearing in process arguments; it is removed on every exit path.

require_env() {
  local name=$1
  if [ -z "${!name:-}" ]; then
    printf 'configuration error: %s is required\n' "$name" >&2
    exit 2
  fi
}

require_env GRPC_ADDR
require_env DPV2_USER_BEARER
require_env DPV2_TEST_ORG_ID

[[ "$GRPC_ADDR" =~ ^(localhost|127\.0\.0\.1|\[::1\]):[0-9]{2,5}$ ]] || {
  printf 'configuration error: GRPC_ADDR must be an explicit loopback host:port\n' >&2
  exit 2
}
[[ "$DPV2_USER_BEARER" =~ ^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$ ]] || {
  printf 'configuration error: DPV2_USER_BEARER must be a compact JWT\n' >&2
  exit 2
}
[[ "$DPV2_TEST_ORG_ID" =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] || {
  printf 'configuration error: DPV2_TEST_ORG_ID has an invalid format\n' >&2
  exit 2
}

TOTAL=${1:-200}
CONCURRENCY=${2:-50}
[[ "$TOTAL" =~ ^[1-9][0-9]{0,5}$ ]] || {
  printf 'configuration error: total requests must be between 1 and 999999\n' >&2
  exit 2
}
[[ "$CONCURRENCY" =~ ^[1-9][0-9]{0,3}$ ]] || {
  printf 'configuration error: concurrency must be between 1 and 9999\n' >&2
  exit 2
}

command -v ghz >/dev/null 2>&1 || {
  printf 'configuration error: ghz is required\n' >&2
  exit 2
}

PROTO_ROOT=$(cd "$(dirname "$0")/../.." && pwd)/proto
umask 077
CONFIG=$(mktemp "${TMPDIR:-/tmp}/dpv2-ghz.XXXXXX.json")
trap 'rm -f "$CONFIG"' EXIT

printf '{
  "proto": "%s/retrieval_v2.proto",
  "import-paths": ["%s"],
  "call": "dataplane.retrieval.v2.RetrievalService/Retrieve",
  "host": "%s",
  "insecure": true,
  "total": %s,
  "concurrency": %s,
  "timeout": 30000000000,
  "metadata": {"authorization": "Bearer %s"},
  "data": {
    "org_id": "%s",
    "query": "authorization-safe retrieval load probe",
    "top_k": 10,
    "zdr_mode": "ephemeral",
    "filters": {}
  }
}\n' \
  "$PROTO_ROOT" \
  "$PROTO_ROOT" \
  "$GRPC_ADDR" \
  "$TOTAL" \
  "$CONCURRENCY" \
  "$DPV2_USER_BEARER" \
  "$DPV2_TEST_ORG_ID" >"$CONFIG"

printf 'gRPC retrieval load test: requests=%s concurrency=%s, response bodies suppressed\n' "$TOTAL" "$CONCURRENCY"
ghz --config "$CONFIG"
