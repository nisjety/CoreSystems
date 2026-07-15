#!/usr/bin/env bash
set -euo pipefail
set +x
umask 077

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
FRONTEND_ROOT="$ROOT/../Frontend Plane/velionv3"
ISOLATED_OVERRIDE="$ROOT/tests/e2e/isolated/docker-compose.yml"
AUTHORITY_OVERRIDE="$ROOT/tests/e2e/isolated/real-authority-compose.yml"
BROWSER_OVERRIDE="$ROOT/tests/e2e/isolated/real-authority-browser-compose.yml"
KEY_FIXTURE="$ROOT/tests/e2e/isolated/fixture.mjs"
USER_GRPC_TLS_FIXTURE="$ROOT/tests/e2e/isolated/generate-user-core-grpc-tls.sh"

for command in docker node openssl git curl pnpm grpcurl; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 2; }
done

random_loopback_port() {
  node -e 'const net=require("node:net");const server=net.createServer();server.listen(0,"127.0.0.1",()=>{process.stdout.write(String(server.address().port));server.close();});'
}

suffix=$(openssl rand -hex 6)
project="dpv2-real-authority-browser-e2e-$suffix"
internal_network="$project-net"
bus_network="$project-bus"
runtime_dir=$(mktemp -d "$ROOT/tests/e2e/.real-authority-browser-runtime.XXXXXX")
chmod 700 "$runtime_dir"
frontend_port=$(random_loopback_port)

case "$project:$internal_network:$bus_network" in
  dpv2-real-authority-browser-e2e-*:*:* ) ;;
  * ) echo "refusing non-isolated browser project names" >&2; exit 2 ;;
esac
[ "$bus_network" != "inter-plane-bus" ] || { echo "refusing shared inter-plane network" >&2; exit 2; }

export DATA_PLANE_COMPOSE_PROJECT="$project"
export DPV2_NETWORK_NAME="$internal_network"
export INTER_PLANE_BUS_NETWORK="$bus_network"
export SOURCE_REVISION=$(git -C "$ROOT" rev-parse HEAD)
export BUILD_DATE=$(date -u +%Y-%m-%dT%H:%M:%SZ)
export COMPOSE_PARALLEL_LIMIT="${COMPOSE_PARALLEL_LIMIT:-1}"
export POSTGRES_USER=dataplane
export POSTGRES_DB=dataplane
export POSTGRES_PASSWORD=$(openssl rand -hex 24)
export DATAPLANE_DRAGONFLY_PASSWORD=$(openssl rand -hex 32)
export DATAPLANE_NATS_TOKEN=$(openssl rand -hex 32)
export DOCUMENTS_GDPR_NATS_PASSWORD=$(openssl rand -hex 32)
export MINIO_ROOT_USER="isolated$(openssl rand -hex 8)"
export MINIO_ROOT_PASSWORD=$(openssl rand -hex 24)
export GRAFANA_ADMIN_PASSWORD=$(openssl rand -hex 24)
export CONTROL_POLICY_SERVICE_API_KEY=$(openssl rand -hex 32)
export REAL_AUTHORITY_PERSISTENCE_SERVICE_ID=browser-fixture-seeder
export REAL_AUTHORITY_PERSISTENCE_SERVICE_API_KEY=$(openssl rand -hex 32)
export MODEL_PLANE_INFERENCE_SERVICE_API_KEY=$(openssl rand -hex 32)
export USER_CORE_RETRIEVAL_TOKEN=$(openssl rand -hex 32)
export USER_CORE_DOCUMENTS_TOKEN=$(openssl rand -hex 32)
export INTERNAL_API_KEY=$(openssl rand -hex 32)
export AUTH_CORE_ISSUER=http://auth-core:3011/api/convex-auth
export JWT_REQUIRED_ISSUER="$AUTH_CORE_ISSUER"

node "$KEY_FIXTURE" prepare "$runtime_dir"
"$USER_GRPC_TLS_FIXTURE" "$runtime_dir"
export ISOLATED_PRIVATE_KEY_FILE="$runtime_dir/private.pem"
export ISOLATED_PUBLIC_KEY_FILE="$runtime_dir/public.pem"
export ISOLATED_JWKS_FILE="$runtime_dir/jwks.json"
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

export REAL_AUTHORITY_RUN_ID="$suffix"
export REAL_AUTHORITY_AUTH_IMAGE="$project-auth-core:local"
export REAL_AUTHORITY_USER_IMAGE="$project-user-core:local"
export REAL_AUTHORITY_GATEWAY_IMAGE="$project-gateway:local"
export REAL_AUTHORITY_FIXTURE_OUTPUT_DIR="$runtime_dir"
export REAL_AUTHORITY_PUBLIC_ORIGIN="http://localhost:$frontend_port"
export REAL_AUTHORITY_AUTH_GRPC_RETRIEVAL_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_USER_GRPC_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_USER_AUTH_INTERNAL_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_AUTH_GRPC_CREDENTIALS_FILE="$runtime_dir/auth-grpc-service-credentials.json"
export REAL_AUTHORITY_USER_GRPC_CLIENT_CREDENTIAL_FILE="$runtime_dir/auth-user-grpc-client.json"
export REAL_AUTHORITY_USER_GRPC_SERVICE_CREDENTIALS_FILE="$runtime_dir/user-grpc-service-credentials.json"
export REAL_AUTHORITY_USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE="$runtime_dir/user-auth-internal-client.json"
export REAL_AUTHORITY_AUTH_INTERNAL_CREDENTIALS_FILE="$runtime_dir/auth-internal-service-credentials.json"
export REAL_AUTHORITY_USER_GRPC_TLS_CA_FILE="$runtime_dir/user-grpc-tls-ca.pem"
export REAL_AUTHORITY_USER_GRPC_TLS_CERT_FILE="$runtime_dir/user-grpc-tls-server.pem"
export REAL_AUTHORITY_USER_GRPC_TLS_KEY_FILE="$runtime_dir/user-grpc-tls-server-key.pem"
REAL_AUTHORITY_AUTH_GRPC_RETRIEVAL_TOKEN="$REAL_AUTHORITY_AUTH_GRPC_RETRIEVAL_TOKEN" \
REAL_AUTHORITY_AUTH_GRPC_CREDENTIALS_FILE="$REAL_AUTHORITY_AUTH_GRPC_CREDENTIALS_FILE" \
REAL_AUTHORITY_USER_GRPC_TOKEN="$REAL_AUTHORITY_USER_GRPC_TOKEN" \
REAL_AUTHORITY_USER_AUTH_INTERNAL_TOKEN="$REAL_AUTHORITY_USER_AUTH_INTERNAL_TOKEN" \
REAL_AUTHORITY_USER_GRPC_CLIENT_CREDENTIAL_FILE="$REAL_AUTHORITY_USER_GRPC_CLIENT_CREDENTIAL_FILE" \
REAL_AUTHORITY_USER_GRPC_SERVICE_CREDENTIALS_FILE="$REAL_AUTHORITY_USER_GRPC_SERVICE_CREDENTIALS_FILE" \
REAL_AUTHORITY_USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE="$REAL_AUTHORITY_USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE" \
REAL_AUTHORITY_AUTH_INTERNAL_CREDENTIALS_FILE="$REAL_AUTHORITY_AUTH_INTERNAL_CREDENTIALS_FILE" \
  node --input-type=module <<'NODE'
import { writeFileSync } from "node:fs";

const credentials = [{
  credentialId: "retrieval-primary",
  principal: "retrieval-engine",
  audience: "auth-core",
  token: process.env.REAL_AUTHORITY_AUTH_GRPC_RETRIEVAL_TOKEN,
  scopes: ["auth:token:validate"],
}];
writeFileSync(
  process.env.REAL_AUTHORITY_AUTH_GRPC_CREDENTIALS_FILE,
  `${JSON.stringify(credentials)}\n`,
  { mode: 0o600 },
);
const authUserCredential = {
  credentialId: "auth-core-isolated",
  principal: "auth-core",
  audience: "user-core-grpc",
  token: process.env.REAL_AUTHORITY_USER_GRPC_TOKEN,
};
const authUserMethods = [
  "/user.v1.UserService/CreateUser",
  "/user.v1.UserService/UpdateUser",
  "/user.v1.UserService/GetUserByEmail",
  "/user.v1.UserService/CreateSession",
  "/user.v1.UserService/GetUser",
  "/user.v1.UserService/DeleteUser",
  "/user.v1.UserService/HealthCheck",
];
const userAuthCredential = {
  credentialId: "user-core-isolated",
  principal: "user-core",
  audience: "auth-core-internal",
  token: process.env.REAL_AUTHORITY_USER_AUTH_INTERNAL_TOKEN,
};
writeFileSync(
  process.env.REAL_AUTHORITY_USER_GRPC_CLIENT_CREDENTIAL_FILE,
  `${JSON.stringify(authUserCredential)}\n`,
  { mode: 0o600 },
);
writeFileSync(
  process.env.REAL_AUTHORITY_USER_GRPC_SERVICE_CREDENTIALS_FILE,
  `${JSON.stringify([{ ...authUserCredential, methods: authUserMethods }])}\n`,
  { mode: 0o600 },
);
writeFileSync(
  process.env.REAL_AUTHORITY_USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE,
  `${JSON.stringify(userAuthCredential)}\n`,
  { mode: 0o600 },
);
writeFileSync(
  process.env.REAL_AUTHORITY_AUTH_INTERNAL_CREDENTIALS_FILE,
  `${JSON.stringify([{ ...userAuthCredential, scopes: ["oauth:token:read", "oauth:token:refresh", "nats:authenticate", "auth:admin"] }])}\n`,
  { mode: 0o600 },
);
NODE
export REAL_AUTHORITY_INTERNAL_SECRET=$(openssl rand -hex 32)
export REAL_AUTHORITY_INTERNAL_API_KEY=$(openssl rand -hex 32)
export REAL_AUTHORITY_MEMBERSHIP_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_APPLICATION_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_AUTH_TO_USER_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_GATEWAY_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_RETRIEVAL_TOKEN="$USER_CORE_RETRIEVAL_TOKEN"
export REAL_AUTHORITY_BETTER_AUTH_SECRET=$(openssl rand -hex 32)
export REAL_AUTHORITY_TOKEN_ENCRYPTION_KEY=$(openssl rand -base64 32)
export REAL_AUTHORITY_USER_JWT_SECRET=$(openssl rand -hex 32)
export REAL_AUTHORITY_RESEND_API_KEY="re_$(openssl rand -hex 32)"
export REAL_AUTHORITY_ORG_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_BILLING_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_MODEL_FIXTURE_KEY=$(openssl rand -hex 32)
export REAL_AUTHORITY_GATEWAY_SESSION_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_GATEWAY_BILLING_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_GATEWAY_ORG_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_GATEWAY_AUDIT_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_GATEWAY_NOTIFICATION_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_GATEWAY_CONVERSATION_TOKEN=$(openssl rand -hex 32)
export REAL_AUTHORITY_PLANE_PRINCIPALS_JSON=$(printf '%s' \
  '{"retrieval-engine":{"credential":"'"$CONTROL_POLICY_SERVICE_API_KEY"'","audiences":["control-policy"],"orgIds":[],"allowAnyOrg":true,"scopes":["data:authorization:decide"],"scopesByAudience":{"control-policy":["data:authorization:decide"]}},"browser-fixture-seeder":{"credential":"'"$REAL_AUTHORITY_PERSISTENCE_SERVICE_API_KEY"'","audiences":["data-plane"],"orgIds":[],"allowAnyOrg":true,"allowPersistentData":true,"scopes":["documents:write","org:data:write_all"],"scopesByAudience":{"data-plane":["documents:write","org:data:write_all"]}}}')
export REAL_AUTHORITY_USER_CREDENTIALS_JSON=$(printf '%s' \
  '[{"principal":"velion-gateway","audience":"user-core","token":"'"$REAL_AUTHORITY_GATEWAY_TOKEN"'","scopes":["users:read:self","users:write:self"]},{"principal":"retrieval-engine","audience":"user-core","token":"'"$REAL_AUTHORITY_RETRIEVAL_TOKEN"'","scopes":["authz:read"]}]')

readonly -a compose=(docker compose --project-name "$project" -f "$ROOT/docker-compose.yml" -f "$ISOLATED_OVERRIDE" -f "$AUTHORITY_OVERRIDE" -f "$BROWSER_OVERRIDE")

build_services_sequentially() {
  local service
  for service in "$@"; do
    if ! "${compose[@]}" build "$service" >/dev/null; then
      echo "isolated browser image build failed for $service" >&2
      return 1
    fi
  done
}

created_bus=0
project_owned=0
images_owned=0
vite_pid=""

remove_owned_project_images() {
  local expected_project=$1 repository failed=0
  case "$expected_project" in
    dpv2-real-authority-browser-e2e-* ) ;;
    * ) echo "refusing image cleanup for non-isolated project" >&2; return 1 ;;
  esac
  while IFS= read -r repository; do
    case "$repository" in
      "$expected_project"-*:*)
        docker image rm -f "$repository" >/dev/null 2>&1 || failed=1
        ;;
    esac
  done < <(docker image ls --format '{{.Repository}}:{{.Tag}}')
  return "$failed"
}

cleanup_runtime_dir() {
  case "$runtime_dir" in
    "$ROOT"/tests/e2e/.real-authority-browser-runtime.* ) ;;
    * ) echo "refusing cleanup for non-isolated runtime directory" >&2; return 1 ;;
  esac
  chmod -R u+rwX "$runtime_dir" >/dev/null 2>&1 || true
  rm -rf "$runtime_dir"
  test ! -e "$runtime_dir"
}

cleanup() {
  rc=$?
  cleanup_failed=0
  trap - EXIT
  if [ -n "$vite_pid" ]; then
    kill "$vite_pid" >/dev/null 2>&1 || true
    wait "$vite_pid" >/dev/null 2>&1 || true
  fi
  if [ "$project_owned" -eq 1 ] && [[ "$project" == dpv2-real-authority-browser-e2e-* ]]; then
    "${compose[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true
  fi
  if [ "$created_bus" -eq 1 ] && [[ "$bus_network" == dpv2-real-authority-browser-e2e-*-bus ]]; then
    docker network rm "$bus_network" >/dev/null 2>&1 || true
  fi
  if [ "$images_owned" -eq 1 ]; then
    remove_owned_project_images "$project" || cleanup_failed=1
  fi
  if ! cleanup_runtime_dir; then
    echo "isolated browser runtime cleanup failed" >&2
    cleanup_failed=1
  fi
  if [ "$rc" -eq 0 ] && [ "$cleanup_failed" -ne 0 ]; then
    rc=1
  fi
  exit "$rc"
}
trap cleanup EXIT

existing_containers=$(docker ps -aq --filter "label=com.docker.compose.project=$project")
existing_volumes=$(docker volume ls -q --filter "label=com.docker.compose.project=$project")
existing_networks=$(docker network ls -q --filter "label=com.docker.compose.project=$project")
existing_images=$({
  docker image ls -q "$REAL_AUTHORITY_AUTH_IMAGE"
  docker image ls -q "$REAL_AUTHORITY_USER_IMAGE"
  docker image ls -q "$REAL_AUTHORITY_GATEWAY_IMAGE"
})
if [ -n "$existing_containers$existing_volumes$existing_networks$existing_images" ] ||
  docker network inspect "$bus_network" >/dev/null 2>&1; then
  echo "refusing collision with existing isolated browser resources" >&2
  exit 2
fi

project_owned=1
images_owned=1
docker network create "$bus_network" >/dev/null
created_bus=1
"${compose[@]}" config --quiet >/dev/null
build_services_sequentially \
  auth-core user-core gateway documents-api index-engine embedding-engine \
  graph-index retrieval-engine wiki-store
if ! "${compose[@]}" up -d postgres nats dragonfly auth-core user-core >/dev/null; then
  echo "isolated authority dependency startup failed; sanitized database bootstrap diagnostics follow" >&2
  "${compose[@]}" logs --no-color --tail 80 authority-db-init >&2 || true
  exit 1
fi
"${compose[@]}" --profile real-authority-verify run --rm authority-verifier

authority_file="$runtime_dir/authority.json"
[ -s "$authority_file" ] || { echo "real authority verifier did not emit its private browser fixture" >&2; exit 1; }
[ "$(stat -f %Lp "$authority_file")" = "600" ] || { echo "real authority browser fixture is not mode 0600" >&2; exit 1; }

"${compose[@]}" up -d --wait --wait-timeout 600 documents-api index-engine embedding-engine graph-index retrieval-engine wiki-store gateway >/dev/null

read_authority_field() {
  AUTHORITY_FILE="$authority_file" AUTHORITY_INDEX="$1" AUTHORITY_FIELD="$2" node -e '
    const fs=require("node:fs");
    const value=JSON.parse(fs.readFileSync(process.env.AUTHORITY_FILE,"utf8")).users?.[Number(process.env.AUTHORITY_INDEX)]?.[process.env.AUTHORITY_FIELD];
    if (typeof value !== "string" || !value.trim()) process.exit(1);
    process.stdout.write(value);
  '
}

real_bearer=$(read_authority_field 0 dataPlaneToken)
real_approval_denied_bearer=$(read_authority_field 0 persistentDataPlaneToken)
real_org=$(read_authority_field 0 orgId)
other_org=$(read_authority_field 1 orgId)
retrieval_grpc=$("${compose[@]}" port retrieval-engine 50052)
graph_grpc=$("${compose[@]}" port graph-index 50053)
wiki_grpc=$("${compose[@]}" port wiki-store 50054)
for address in "$retrieval_grpc" "$graph_grpc" "$wiki_grpc"; do
  case "$address" in
    127.0.0.1:* ) ;;
    * ) echo "isolated browser gRPC endpoint is not loopback-bound" >&2; exit 1 ;;
  esac
done
GRPC_RETRIEVAL_ADDR="$retrieval_grpc" \
GRPC_GRAPH_ADDR="$graph_grpc" \
GRPC_WIKI_ADDR="$wiki_grpc" \
GRPC_VALID_BEARER="$real_bearer" \
GRPC_APPROVAL_DENIED_BEARER="$real_approval_denied_bearer" \
GRPC_VALID_ORG_ID="$real_org" \
GRPC_OTHER_ORG_ID="$other_org" \
"$ROOT/tests/e2e/grpc-auth-matrix.sh"

seed_document() {
  local index="$1" title="$2" marker="$3"
  local org_id durable_bearer request_body response status response_body document_id
  org_id=$(read_authority_field "$index" orgId)
  durable_bearer=$(read_authority_field "$index" persistentDataPlaneToken)
  request_body=$(printf '{"org_id":"%s","source":"isolated://real-authority-browser/%s","type":"text","title":"%s","content":"%s is the unique grounded browser GraphRAG fixture. Search %s to find this source.","visibility":"org","zdr_classification":"internal"}' \
    "$org_id" "$marker" "$title" "$marker" "$marker")
  response=$({
    printf 'silent\nshow-error\nwrite-out = "\\n%%{http_code}"\nrequest = "POST"\nurl = "http://127.0.0.1:8010/v1/documents/"\n'
    printf 'header = "Authorization: Bearer %s"\nheader = "Content-Type: application/json"\n' "$durable_bearer"
    printf 'data = "%s"\n' "${request_body//\"/\\\"}"
  } | "${compose[@]}" exec -T documents-api curl --config - 2>/dev/null)
  status=${response##*$'\n'}
  response_body=${response%$'\n'*}
  [ "$status" = "201" ] || { echo "isolated browser document seed failed (HTTP $status)" >&2; exit 1; }
  document_id=$(DOCUMENT_RESPONSE_BODY="$response_body" node -e '
    const value=JSON.parse(process.env.DOCUMENT_RESPONSE_BODY);
    const id=value.document_id;
    if (typeof id!=="string" || !/^[A-Za-z0-9_-]{1,256}$/.test(id)) process.exit(1);
    process.stdout.write(id);
  ')
  printf '%s\t%s\t%s\t%s\n' "$org_id" "$document_id" "$title" "$marker"
}

fixture_one=$(seed_document 0 "Isolated Knowledge Alpha $suffix" "GRAPH_NODE_ALPHA_$suffix")
fixture_two=$(seed_document 1 "Isolated Knowledge Beta $suffix" "GRAPH_NODE_BETA_$suffix")
browser_fixture_file="$runtime_dir/browser-fixtures.json"
FIXTURE_ONE="$fixture_one" FIXTURE_TWO="$fixture_two" FIXTURE_OUTPUT="$browser_fixture_file" node -e '
  const fs=require("node:fs");
  const parse=(line)=>{const [orgId,documentId,title,graphMarker]=line.split("\t");return {orgId,documentId,title,graphMarker};};
  fs.writeFileSync(process.env.FIXTURE_OUTPUT,JSON.stringify({fixtures:[parse(process.env.FIXTURE_ONE),parse(process.env.FIXTURE_TWO)]})+"\n",{mode:0o600});
  fs.chmodSync(process.env.FIXTURE_OUTPUT,0o600);
'

doc_one=$(printf '%s' "$fixture_one" | cut -f2)
doc_two=$(printf '%s' "$fixture_two" | cut -f2)
marker_one=$(printf '%s' "$fixture_one" | cut -f4)
marker_two=$(printf '%s' "$fixture_two" | cut -f4)
pipeline_ready=0
for _ in {1..180}; do
  state=$("${compose[@]}" exec -T postgres psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Atq -c \
    "SELECT (SELECT count(*) FROM knowledge_units WHERE document_id IN ('$doc_one','$doc_two')) || ':' || (SELECT count(*) FROM graph_entities WHERE entity_text IN ('$marker_one','$marker_two'));" 2>/dev/null || true)
  case "$state" in
    [2-9]*:[2-9]* ) pipeline_ready=1; break ;;
  esac
  sleep 1
done
[ "$pipeline_ready" -eq 1 ] || { echo "isolated document-to-GraphRAG pipeline did not become ready" >&2; exit 1; }

gateway_address=$("${compose[@]}" port gateway 3185)
case "$gateway_address" in
  127.0.0.1:* ) ;;
  * ) echo "isolated gateway endpoint is not loopback-bound" >&2; exit 1 ;;
esac

(
  cd "$FRONTEND_ROOT"
  GATEWAY_PROXY_TARGET="http://$gateway_address" \
  VITE_ALLOW_DEV_AUTH_BYPASS=false \
  pnpm dev --host 127.0.0.1 --port "$frontend_port" --strictPort >"$runtime_dir/vite.log" 2>&1
) &
vite_pid=$!

frontend_ready=0
for _ in {1..120}; do
  if curl -fsS --max-time 2 "$REAL_AUTHORITY_PUBLIC_ORIGIN/" >/dev/null 2>&1; then
    frontend_ready=1
    break
  fi
  sleep 1
done
[ "$frontend_ready" -eq 1 ] || { echo "isolated Velion frontend did not become ready" >&2; exit 1; }

REAL_AUTHORITY_BASE_URL="$REAL_AUTHORITY_PUBLIC_ORIGIN" \
REAL_AUTHORITY_FIXTURE_FILE="$authority_file" \
REAL_AUTHORITY_BROWSER_FIXTURE_FILE="$browser_fixture_file" \
REAL_AUTHORITY_PLAYWRIGHT_OUTPUT_DIR="$runtime_dir/playwright" \
pnpm --dir "$FRONTEND_ROOT" exec playwright test --config=playwright.real-authority.config.ts --project=real-authority

echo "PASS: disposable real Auth/User/Data/Velion browser Knowledge and GraphRAG E2E"
