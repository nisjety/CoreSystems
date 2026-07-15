#!/usr/bin/env bash
set -euo pipefail
set +x
umask 077

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OVERRIDE="$ROOT/tests/e2e/isolated/docker-compose.yml"
FIXTURE="$ROOT/tests/e2e/isolated/fixture.mjs"
MULTISTORE_SNAPSHOT="$ROOT/tests/e2e/multistore-zdr-snapshot.sh"

for command in docker node openssl git; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 2; }
done

suffix="$(openssl rand -hex 6)"
project="dpv2-mvp-e2e-$suffix"
internal_network="$project-net"
bus_network="$project-bus"
runtime_dir=$(mktemp -d "$ROOT/tests/e2e/.isolated-runtime.XXXXXX")
cleanup_runtime() {
  rm -rf "$runtime_dir"
}
trap cleanup_runtime EXIT

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
export DATAPLANE_NATS_TOKEN="$(openssl rand -hex 32)"
export DOCUMENTS_GDPR_NATS_PASSWORD="$(openssl rand -hex 32)"
export DATAPLANE_DRAGONFLY_PASSWORD="$(openssl rand -hex 32)"
export MINIO_ROOT_USER="isolated$(openssl rand -hex 8)"
export MINIO_ROOT_PASSWORD="$(openssl rand -hex 24)"
export GRAFANA_ADMIN_PASSWORD="$(openssl rand -hex 24)"
export CONTROL_POLICY_SERVICE_API_KEY="$(openssl rand -hex 32)"
export MODEL_PLANE_INFERENCE_SERVICE_API_KEY="$(openssl rand -hex 32)"
export USER_CORE_RETRIEVAL_TOKEN="$(openssl rand -hex 32)"
export USER_CORE_DOCUMENTS_TOKEN="$(openssl rand -hex 32)"
export AUTH_CORE_ISSUER="http://auth-core:3011/api/convex-auth"
export JWT_REQUIRED_ISSUER="$AUTH_CORE_ISSUER"

node "$FIXTURE" prepare "$runtime_dir"
export ISOLATED_JWKS_FILE="$runtime_dir/jwks.json"
export ISOLATED_PUBLIC_KEY_FILE="$runtime_dir/public.pem"
export ISOLATED_PRIVATE_KEY_FILE="$runtime_dir/private.pem"
export DOCUMENTS_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/documents-events.pem"
export DOCUMENTS_EVENT_VERIFYING_PUBLIC_KEY_PATH="$runtime_dir/documents-events.pub"
export EMBEDDING_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/embedding-events.pem"
export EMBEDDING_EVENT_VERIFYING_PUBLIC_KEY_PATH="$runtime_dir/embedding-events.pub"
export INDEX_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/index-events.pem"
export INDEX_EVENT_VERIFYING_PUBLIC_KEY_PATH="$runtime_dir/index-events.pub"
export WIKI_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/wiki-events.pem"
export WIKI_EVENT_VERIFYING_PUBLIC_KEY_PATH="$runtime_dir/wiki-events.pub"
export RETRIEVAL_EVENT_SIGNING_PRIVATE_KEY_PATH="$runtime_dir/retrieval-events.pem"
export RETRIEVAL_EVENT_VERIFYING_PUBLIC_KEY_PATH="$runtime_dir/retrieval-events.pub"

compose=(docker compose --project-name "$project" -f "$ROOT/docker-compose.yml" -f "$OVERRIDE")

build_services_sequentially() {
  local service
  for service in "$@"; do
    if ! "${compose[@]}" build "$service" >/dev/null; then
      echo "isolated MVP image build failed for $service" >&2
      return 1
    fi
  done
}

existing_containers=$(docker ps -aq --filter "label=com.docker.compose.project=$project")
existing_volumes=$(docker volume ls -q --filter "label=com.docker.compose.project=$project")
existing_networks=$(docker network ls -q --filter "label=com.docker.compose.project=$project")
if [ -n "$existing_containers$existing_volumes$existing_networks" ] ||
  docker network inspect "$bus_network" >/dev/null 2>&1; then
  rm -rf "$runtime_dir"
  echo "refusing collision with existing isolated project resources" >&2
  exit 2
fi
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
trap cleanup EXIT

docker network create "$bus_network" >/dev/null
created_network=1
"${compose[@]}" config >/dev/null
build_services_sequentially \
  migrate retrieval-engine index-engine embedding-engine graph-index \
  quickwit-adapter documents-api wiki-store data-orchestrator data-quality
if ! "${compose[@]}" up -d --wait --wait-timeout 600; then
  echo "isolated stack startup failed; sanitized migrator diagnostics follow" >&2
  "${compose[@]}" logs --no-color --tail 120 migrate >&2 || true
  exit 1
fi

own_org="isolated-$(openssl rand -hex 12)"
spoof_org="isolated-$(openssl rand -hex 12)"
user_id="isolated-user-$(openssl rand -hex 8)"
scopes="data:read,data:quality:admin,data:orchestrate,data:admin,data:search:rebuild,data:search:rebuild:approve,documents:read,documents:write,org:data:write_all,wiki.read,wiki.write"
bearer=$(JWT_ISSUER="$AUTH_CORE_ISSUER" JWT_ORG_ID="$own_org" JWT_USER_ID="$user_id" JWT_SCOPES="$scopes" JWT_ZDR=true node "$FIXTURE" mint "$runtime_dir")
durable_bearer=$(JWT_ISSUER="$AUTH_CORE_ISSUER" JWT_ORG_ID="$own_org" JWT_USER_ID="$user_id" JWT_SCOPES="$scopes" JWT_ZDR=false node "$FIXTURE" mint "$runtime_dir")

snapshot_content() {
  "${compose[@]}" exec -T postgres psql -U dataplane -d dataplane -Atq -c \
    'SELECT (SELECT count(*) FROM documents),(SELECT count(*) FROM knowledge_units),(SELECT count(*) FROM retrieval_runs),(SELECT count(*) FROM retrieval_candidates),(SELECT count(*) FROM graph_entities),(SELECT count(*) FROM graph_relationships),(SELECT count(*) FROM source_objects),(SELECT count(*) FROM documents_outbox);' \
    | openssl dgst -sha256
}

wait_for_stable_snapshot() {
  local previous current stable
  previous=$(snapshot_content)
  stable=0
  for _ in {1..30}; do
    sleep 1
    current=$(snapshot_content)
    if [ "$current" = "$previous" ]; then
      stable=$((stable + 1))
      if [ "$stable" -ge 5 ]; then
        printf '%s' "$current"
        return 0
      fi
    else
      previous="$current"
      stable=0
    fi
  done
  echo "isolated content snapshot did not become quiescent" >&2
  return 1
}

multistore_snapshot() {
  DPV2_COMPOSE_PROJECT="$project" "$MULTISTORE_SNAPSHOT"
}

wait_for_stable_multistore_snapshot() {
  local previous current stable
  previous=$(multistore_snapshot)
  stable=0
  for _ in {1..20}; do
    sleep 1
    current=$(multistore_snapshot)
    if [ "$current" = "$previous" ]; then
      stable=$((stable + 1))
      if [ "$stable" -ge 3 ]; then
        printf '%s' "$current"
        return 0
      fi
    else
      previous="$current"
      stable=0
    fi
  done
  echo "isolated multi-store snapshot did not become quiescent" >&2
  return 1
}

# Establish that the authenticated write path is functional before treating a
# ZDR denial as evidence. This synthetic document is confined to disposable
# volumes and is removed with the isolated project.
control_body=$(printf '{"org_id":"%s","source":"isolated://write-control","type":"text","title":"isolated-write-control","content":"synthetic write authorization control","visibility":"private","zdr_classification":"internal"}' "$own_org")
control_response=$({
  printf 'silent\nwrite-out = "\\n%%{http_code}"\nrequest = "POST"\nurl = "http://127.0.0.1:8010/v1/documents/"\n'
  printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\n' "$durable_bearer"
  printf 'data = "%s"\n' "${control_body//\"/\\\"}"
} | "${compose[@]}" exec -T documents-api curl --config - 2>/dev/null)
control_status=${control_response##*$'\n'}
control_response_body=${control_response%$'\n'*}
[ "$control_status" = "201" ] || { echo "positive write authorization control failed" >&2; exit 1; }
CONTROL_RESPONSE_BODY="$control_response_body" CONTROL_ORG_ID="$own_org" node -e '
  const value=JSON.parse(process.env.CONTROL_RESPONSE_BODY);
  if (!value.document_id || value.org_id !== process.env.CONTROL_ORG_ID) process.exit(1);
' || { echo "positive write authorization control returned an invalid document" >&2; exit 1; }

before_postgres=$(wait_for_stable_snapshot)
before=$(wait_for_stable_multistore_snapshot)
single_body=$(printf '{"org_id":"%s","source":"isolated://signed-zdr-single","type":"text","title":"isolated-signed-zdr","content":"signed restrictive fixture","visibility":"private"}' "$own_org")
single_response=$({
  printf 'silent\nwrite-out = "\\n%%{http_code}"\nrequest = "POST"\nurl = "http://127.0.0.1:8010/v1/documents/"\n'
  printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\n' "$bearer"
  printf 'data = "%s"\n' "${single_body//\"/\\\"}"
} | "${compose[@]}" exec -T documents-api curl --config - 2>/dev/null)
single_status=${single_response##*$'\n'}
single_response_body=${single_response%$'\n'*}
[ "$single_status" = "403" ] || { echo "isolated ZDR single guard failed" >&2; exit 1; }
SINGLE_RESPONSE_BODY="$single_response_body" node -e '
  const value=JSON.parse(process.env.SINGLE_RESPONSE_BODY);
  if (value.error !== "verified token zdr=true forbids durable document persistence") process.exit(1);
' || { echo "isolated ZDR single guard returned the wrong contract" >&2; exit 1; }

source_object_body='{"connector":"isolated","source":"isolated://signed-zdr-source-object","external_id":"signed-zdr-source-object","name":"signed-zdr-source-object.txt"}'
source_object_response=$({
  printf 'silent\nwrite-out = "\\n%%{http_code}"\nrequest = "POST"\nurl = "http://127.0.0.1:8010/v1/source-objects/"\n'
  printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\n' "$bearer"
  printf 'data = "%s"\n' "${source_object_body//\"/\\\"}"
} | "${compose[@]}" exec -T documents-api curl --config - 2>/dev/null)
source_object_status=${source_object_response##*$'\n'}
source_object_response_body=${source_object_response%$'\n'*}
[ "$source_object_status" = "403" ] || { echo "isolated ZDR source-object guard failed" >&2; exit 1; }
SOURCE_OBJECT_RESPONSE_BODY="$source_object_response_body" node -e '
  const value=JSON.parse(process.env.SOURCE_OBJECT_RESPONSE_BODY);
  if (value.error !== "verified token zdr=true forbids durable source-object persistence") process.exit(1);
' || { echo "isolated ZDR source-object guard returned the wrong contract" >&2; exit 1; }

bulk_zdr_body=$(printf '{"org_id":"%s","source":"isolated://body-zdr-bulk","type":"text","title":"isolated-body-zdr","content":"body restrictive fixture","visibility":"private","ingest_policy":{"zdr_mode":"on","ephemeral_only":true}}' "$own_org")
bulk_body=$(printf '{"documents":[%s]}' "$bulk_zdr_body")
bulk_response=$({
  printf 'silent\nrequest = "POST"\nurl = "http://127.0.0.1:8010/v1/documents/bulk"\n'
  printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\n' "$durable_bearer"
  printf 'data = "%s"\n' "${bulk_body//\"/\\\"}"
} | "${compose[@]}" exec -T documents-api curl --config - 2>/dev/null)
BULK_RESPONSE="$bulk_response" node -e '
  const value=JSON.parse(process.env.BULK_RESPONSE);
  if (value.accepted !== 0 || value.rejected !== 1 ||
      !Array.isArray(value.rejection_reasons) ||
      !value.rejection_reasons.some((reason) => reason.includes("ingest_policy.zdr_mode=on or ephemeral_only=true forbids durable document persistence"))) process.exit(1);
' || { echo "isolated ZDR bulk guard returned the wrong contract" >&2; exit 1; }

retrieval_probe="signed-zdr-multistore-$suffix"
retrieval_body=$(printf '{"org_id":"%s","query":"%s","top_k":1,"zdr_mode":"ephemeral"}' "$own_org" "$retrieval_probe")
retrieval_status=$({
  printf 'silent\noutput = "/dev/null"\nwrite-out = "%%{http_code}"\nrequest = "POST"\nurl = "http://127.0.0.1:8004/v1/retrieve"\n'
  printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\n' "$bearer"
  printf 'data = "%s"\n' "${retrieval_body//\"/\\\"}"
} | "${compose[@]}" exec -T retrieval-engine curl --config - 2>/dev/null)
[ "$retrieval_status" = "200" ] || { echo "signed ZDR retrieval guard failed" >&2; exit 1; }

wiki_body=$(printf '{"workspace_id":"isolated-workspace","title":"signed-zdr-wiki","path":"/signed-zdr-%s"}' "$suffix")
wiki_response=$({
  printf 'silent\nwrite-out = "\\n%%{http_code}"\nrequest = "POST"\nurl = "http://127.0.0.1:8011/v1/wiki/pages"\n'
  printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\n' "$bearer"
  printf 'data = "%s"\n' "${wiki_body//\"/\\\"}"
} | "${compose[@]}" exec -T wiki-store curl --config - 2>/dev/null)
wiki_status=${wiki_response##*$'\n'}
[ "$wiki_status" = "403" ] || { echo "signed ZDR wiki HTTP mutation guard failed" >&2; exit 1; }

retrieval_grpc=$("${compose[@]}" port retrieval-engine 50052)
graph_grpc=$("${compose[@]}" port graph-index 50053)
wiki_grpc=$("${compose[@]}" port wiki-store 50054)
for address in "$retrieval_grpc" "$graph_grpc" "$wiki_grpc"; do
  case "$address" in
    127.0.0.1:*) ;;
    *) echo "isolated gRPC endpoint is not loopback-bound" >&2; exit 1 ;;
  esac
done

GRPC_RETRIEVAL_ADDR="$retrieval_grpc" \
GRPC_GRAPH_ADDR="$graph_grpc" \
GRPC_WIKI_ADDR="$wiki_grpc" \
GRPC_VALID_BEARER="$bearer" \
GRPC_APPROVAL_DENIED_BEARER="$durable_bearer" \
GRPC_VALID_ORG_ID="$own_org" \
GRPC_OTHER_ORG_ID="$spoof_org" \
"$ROOT/tests/e2e/grpc-auth-matrix.sh"

for _ in {1..10}; do
  sleep 1
  [ "$before_postgres" = "$(snapshot_content)" ] || {
    echo "isolated ZDR content snapshot changed after quiescence" >&2
    exit 1
  }
done
after=$(wait_for_stable_multistore_snapshot)
[ "$before" = "$after" ] || {
  echo "isolated multi-store ZDR snapshot changed" >&2
  exit 1
}

DPV2_COMPOSE_PROJECT="$project" \
DPV2_TEST_ORG_ID="$own_org" \
DPV2_SPOOF_ORG_ID="$spoof_org" \
DPV2_USER_BEARER="$bearer" \
DPV2_QUALITY_BEARER="$bearer" \
DPV2_QUICKWIT_ADMIN_BEARER="$bearer" \
"$ROOT/scripts/auth-regression-matrix.sh"

echo "PASS: isolated MVP HTTP/gRPC auth and multi-store ZDR runtime matrix"
