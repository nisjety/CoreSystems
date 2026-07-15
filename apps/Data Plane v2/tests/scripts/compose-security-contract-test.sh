#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
data_compose="$root_dir/apps/Data Plane v2/docker-compose.yml"
self_owned_compose="$root_dir/apps/Data Plane v2/docker-compose.self-owned-data.yml"
control_compose="$root_dir/apps/Control Plane/docker-compose.yml"
model_production_compose="$root_dir/apps/Model Plane/deploy/docker-compose.production.yml"
ingestion_production_compose="$root_dir/apps/Ingestion Plane/docker-compose.production.yml"

# Disposable E2E projects must be able to select unique network names instead
# of attaching to the shared runtime by accident.
rg -Fq 'name: ${DATA_PLANE_COMPOSE_PROJECT:-data-plane-v2}' "$data_compose"
rg -Fq 'name: ${DPV2_NETWORK_NAME:-dpv2-net}' "$data_compose"
rg -Fq 'name: ${INTER_PLANE_BUS_NETWORK:-inter-plane-bus}' "$data_compose"

# Data verifiers may receive only the public verification key, never the
# directory that also contains Control's signing key.
if rg -q 'auth-core/keys:/app/keys' "$data_compose"; then
  echo "FAIL: Data container receives the complete auth-core key directory" >&2
  exit 1
fi
test "$(rg -c 'auth-core/keys/convex-auth\.pub:/app/keys/convex-auth\.pub:ro' "$data_compose")" -eq 2

# Async producers receive only their own private signing key; consumers receive
# only direct producers' public verification keys. Signed consumption is enabled
# on retrieval, index, embedding, graph, and the Quickwit projection adapter.
test "$(rg -c 'ENABLE_SIGNED_EVENT_CONSUMERS: "1"' "$data_compose")" -eq 5
test "$(rg -c 'EVENT_SIGNING_PRIVATE_KEY_PATH: /run/event-keys/documents-events\.pem' "$data_compose")" -eq 1
test "$(rg -c 'INDEX_EVENT_PRIVATE_KEY_PATH: /run/event-keys/index-events\.pem' "$data_compose")" -eq 1
test "$(rg -c 'EMBEDDING_EVENT_PRIVATE_KEY_PATH: /run/event-keys/embedding-events\.pem' "$data_compose")" -eq 1
test "$(rg -c 'documents-events\.pub:ro' "$data_compose")" -eq 3
test "$(rg -c 'index-events\.pub:ro' "$data_compose")" -eq 3
rg -q 'INDEX_EVENT_PUBLIC_KEY_PATH: /run/event-keys/index-events\.pub' "$data_compose"
test "$(rg -c 'embedding-events\.pub:ro' "$data_compose")" -eq 3
test "$(rg -c 'wiki-events\.pub:ro' "$data_compose")" -eq 2
test "$(rg -c 'retrieval-events\.pub:ro' "$data_compose")" -eq 1

# user-core is internal; diagnostics and service ports must not be host-published.
if rg -q 'PPROF_ENABLED: "true"|"6060:6060"|"3012:3012"|"50012:50012"' "$control_compose"; then
  echo "FAIL: user-core diagnostics or service ports are exposed by default" >&2
  exit 1
fi

# Data grant readers use distinct credentials with read-only scope. Grant
# mutation remains unavailable until a resource-owner delegation exists.
rg -q '"principal":"retrieval-engine".*"scopes":\["authz:read"\]' "$control_compose"
rg -q '"principal":"documents-api".*"scopes":\["authz:read"\]' "$control_compose"
if rg -q '"principal":"(retrieval-engine|documents-api)"[^]]*"authz:write"' "$control_compose"; then
  echo "FAIL: Data grant readers received authz mutation scope" >&2
  exit 1
fi

# Optional self-owned data dependencies stay private and require credentials.
if rg -q '^    ports:|change-me-' "$self_owned_compose"; then
  echo "FAIL: optional self-owned dependency publishes a host port or default credential" >&2
  exit 1
fi
rg -q 'YENTE_UPDATE_TOKEN: \$\{YENTE_UPDATE_TOKEN:\?' "$self_owned_compose"
rg -q 'NOMINATIM_PASSWORD: \$\{NOMINATIM_PASSWORD:\?' "$self_owned_compose"

# Cross-plane callers use explicit production overrides: every base-published
# port is reset, and credentials/dev bypasses become required or disabled.
test -f "$model_production_compose"
test -f "$ingestion_production_compose"
test "$(rg -c 'ports: !reset \[\]' "$model_production_compose")" -eq 19
# 13 = the number of host-port-publishing Ingestion services in the base compose
# (quarry-edge/control, imports/integration/shipping/webhook/finspo APIs,
# connector-runtime-engine, postgres, temporal, nats, qdrant, searxng). Every one
# is reset here; verified against the base compose + the running fleet.
test "$(rg -c 'ports: !reset \[\]' "$ingestion_production_compose")" -eq 13
rg -q 'POSTGRES_PASSWORD: \$\{MODEL_POSTGRES_PASSWORD:\?' "$model_production_compose"
rg -q 'MINIO_ROOT_PASSWORD: \$\{MODEL_MINIO_ROOT_PASSWORD:\?' "$model_production_compose"
rg -q 'MODEL_GATEWAY_AUTH_DEV_BYPASS: ""' "$model_production_compose"
rg -q 'QUARRY_EDGE__CONTROL_API_KEY: \$\{QUARRY_CONTROL_API_KEY:\?' "$ingestion_production_compose"
rg -q 'QUARRY_EDGE_AUTH_DEV_BYPASS: "0"' "$ingestion_production_compose"
rg -q 'INTEGRATION_CREDENTIALS_ENCRYPTION_KEY: \$\{INTEGRATION_CREDENTIALS_ENCRYPTION_KEY:\?' "$ingestion_production_compose"

echo "PASS: Compose security contract"
