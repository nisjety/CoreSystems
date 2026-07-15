#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
preflight="$root/scripts/validate-release-credentials.sh"

secure_render="$(docker compose \
  -f "$root/docker-compose.yml" \
  -f "$root/docker-compose.production.yml" \
  config --no-interpolate)"
if grep -Eq 'control-shared-legacy-bridge|VELION_(LEGACY_)?NATS_TOKEN' \
  <<<"$secure_render"; then
  printf 'secure production render contains the legacy bridge or token\n' >&2
  exit 1
fi
legacy_render="$(docker compose \
  -f "$root/docker-compose.yml" \
  -f "$root/docker-compose.legacy-bridge.yml" \
  config --no-interpolate)"
grep -q 'control-shared-legacy-bridge' <<<"$legacy_render"
grep -q 'VELION_LEGACY_NATS_TOKEN' <<<"$legacy_render"

names=($(sed -n '/^credential_names=(/,/^)/p' "$preflight" |
  tr ' ' '\n' |
  sed -n 's/^\([A-Z][A-Z0-9_]*\)$/\1/p'))
names+=(VELION_NATS_TOKEN)

for required_model_credential in \
  MODEL_NATS_RUNTIME_PASSWORD \
  MODEL_GATEWAY_NATS_PASSWORD \
  MODEL_SESSION_CORE_NATS_PASSWORD \
  APPLICATION_CONVEX_MODEL_NATS_PASSWORD \
  APPLICATION_INSIGHT_MODEL_NATS_PASSWORD \
  AUDIT_MODEL_NATS_PASSWORD \
  MODEL_NATS_PROVISIONER_PASSWORD; do
  if ! printf '%s\n' "${names[@]}" | grep -qx "$required_model_credential"; then
    printf 'credential preflight omits configured Model NATS credential %s\n' \
      "$required_model_credential" >&2
    exit 1
  fi
  if ! grep -q "^${required_model_credential}=" "$root/.env.example"; then
    printf 'release environment template omits configured Model NATS credential %s\n' \
      "$required_model_credential" >&2
    exit 1
  fi
done

for required_control_credential in \
  AUTH_USER_CORE_GRPC_SERVICE_TOKEN \
  USER_AUTH_INTERNAL_SERVICE_TOKEN \
  QUARRY_AUTH_INTERNAL_SERVICE_TOKEN; do
  if ! printf '%s\n' "${names[@]}" | grep -qx "$required_control_credential"; then
    printf 'credential preflight omits scoped Control credential %s\n' \
      "$required_control_credential" >&2
    exit 1
  fi
done

base_env=()
retired_env=()
for index in "${!names[@]}"; do
  assignment="${names[$index]}=release-${index}-0123456789abcdef0123456789abcdef"
  base_env+=("$assignment")
  if [[ "${names[$index]}" != "VELION_NATS_TOKEN" ]]; then
    retired_env+=("$assignment")
  fi
done

runtime_dir="$(mktemp -d "${TMPDIR:-/tmp}/control-release-preflight.XXXXXX")"
trap 'rm -rf "$runtime_dir"' EXIT INT TERM
gateway_auth_token=''
retrieval_auth_token=''
auth_user_grpc_token=''
user_auth_internal_token=''
quarry_auth_internal_token=''
for assignment in "${base_env[@]}"; do
  case "$assignment" in
    GATEWAY_AUTH_GRPC_SERVICE_TOKEN=*) gateway_auth_token="${assignment#*=}" ;;
    RETRIEVAL_AUTH_GRPC_SERVICE_TOKEN=*) retrieval_auth_token="${assignment#*=}" ;;
    AUTH_USER_CORE_GRPC_SERVICE_TOKEN=*) auth_user_grpc_token="${assignment#*=}" ;;
    USER_AUTH_INTERNAL_SERVICE_TOKEN=*) user_auth_internal_token="${assignment#*=}" ;;
    QUARRY_AUTH_INTERNAL_SERVICE_TOKEN=*) quarry_auth_internal_token="${assignment#*=}" ;;
  esac
done
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$runtime_dir/auth-private.pem" >/dev/null 2>&1
openssl pkey -in "$runtime_dir/auth-private.pem" -pubout \
  -out "$runtime_dir/auth-public.pem" >/dev/null 2>&1
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$runtime_dir/user-grpc-ca-key.pem" >/dev/null 2>&1
openssl req -x509 -new -key "$runtime_dir/user-grpc-ca-key.pem" \
  -sha256 -days 1 -subj '/CN=Control Plane User Core test CA' \
  -addext 'basicConstraints=critical,CA:TRUE' \
  -addext 'keyUsage=critical,keyCertSign,cRLSign' \
  -out "$runtime_dir/user-grpc-ca.pem" >/dev/null 2>&1
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$runtime_dir/user-grpc-server-key.pem" >/dev/null 2>&1
openssl req -new -key "$runtime_dir/user-grpc-server-key.pem" \
  -sha256 -subj '/CN=user-core' \
  -addext 'subjectAltName=DNS:user-core' \
  -addext 'extendedKeyUsage=serverAuth' \
  -out "$runtime_dir/user-grpc-server.csr" >/dev/null 2>&1
openssl x509 -req -in "$runtime_dir/user-grpc-server.csr" \
  -CA "$runtime_dir/user-grpc-ca.pem" \
  -CAkey "$runtime_dir/user-grpc-ca-key.pem" \
  -CAcreateserial -sha256 -days 1 -copy_extensions copy \
  -out "$runtime_dir/user-grpc-server.pem" >/dev/null 2>&1
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out "$runtime_dir/wrong-host-key.pem" >/dev/null 2>&1
openssl req -new -key "$runtime_dir/wrong-host-key.pem" \
  -sha256 -subj '/CN=attacker.invalid' \
  -addext 'subjectAltName=DNS:attacker.invalid' \
  -addext 'extendedKeyUsage=serverAuth' \
  -out "$runtime_dir/wrong-host.csr" >/dev/null 2>&1
openssl x509 -req -in "$runtime_dir/wrong-host.csr" \
  -CA "$runtime_dir/user-grpc-ca.pem" \
  -CAkey "$runtime_dir/user-grpc-ca-key.pem" \
  -CAcreateserial -sha256 -days 1 -copy_extensions copy \
  -out "$runtime_dir/wrong-host.pem" >/dev/null 2>&1
printf '%s' \
  '[{"credentialId":"velion-gateway-primary","principal":"velion-gateway","audience":"auth-core","token":"'"$gateway_auth_token"'","scopes":["auth:token:validate","auth:user:read"]},{"credentialId":"retrieval-engine-primary","principal":"retrieval-engine","audience":"auth-core","token":"'"$retrieval_auth_token"'","scopes":["auth:token:validate"]}]' \
  >"$runtime_dir/auth-grpc-credentials.json"
printf '%s' \
  '[{"credentialId":"user-core-primary","principal":"user-core","audience":"auth-core-internal","token":"'"$user_auth_internal_token"'","scopes":["oauth:token:read","oauth:token:refresh","nats:authenticate","auth:admin"]},{"credentialId":"quarry-control-primary","principal":"quarry-control","audience":"auth-core-internal","token":"'"$quarry_auth_internal_token"'","scopes":["agent:provision"]}]' \
  >"$runtime_dir/auth-internal-credentials.json"
printf '%s' \
  '{"credentialId":"auth-core-primary","principal":"auth-core","audience":"user-core-grpc","token":"'"$auth_user_grpc_token"'"}' \
  >"$runtime_dir/user-grpc-client-credential.json"
printf '%s' \
  '[{"credentialId":"auth-core-primary","principal":"auth-core","audience":"user-core-grpc","token":"'"$auth_user_grpc_token"'","methods":["/user.v1.UserService/CreateUser","/user.v1.UserService/UpdateUser","/user.v1.UserService/GetUserByEmail","/user.v1.UserService/CreateSession","/user.v1.UserService/GetUser","/user.v1.UserService/DeleteUser","/user.v1.UserService/HealthCheck"]}]' \
  >"$runtime_dir/user-grpc-service-credentials.json"
printf '%s' \
  '{"credentialId":"user-core-primary","principal":"user-core","audience":"auth-core-internal","token":"'"$user_auth_internal_token"'"}' \
  >"$runtime_dir/user-auth-internal-client-credential.json"
printf '%s' \
  '{"credentialId":"auth-core-primary","principal":"auth-core","audience":"user-core-grpc","token":"'"$retrieval_auth_token"'"}' \
  >"$runtime_dir/mismatched-user-grpc-client-credential.json"
printf '%s' \
  '[{"credentialId":"user-core-primary","principal":"user-core","audience":"auth-core-internal","token":"'"$user_auth_internal_token"'","scopes":["nats:authenticate"]}]' \
  >"$runtime_dir/incomplete-auth-internal-credentials.json"
printf '%s' \
  '[{"credentialId":"auth-core-primary","principal":"auth-core","audience":"user-core-grpc","token":"'"$auth_user_grpc_token"'","methods":["*"]}]' \
  >"$runtime_dir/invalid-user-grpc-service-credentials.json"
printf '%s' \
  '{"credentialId":"user-core-primary","principal":"attacker","audience":"auth-core-internal","token":"'"$user_auth_internal_token"'"}' \
  >"$runtime_dir/invalid-user-auth-client-credential.json"
chmod 600 \
  "$runtime_dir/auth-private.pem" \
  "$runtime_dir/user-grpc-ca-key.pem" \
  "$runtime_dir/user-grpc-server-key.pem" \
  "$runtime_dir/wrong-host-key.pem" \
  "$runtime_dir/auth-grpc-credentials.json" \
  "$runtime_dir/auth-internal-credentials.json" \
  "$runtime_dir/user-grpc-client-credential.json" \
  "$runtime_dir/user-grpc-service-credentials.json" \
  "$runtime_dir/user-auth-internal-client-credential.json" \
  "$runtime_dir/mismatched-user-grpc-client-credential.json" \
  "$runtime_dir/incomplete-auth-internal-credentials.json" \
  "$runtime_dir/invalid-user-grpc-service-credentials.json" \
  "$runtime_dir/invalid-user-auth-client-credential.json"
chmod 644 "$runtime_dir/auth-public.pem"
chmod 644 \
  "$runtime_dir/user-grpc-ca.pem" \
  "$runtime_dir/user-grpc-server.pem" \
  "$runtime_dir/wrong-host.pem"
base_env+=(
  "AUTH_GRPC_SERVICE_CREDENTIALS_FILE=$runtime_dir/auth-grpc-credentials.json"
  "AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE=$runtime_dir/auth-internal-credentials.json"
  "USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE=$runtime_dir/user-grpc-client-credential.json"
  "USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE=$runtime_dir/user-grpc-service-credentials.json"
  "USER_CORE_GRPC_TLS_CA_FILE=$runtime_dir/user-grpc-ca.pem"
  "USER_CORE_GRPC_TLS_CERT_FILE=$runtime_dir/user-grpc-server.pem"
  "USER_CORE_GRPC_TLS_KEY_FILE=$runtime_dir/user-grpc-server-key.pem"
  "USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE=$runtime_dir/user-auth-internal-client-credential.json"
  "CONVEX_AUTH_PRIVATE_KEY_FILE=$runtime_dir/auth-private.pem"
  "CONVEX_AUTH_PUBLIC_KEY_FILE=$runtime_dir/auth-public.pem"
)
retired_env+=(
  "AUTH_GRPC_SERVICE_CREDENTIALS_FILE=$runtime_dir/auth-grpc-credentials.json"
  "AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE=$runtime_dir/auth-internal-credentials.json"
  "USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE=$runtime_dir/user-grpc-client-credential.json"
  "USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE=$runtime_dir/user-grpc-service-credentials.json"
  "USER_CORE_GRPC_TLS_CA_FILE=$runtime_dir/user-grpc-ca.pem"
  "USER_CORE_GRPC_TLS_CERT_FILE=$runtime_dir/user-grpc-server.pem"
  "USER_CORE_GRPC_TLS_KEY_FILE=$runtime_dir/user-grpc-server-key.pem"
  "USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE=$runtime_dir/user-auth-internal-client-credential.json"
  "CONVEX_AUTH_PRIVATE_KEY_FILE=$runtime_dir/auth-private.pem"
  "CONVEX_AUTH_PUBLIC_KEY_FILE=$runtime_dir/auth-public.pem"
)

env "${base_env[@]}" "$preflight" >/dev/null
env "${base_env[@]}" REQUIRE_LEGACY_BRIDGE=1 "$preflight" >/dev/null
env "${retired_env[@]}" REQUIRE_LEGACY_BRIDGE=0 "$preflight" >/dev/null

expect_failure() {
  local expected_name="$1"
  shift
  local output
  if output="$(env "${base_env[@]}" "$@" "$preflight" 2>&1)"; then
    printf 'expected credential preflight failure for %s\n' "$expected_name" >&2
    exit 1
  fi
  grep -q "$expected_name" <<<"$output"
}

expect_failure AUTH_NATS_PASSWORD AUTH_NATS_PASSWORD=
expect_failure USER_NATS_PASSWORD USER_NATS_PASSWORD=short
expect_failure ORG_NATS_PASSWORD ORG_NATS_PASSWORD=change-me-0123456789abcdef0123456789abcdef
expect_failure MODEL_NATS_RUNTIME_PASSWORD MODEL_NATS_RUNTIME_PASSWORD=short
expect_failure MODEL_GATEWAY_NATS_PASSWORD MODEL_GATEWAY_NATS_PASSWORD=short
expect_failure AUTH_GRPC_SERVICE_CREDENTIALS_FILE \
  AUTH_GRPC_SERVICE_CREDENTIALS_FILE="$runtime_dir/missing-registry.json"
expect_failure AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE \
  AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE="$runtime_dir/missing-auth-internal-registry.json"
expect_failure USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE \
  USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE="$runtime_dir/missing-user-grpc-client.json"
expect_failure USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE \
  USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE="$runtime_dir/missing-user-grpc-registry.json"
expect_failure USER_CORE_GRPC_TLS_CA_FILE \
  USER_CORE_GRPC_TLS_CA_FILE="$runtime_dir/missing-user-grpc-ca.pem"
expect_failure USER_CORE_GRPC_TLS_CERT_FILE \
  USER_CORE_GRPC_TLS_CERT_FILE="$runtime_dir/auth-public.pem"
expect_failure USER_CORE_GRPC_TLS_KEY_FILE \
  USER_CORE_GRPC_TLS_KEY_FILE="$runtime_dir/auth-private.pem"
expect_failure USER_CORE_GRPC_TLS_CERT_FILE \
  USER_CORE_GRPC_TLS_CERT_FILE="$runtime_dir/wrong-host.pem" \
  USER_CORE_GRPC_TLS_KEY_FILE="$runtime_dir/wrong-host-key.pem"
expect_failure USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE \
  USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE="$runtime_dir/missing-user-auth-client.json"
expect_failure USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE \
  USER_CORE_GRPC_CLIENT_CREDENTIAL_FILE="$runtime_dir/mismatched-user-grpc-client-credential.json"
expect_failure AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE \
  AUTH_INTERNAL_SERVICE_CREDENTIALS_FILE="$runtime_dir/incomplete-auth-internal-credentials.json"
expect_failure USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE \
  USER_CORE_GRPC_SERVICE_CREDENTIALS_FILE="$runtime_dir/invalid-user-grpc-service-credentials.json"
expect_failure USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE \
  USER_AUTH_INTERNAL_CLIENT_CREDENTIAL_FILE="$runtime_dir/invalid-user-auth-client-credential.json"
expect_failure CONVEX_AUTH_PRIVATE_KEY_FILE \
  CONVEX_AUTH_PRIVATE_KEY_FILE="$runtime_dir/missing-private.pem"

reused_new_control='reused-new-control-credential-0123456789abcdef'
expect_failure USER_AUTH_INTERNAL_SERVICE_TOKEN \
  AUTH_USER_CORE_GRPC_SERVICE_TOKEN="$reused_new_control" \
  USER_AUTH_INTERNAL_SERVICE_TOKEN="$reused_new_control"

if env "${retired_env[@]}" REQUIRE_LEGACY_BRIDGE=1 "$preflight" >/dev/null 2>&1; then
  printf 'expected legacy bridge credential failure when its profile is required\n' >&2
  exit 1
fi

sensitive='never-print-this-reused-secret-0123456789abcdef'
output=''
if output="$(env "${base_env[@]}" \
  BILLING_NATS_PASSWORD="$sensitive" \
  SESSION_NATS_PASSWORD="$sensitive" \
  "$preflight" 2>&1)"; then
  printf 'expected reused credential failure\n' >&2
  exit 1
fi
grep -q BILLING_NATS_PASSWORD <<<"$output"
grep -q SESSION_NATS_PASSWORD <<<"$output"
if grep -q "$sensitive" <<<"$output"; then
  printf 'credential value leaked in preflight output\n' >&2
  exit 1
fi

printf 'credential preflight tests passed\n'
