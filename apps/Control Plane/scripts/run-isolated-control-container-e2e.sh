#!/usr/bin/env bash

# Runs the Auth -> Org -> Billing lifecycle through real service containers and
# scoped HTTP middleware. Every container, image, network, database, and secret
# is generated for this run; no Control Plane compose project or volume is used.

set -euo pipefail

root_dir="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
prefix="control-mvp-ct-${PPID}-$$"
network="${prefix}-network"
postgres_container="${prefix}-postgres"
nats_container="${prefix}-nats"
dragonfly_container="${prefix}-dragonfly"
auth_container="${prefix}-auth"
org_container="${prefix}-org"
billing_container="${prefix}-billing"
driver_containers=(
  "${prefix}-driver-seed-and-converge"
  "${prefix}-driver-membership-lifecycle"
  "${prefix}-driver-partial-delete"
  "${prefix}-driver-retry-delete"
)
auth_image="${prefix}-auth:local"
org_image="${prefix}-org:local"
billing_image="${prefix}-billing:local"
driver_image="${prefix}-driver:local"

postgres_password="$(openssl rand -hex 24)"
fixture_id="$(openssl rand -hex 24)"
nats_password="$(openssl rand -hex 32)"
better_auth_secret="$(openssl rand -hex 32)"
resend_api_key="re_$(openssl rand -hex 32)"
membership_authority_token="$(openssl rand -hex 32)"
application_reconciler_token="$(openssl rand -hex 32)"
user_core_auth_token="$(openssl rand -hex 32)"
auth_grpc_gateway_token="$(openssl rand -hex 32)"
auth_internal_nats_token="$(openssl rand -hex 32)"
user_grpc_client_token="$(openssl rand -hex 32)"
org_gateway_token="$(openssl rand -hex 32)"
org_auth_token="$(openssl rand -hex 32)"
billing_gateway_token="$(openssl rand -hex 32)"
billing_auth_token="$(openssl rand -hex 32)"
billing_writer_token="$(openssl rand -hex 32)"
billing_org_client_token="$(openssl rand -hex 32)"
auth_key_dir=""

emit_bounded_container_logs() {
  container="$1"
  docker logs --tail 160 "$container" 2>&1 |
    node "$root_dir/scripts/redact-container-logs.mjs" || true
}

cleanup() {
  status=$?
  if [ "$status" -ne 0 ]; then
    for container in "$auth_container" "$org_container" "$billing_container"; do
      emit_bounded_container_logs "$container"
    done
  fi
  for container in \
    "${driver_containers[@]}" \
    "$auth_container" "$org_container" "$billing_container" "$dragonfly_container" "$nats_container" \
    "$postgres_container"; do
    docker rm -f "$container" >/dev/null 2>&1 || true
  done
  docker network rm "$network" >/dev/null 2>&1 || true
  for image in "$driver_image" "$auth_image" "$org_image" "$billing_image"; do
    docker image rm -f "$image" >/dev/null 2>&1 || true
  done
  if [ -n "$auth_key_dir" ]; then
    rm -rf "$auth_key_dir"
  fi
  return "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

for command in docker node openssl; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "required command is unavailable: $command" >&2
    exit 1
  fi
done
if ! docker info >/dev/null 2>&1; then
  echo "Docker is unavailable" >&2
  exit 1
fi

auth_key_dir="$(mktemp -d "${TMPDIR:-/tmp}/control-auth-keys.XXXXXX")"
chmod 700 "$auth_key_dir"
openssl genpkey -algorithm RSA \
  -pkeyopt rsa_keygen_bits:2048 \
  -out "$auth_key_dir/private.pem" >/dev/null 2>&1
openssl pkey \
  -in "$auth_key_dir/private.pem" \
  -pubout \
  -out "$auth_key_dir/public.pem" >/dev/null 2>&1
openssl req -x509 -new \
  -key "$auth_key_dir/private.pem" \
  -sha256 -days 1 \
  -subj '/CN=Control lifecycle disposable User Core CA' \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign' \
  -out "$auth_key_dir/user-grpc-ca.pem" >/dev/null 2>&1
# These are disposable fixture keys on a private temporary path. Read-only
# container mounts need the unprivileged runtime UID to read them.
chmod 755 "$auth_key_dir"
chmod 444 \
  "$auth_key_dir/private.pem" \
  "$auth_key_dir/public.pem" \
  "$auth_key_dir/user-grpc-ca.pem"

echo "[build] isolated Auth, Org, Billing, and lifecycle-driver images"
docker build --tag "$auth_image" "$root_dir/auth-core"
docker build --tag "$org_image" "$root_dir/org-core"
docker build --tag "$billing_image" "$root_dir/billing-core"
docker build \
  --build-arg "AUTH_IMAGE=$auth_image" \
  --file "$root_dir/scripts/fixtures/Dockerfile.control-lifecycle-driver" \
  --tag "$driver_image" \
  "$root_dir"

docker network create "$network" >/dev/null
docker run --detach \
  --name "$postgres_container" \
  --network "$network" \
  --env POSTGRES_USER=control_lifecycle \
  --env "POSTGRES_PASSWORD=$postgres_password" \
  --env POSTGRES_DB=postgres \
  postgres:15.18-alpine3.24@sha256:3d0f7584ed7d04e27fa050d6683a74746608faf21f202be78460d679cc56461f \
  >/dev/null

for _ in $(seq 1 90); do
  if docker exec "$postgres_container" pg_isready \
    -U control_lifecycle -d postgres >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
if ! docker exec "$postgres_container" pg_isready \
  -U control_lifecycle -d postgres >/dev/null 2>&1; then
  echo "disposable Postgres did not become ready" >&2
  emit_bounded_container_logs "$postgres_container"
  exit 1
fi

for database in auth_service org_core billing_service; do
  docker exec "$postgres_container" createdb -U control_lifecycle "$database"
  docker exec "$postgres_container" psql -X -v ON_ERROR_STOP=1 \
    -U control_lifecycle -d "$database" \
    -c "CREATE TABLE control_lifecycle_fixture (fixture_id TEXT PRIMARY KEY, database_name TEXT NOT NULL); INSERT INTO control_lifecycle_fixture (fixture_id, database_name) VALUES ('$fixture_id', '$database');" \
    >/dev/null
done

docker run --detach \
  --name "$nats_container" \
  --network "$network" \
  nats:2.10.29-alpine3.22@sha256:b83efabe3e7def1e0a4a31ec6e078999bb17c80363f881df35edc70fcb6bb927 \
  --jetstream --user control-lifecycle --pass "$nats_password" >/dev/null

docker run --detach \
  --name "$dragonfly_container" \
  --network "$network" \
  docker.dragonflydb.io/dragonflydb/dragonfly:v1.37.0 \
  --logtostderr \
  --proactor_threads=2 \
  --num_shards=1 \
  --maxmemory=512mb >/dev/null

for _ in $(seq 1 45); do
  if docker exec "$dragonfly_container" redis-cli ping 2>/dev/null | grep -q PONG; then
    break
  fi
  sleep 1
done
if ! docker exec "$dragonfly_container" redis-cli ping 2>/dev/null | grep -q PONG; then
  echo "disposable Dragonfly did not become ready" >&2
  exit 1
fi

auth_database_url="postgres://control_lifecycle:${postgres_password}@${postgres_container}:5432/auth_service?sslmode=disable"
org_database_url="postgres://control_lifecycle:${postgres_password}@${postgres_container}:5432/org_core?sslmode=disable"
billing_database_url="postgres://control_lifecycle:${postgres_password}@${postgres_container}:5432/billing_service?sslmode=disable"

org_credentials="$(printf '%s' \
  '[{"principal":"velion-gateway","audience":"org-core","token":"'"$org_gateway_token"'","scopes":["org:read:self","org:provision:self","org:settings:write:self","org:onboarding:write:self"]},{"principal":"auth-core","audience":"org-core","token":"'"$org_auth_token"'","scopes":["org:projection:write:any","org:projection:delete:any"]}]')"
billing_credentials="$(printf '%s' \
  '[{"principal":"velion-gateway","audience":"billing-core","token":"'"$billing_gateway_token"'","scopes":["billing:account:read:self","billing:entitlement:read:self","billing:quota:read:self","billing:checkout:create:self","billing:checkout:confirm:self"]},{"principal":"auth-core","audience":"billing-core","token":"'"$billing_auth_token"'","scopes":["billing:organization:deactivate:any"]},{"principal":"lifecycle-writer","audience":"billing-core","token":"'"$billing_writer_token"'","scopes":["billing:account:write:any"]}]')"
printf '%s' \
  '[{"credentialId":"velion-gateway-primary","principal":"velion-gateway","audience":"auth-core","token":"'"$auth_grpc_gateway_token"'","scopes":["auth:token:validate","auth:user:read"]}]' \
  >"$auth_key_dir/grpc-credentials.json"
printf '%s' \
  '[{"credentialId":"control-lifecycle-nats-auth","principal":"control-lifecycle","audience":"auth-core-internal","token":"'"$auth_internal_nats_token"'","scopes":["nats:authenticate"]}]' \
  >"$auth_key_dir/internal-credentials.json"
printf '%s' \
  '{"credentialId":"auth-core-control-lifecycle","principal":"auth-core","audience":"user-core-grpc","token":"'"$user_grpc_client_token"'"}' \
  >"$auth_key_dir/user-grpc-client-credential.json"
chmod 444 \
  "$auth_key_dir/grpc-credentials.json" \
  "$auth_key_dir/internal-credentials.json" \
  "$auth_key_dir/user-grpc-client-credential.json"

docker run --detach \
  --name "$org_container" \
  --network "$network" \
  --env "DATABASE_URL=$org_database_url" \
  --env DB_USER=control_lifecycle \
  --env HTTP_PORT=3013 \
  --env GRPC_PORT=9090 \
  --env METRICS_PORT=9091 \
  --env "NATS_URL=nats://${nats_container}:4222" \
  --env NATS_USER=control-lifecycle \
  --env "NATS_PASSWORD=$nats_password" \
  --env DRAGONFLY_ENABLED=false \
  --env "ORG_CORE_SERVICE_CREDENTIALS=$org_credentials" \
  --env "USER_CORE_SERVICE_TOKEN=$billing_org_client_token" \
  "$org_image" >/dev/null

docker run --detach \
  --name "$billing_container" \
  --network "$network" \
  --env "DATABASE_URL=$billing_database_url" \
  --env DB_USER=control_lifecycle \
  --env HTTP_PORT=3014 \
  --env GRPC_PORT=50013 \
  --env METRICS_PORT=9092 \
  --env "NATS_URL=nats://${nats_container}:4222" \
  --env NATS_USER=control-lifecycle \
  --env "NATS_PASSWORD=$nats_password" \
  --env DRAGONFLY_ENABLED=false \
  --env PAYMENT_PROVIDER=stripe \
  --env "ORG_CORE_SERVICE_TOKEN=$billing_org_client_token" \
  --env "BILLING_CORE_SERVICE_CREDENTIALS=$billing_credentials" \
  "$billing_image" >/dev/null

wait_for_health() {
  container="$1"
  url="$2"
  for _ in $(seq 1 60); do
    if docker run --rm \
      --network "$network" \
      --entrypoint node "$driver_image" \
      -e "fetch('$url').then(r => { if (!r.ok) process.exit(1) }).catch(() => process.exit(1))" \
      >/dev/null 2>&1; then
      return 0
    fi
    if [ "$(docker inspect -f '{{.State.Running}}' "$container" 2>/dev/null || true)" != "true" ]; then
      echo "$container exited before becoming healthy" >&2
      emit_bounded_container_logs "$container" >&2
      return 1
    fi
    sleep 1
  done
  echo "$container health check timed out" >&2
  emit_bounded_container_logs "$container" >&2
  return 1
}

wait_for_auth_jwks() {
  for _ in $(seq 1 90); do
    if docker exec "$auth_container" node -e \
      "fetch('http://127.0.0.1:3011/api/convex-auth/jwks').then(async response => { if (!response.ok) process.exit(1); const body = await response.json(); if (!body || !Array.isArray(body.keys) || body.keys.length !== 1 || body.keys[0].kty !== 'RSA' || body.keys[0].alg !== 'RS256' || typeof body.keys[0].kid !== 'string' || typeof body.keys[0].n !== 'string' || typeof body.keys[0].e !== 'string') process.exit(1); }).catch(() => process.exit(1))" \
      >/dev/null 2>&1; then
      return 0
    fi
    if [ "$(docker inspect -f '{{.State.Running}}' "$auth_container" 2>/dev/null || true)" != "true" ]; then
      echo "$auth_container exited before JWKS readiness" >&2
      emit_bounded_container_logs "$auth_container" >&2
      return 1
    fi
    sleep 1
  done
  echo "$auth_container JWKS readiness timed out" >&2
  emit_bounded_container_logs "$auth_container" >&2
  return 1
}

run_driver_phase() {
  phase="$1"
  docker run --rm \
    --name "${prefix}-driver-${phase}" \
    --network "$network" \
    --env NODE_ENV=test \
    --env "BETTER_AUTH_SECRET=$better_auth_secret" \
    --env "RESEND_API_KEY=$resend_api_key" \
    --env RESEND_FROM_EMAIL=fixture@container.invalid \
    --env RESEND_FROM_NAME=Control-Lifecycle-Fixture \
    --env BETTER_AUTH_URL=http://localhost:3011 \
    --env FRONTEND_URL=http://localhost:3000 \
    --env ORGANIZATION_ENABLED=true \
    --env ORG_REQUIRE_EMAIL_VERIFICATION=false \
    --env BEARER_TOKEN_ENABLED=true \
    --env RATE_LIMIT_ENABLED=false \
    --env SESSION_COOKIE_CACHE_ENABLED=false \
    --env PASSKEY_RP_ID=localhost \
    --env PASSKEY_ORIGIN=http://localhost:3000 \
    --env "DRAGONFLY_URL=redis://${dragonfly_container}:6379" \
    --env "DATABASE_URL=$auth_database_url" \
    --env DB_USER=control_lifecycle \
    --env "CONTROL_LIFECYCLE_PHASE=$phase" \
    --env "CONTROL_LIFECYCLE_FIXTURE_ID=$fixture_id" \
    --env "CONTROL_LIFECYCLE_POSTGRES_HOST=$postgres_container" \
    --env "CONTROL_LIFECYCLE_ORG_DATABASE_URL=$org_database_url" \
    --env "CONTROL_LIFECYCLE_BILLING_DATABASE_URL=$billing_database_url" \
    --env "CONTROL_LIFECYCLE_BILLING_WRITER_TOKEN=$billing_writer_token" \
    --env "ORG_SERVICE_URL=http://${org_container}:3013" \
    --env "ORG_CORE_SERVICE_TOKEN=$org_auth_token" \
    --env "BILLING_CORE_URL=http://${billing_container}:3014" \
    --env "BILLING_CORE_SERVICE_TOKEN=$billing_auth_token" \
    "$driver_image"
}

wait_for_health "$org_container" "http://${org_container}:3013/health"
wait_for_health "$billing_container" "http://${billing_container}:3014/health"

echo "[startup] production-mode Auth image and stable JWKS readiness"
docker run --detach \
  --name "$auth_container" \
  --network "$network" \
  --volume "${auth_key_dir}:/run/control-auth-keys:ro" \
  --env NODE_ENV=production \
  --env PORT=3011 \
  --env GRPC_PORT=50011 \
  --env "DATABASE_URL=$auth_database_url" \
  --env DB_USER=control_lifecycle \
  --env "DRAGONFLY_URL=redis://${dragonfly_container}:6379" \
  --env "NATS_URL=nats://${nats_container}:4222" \
  --env NATS_USER=control-lifecycle \
  --env "NATS_PASSWORD=$nats_password" \
  --env "BETTER_AUTH_SECRET=$better_auth_secret" \
  --env "RESEND_API_KEY=$resend_api_key" \
  --env RESEND_FROM_EMAIL=fixture@container.invalid \
  --env RESEND_FROM_NAME=Control-Lifecycle-Fixture \
  --env BETTER_AUTH_URL=https://control-lifecycle.invalid \
  --env FRONTEND_URL=https://control-lifecycle.invalid \
  --env BETTER_AUTH_TRUSTED_ORIGINS=https://control-lifecycle.invalid \
  --env RATE_LIMIT_ENABLED=true \
  --env ORGANIZATION_ENABLED=true \
  --env ORG_REQUIRE_EMAIL_VERIFICATION=false \
  --env BEARER_TOKEN_ENABLED=true \
  --env SESSION_COOKIE_CACHE_ENABLED=false \
  --env PASSKEY_RP_ID=control-lifecycle.invalid \
  --env PASSKEY_ORIGIN=https://control-lifecycle.invalid \
  --env "ORG_CORE_URL=http://${org_container}:3013" \
  --env "ORG_SERVICE_URL=http://${org_container}:3013" \
  --env "ORG_CORE_SERVICE_TOKEN=$org_auth_token" \
  --env "BILLING_CORE_URL=http://${billing_container}:3014" \
  --env "BILLING_CORE_SERVICE_TOKEN=$billing_auth_token" \
  --env "USER_CORE_SERVICE_TOKEN=$user_core_auth_token" \
  --env "USER_CORE_MEMBERSHIP_SERVICE_TOKEN=$membership_authority_token" \
  --env "APPLICATION_RECONCILER_AUTH_TOKEN=$application_reconciler_token" \
  --env AUTH_GRPC_SERVICE_CREDENTIALS_FILE=/run/control-auth-keys/grpc-credentials.json \
  --env AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE=/run/control-auth-keys/internal-credentials.json \
  --env USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE=/run/control-auth-keys/user-grpc-client-credential.json \
  --env USER_CORE_GRPC_TLS_CA_FILE=/run/control-auth-keys/user-grpc-ca.pem \
  --env USER_SERVICE_GRPC_URL=user-core:50012 \
  --env CONVEX_AUTH_PRIVATE_KEY_FILE=/run/control-auth-keys/private.pem \
  --env CONVEX_AUTH_PUBLIC_KEY_FILE=/run/control-auth-keys/public.pem \
  "$auth_image" >/dev/null
wait_for_auth_jwks
docker stop "$auth_container" >/dev/null

echo "[1/4] verified invitation acceptance -> Auth outbox -> Org convergence"
run_driver_phase seed-and-converge

echo "[2/4] canonical membership retry, role ordering, removal, and evidence"
run_driver_phase membership-lifecycle

echo "[3/4] Auth deletion with Billing unavailable and durable Org checkpoint"
docker stop "$billing_container" >/dev/null
CONTROL_LIFECYCLE_PHASE=partial-delete run_driver_phase partial-delete

echo "[4/4] Billing restart, Auth retry, tombstones, and delayed resurrection rejection"
docker start "$billing_container" >/dev/null
wait_for_health "$billing_container" "http://${billing_container}:3014/health"
run_driver_phase retry-delete

echo "isolated container-to-container Control lifecycle E2E passed"
