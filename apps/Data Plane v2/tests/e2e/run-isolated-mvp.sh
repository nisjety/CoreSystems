#!/usr/bin/env bash
set -euo pipefail
set +x

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OVERRIDE="$ROOT/tests/e2e/isolated/docker-compose.yml"
FIXTURE="$ROOT/tests/e2e/isolated/fixture.mjs"

for command in docker node openssl git; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 2; }
done

suffix="$(openssl rand -hex 6)"
project="dpv2-mvp-e2e-$suffix"
internal_network="$project-net"
bus_network="$project-bus"
runtime_dir=$(mktemp -d "$ROOT/tests/e2e/.isolated-runtime.XXXXXX")

case "$project:$internal_network:$bus_network" in
  dpv2-mvp-e2e-*:*:* ) ;;
  * ) echo "refusing non-isolated project names" >&2; exit 2 ;;
esac
[ "$bus_network" != "inter-plane-bus" ] || { echo "refusing shared inter-plane network" >&2; exit 2; }

export DATA_PLANE_COMPOSE_PROJECT="$project"
export DPV2_NETWORK_NAME="$internal_network"
export INTER_PLANE_BUS_NETWORK="$bus_network"
export SOURCE_REVISION="$(git -C "$ROOT" rev-parse HEAD)"
export BUILD_DATE="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
# The local verification host may already run the shared development stack.
# Serialize Compose build/start work so the disposable proof cannot exhaust
# Docker Desktop while compiling several Rust images concurrently.
export COMPOSE_PARALLEL_LIMIT="${COMPOSE_PARALLEL_LIMIT:-1}"
export POSTGRES_PASSWORD="$(openssl rand -hex 24)"
export MINIO_ROOT_USER="isolated$(openssl rand -hex 8)"
export MINIO_ROOT_PASSWORD="$(openssl rand -hex 24)"
export GRAFANA_ADMIN_PASSWORD="$(openssl rand -hex 24)"
export CONTROL_POLICY_SERVICE_API_KEY="$(openssl rand -hex 32)"
export ISOLATED_CONTROL_POLICY_BEARER="$(openssl rand -hex 32)"
export USER_CORE_RETRIEVAL_TOKEN="$(openssl rand -hex 32)"
export USER_CORE_DOCUMENTS_TOKEN="$(openssl rand -hex 32)"
export AUTH_CORE_ISSUER="http://auth-core:3011/api/convex-auth"
export JWT_REQUIRED_ISSUER="$AUTH_CORE_ISSUER"

node "$FIXTURE" prepare "$runtime_dir"
export ISOLATED_JWKS_FILE="$runtime_dir/jwks.json"
export ISOLATED_PUBLIC_KEY_FILE="$runtime_dir/public.pem"
export DOCUMENTS_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/private.pem"
export DOCUMENTS_EVENT_VERIFYING_PUBLIC_KEY_PATH="$runtime_dir/public.pem"
export EMBEDDING_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/private.pem"
export EMBEDDING_EVENT_VERIFYING_PUBLIC_KEY_PATH="$runtime_dir/public.pem"
export INDEX_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/private.pem"
export INDEX_EVENT_VERIFYING_PUBLIC_KEY_PATH="$runtime_dir/public.pem"
export WIKI_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/private.pem"
export WIKI_EVENT_VERIFYING_PUBLIC_KEY_PATH="$runtime_dir/public.pem"

compose=(docker compose --project-name "$project" -f "$ROOT/docker-compose.yml" -f "$OVERRIDE")
created_network=0
cleanup() {
  rc=$?
  if [[ "$project" == dpv2-mvp-e2e-* ]]; then
    "${compose[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true
  fi
  if [ "$created_network" -eq 1 ] && [[ "$bus_network" == dpv2-mvp-e2e-*-bus ]]; then
    docker network rm "$bus_network" >/dev/null 2>&1 || true
  fi
  rm -rf "$runtime_dir"
  exit "$rc"
}
trap cleanup EXIT INT TERM

docker network create "$bus_network" >/dev/null
created_network=1
"${compose[@]}" config >/dev/null
"${compose[@]}" up -d --build --wait --wait-timeout 600

own_org="isolated-$(openssl rand -hex 12)"
spoof_org="isolated-$(openssl rand -hex 12)"
user_id="isolated-user-$(openssl rand -hex 8)"
scopes="data:read,data:quality:admin,data:orchestrate,data:admin,data:search:rebuild,data:search:rebuild:approve,documents:read,documents:write"
bearer=$(JWT_ISSUER="$AUTH_CORE_ISSUER" JWT_ORG_ID="$own_org" JWT_USER_ID="$user_id" JWT_SCOPES="$scopes" node "$FIXTURE" mint "$runtime_dir")

snapshot_content() {
  "${compose[@]}" exec -T postgres psql -U dataplane -d dataplane -Atq -c \
    'SELECT (SELECT count(*) FROM documents),(SELECT count(*) FROM knowledge_units),(SELECT count(*) FROM retrieval_runs),(SELECT count(*) FROM retrieval_candidates),(SELECT count(*) FROM graph_entities),(SELECT count(*) FROM graph_relationships),(SELECT count(*) FROM source_objects),(SELECT count(*) FROM documents_outbox);' \
    | openssl dgst -sha256
}

before=$(snapshot_content)
single_body=$(printf '{"org_id":"%s","source":"isolated://zdr-single","type":"text","title":"isolated-zdr","content":"ephemeral fixture","visibility":"private","ingest_policy":{"zdr_mode":"on","ephemeral_only":true}}' "$own_org")
single_status=$({
  printf 'silent\noutput = "/dev/null"\nwrite-out = "%%{http_code}"\nrequest = "POST"\nurl = "http://127.0.0.1:8010/v1/documents/"\n'
  printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\n' "$bearer"
  printf 'data = "%s"\n' "${single_body//\"/\\\"}"
} | "${compose[@]}" exec -T documents-api curl --config - 2>/dev/null)
[ "$single_status" = "403" ] || { echo "isolated ZDR single guard failed" >&2; exit 1; }

bulk_body=$(printf '{"documents":[%s]}' "$single_body")
bulk_response=$({
  printf 'silent\nrequest = "POST"\nurl = "http://127.0.0.1:8010/v1/documents/bulk"\n'
  printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\n' "$bearer"
  printf 'data = "%s"\n' "${bulk_body//\"/\\\"}"
} | "${compose[@]}" exec -T documents-api curl --config - 2>/dev/null)
BULK_RESPONSE="$bulk_response" node -e 'const value=JSON.parse(process.env.BULK_RESPONSE); if(value.accepted!==0 || value.rejected!==1) process.exit(1)'
[ "$before" = "$(snapshot_content)" ] || { echo "isolated ZDR content snapshot changed" >&2; exit 1; }

DPV2_COMPOSE_PROJECT="$project" \
DPV2_TEST_ORG_ID="$own_org" \
DPV2_SPOOF_ORG_ID="$spoof_org" \
DPV2_USER_BEARER="$bearer" \
DPV2_QUALITY_BEARER="$bearer" \
DPV2_QUICKWIT_ADMIN_BEARER="$bearer" \
"$ROOT/scripts/auth-regression-matrix.sh"

echo "PASS: isolated MVP auth and ZDR runtime matrix"
