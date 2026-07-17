#!/usr/bin/env bash
set -euo pipefail
set +x
umask 077

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
ISOLATED_OVERRIDE="$ROOT/tests/e2e/isolated/docker-compose.yml"
AUTHORITY_OVERRIDE="$ROOT/tests/e2e/isolated/real-authority-compose.yml"
KEY_FIXTURE="$ROOT/tests/e2e/isolated/fixture.mjs"
USER_GRPC_TLS_FIXTURE="$ROOT/tests/e2e/isolated/generate-user-core-grpc-tls.sh"

for command in docker node openssl git; do
  command -v "$command" >/dev/null 2>&1 || { echo "missing required command: $command" >&2; exit 2; }
done

suffix=$(openssl rand -hex 6)
project="dpv2-real-authority-e2e-$suffix"
internal_network="$project-net"
bus_network="$project-bus"
runtime_dir=$(mktemp -d "$ROOT/tests/e2e/.real-authority-runtime.XXXXXX")
chmod 700 "$runtime_dir"
export REAL_AUTHORITY_FIXTURE_OUTPUT_DIR="$runtime_dir"

case "$project:$internal_network:$bus_network" in
  dpv2-real-authority-e2e-*:*:* ) ;;
  * ) echo "refusing non-isolated project names" >&2; exit 2 ;;
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
export MODEL_PLANE_EMBEDDING_INFERENCE_SERVICE_API_KEY=$(openssl rand -hex 32)
export USER_CORE_RETRIEVAL_TOKEN=$(openssl rand -hex 32)
export USER_CORE_DOCUMENTS_TOKEN=$(openssl rand -hex 32)
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
export REAL_AUTHORITY_PLANE_PRINCIPALS_JSON=$(printf '%s' \
  '{"retrieval-engine":{"credential":"'"$CONTROL_POLICY_SERVICE_API_KEY"'","audiences":["control-policy"],"orgIds":[],"allowAnyOrg":true,"scopes":["data:authorization:decide"],"scopesByAudience":{"control-policy":["data:authorization:decide"]}},"embedding-engine":{"credential":"'"$MODEL_PLANE_EMBEDDING_INFERENCE_SERVICE_API_KEY"'","audiences":["inference-core"],"orgIds":[],"allowAnyOrg":true,"scopes":["inference:invoke"],"scopesByAudience":{"inference-core":["inference:invoke"]}},"browser-fixture-seeder":{"credential":"'"$REAL_AUTHORITY_PERSISTENCE_SERVICE_API_KEY"'","audiences":["data-plane"],"orgIds":[],"allowAnyOrg":true,"allowPersistentData":true,"scopes":["documents:write","org:data:write_all"],"scopesByAudience":{"data-plane":["documents:write","org:data:write_all"]}}}')
export REAL_AUTHORITY_USER_CREDENTIALS_JSON=$(printf '%s' \
  '[{"principal":"velion-gateway","audience":"user-core","token":"'"$REAL_AUTHORITY_GATEWAY_TOKEN"'","scopes":["users:read:self","users:write:self"]},{"principal":"retrieval-engine","audience":"user-core","token":"'"$REAL_AUTHORITY_RETRIEVAL_TOKEN"'","scopes":["authz:read"]}]')

compose=(docker compose --project-name "$project" -f "$ROOT/docker-compose.yml" -f "$ISOLATED_OVERRIDE" -f "$AUTHORITY_OVERRIDE")
readonly -a compose
created_bus=0
project_owned=0
images_owned=0
cleanup() {
  rc=$?
  trap - EXIT
  if [ "$project_owned" -eq 1 ] && [[ "$project" == dpv2-real-authority-e2e-* ]]; then
    "${compose[@]}" down --volumes --remove-orphans >/dev/null 2>&1 || true
  fi
  if [ "$created_bus" -eq 1 ] && [[ "$bus_network" == dpv2-real-authority-e2e-*-bus ]]; then
    docker network rm "$bus_network" >/dev/null 2>&1 || true
  fi
  if [ "$images_owned" -eq 1 ]; then
    docker image rm -f "$REAL_AUTHORITY_AUTH_IMAGE" "$REAL_AUTHORITY_USER_IMAGE" >/dev/null 2>&1 || true
  fi
  rm -rf "$runtime_dir"
  exit "$rc"
}
trap cleanup EXIT

existing_containers=$(docker ps -aq --filter "label=com.docker.compose.project=$project")
existing_volumes=$(docker volume ls -q --filter "label=com.docker.compose.project=$project")
existing_networks=$(docker network ls -q --filter "label=com.docker.compose.project=$project")
existing_auth_image=$(docker image ls -q "$REAL_AUTHORITY_AUTH_IMAGE")
existing_user_image=$(docker image ls -q "$REAL_AUTHORITY_USER_IMAGE")
existing_images="$existing_auth_image$existing_user_image"
if [ -n "$existing_containers$existing_volumes$existing_networks$existing_images" ] ||
  docker network inspect "$bus_network" >/dev/null 2>&1; then
  echo "refusing collision with existing isolated project resources" >&2
  exit 2
fi

project_owned=1
images_owned=1
docker network create "$bus_network" >/dev/null
created_bus=1
"${compose[@]}" config --quiet >/dev/null
"${compose[@]}" build auth-core user-core >/dev/null
"${compose[@]}" up -d postgres nats dragonfly auth-core user-core >/dev/null
"${compose[@]}" --profile real-authority-verify run --rm authority-verifier

assert_plaintext_user_grpc_rejected() {
  "${compose[@]}" exec -T auth-core node <<'NODE'
const grpc = require('@grpc/grpc-js');
const client = new grpc.Client(
  'user-core:50012',
  grpc.credentials.createInsecure(),
);
client.waitForReady(Date.now() + 3_000, (error) => {
  client.close();
  process.exit(error ? 0 : 1);
});
NODE
}
assert_plaintext_user_grpc_rejected

echo "PASS: disposable real Auth/User/Control authority integration"
