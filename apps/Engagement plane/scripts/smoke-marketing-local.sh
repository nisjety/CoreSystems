#!/usr/bin/env sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
studio_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)
compose_file="$studio_dir/docker-compose.marketing-local.yml"
project_name="marketing-local-smoke-$$"

random_secret() {
  openssl rand -base64 48 | tr -d '\n'
}

MARKETING_LOCAL_INTEGRATION_API_KEY=$(random_secret)
MARKETING_LOCAL_MARKETING_API_KEY=$(random_secret)
MARKETING_LOCAL_CONTROL_API_KEY=$(random_secret)
MARKETING_LOCAL_ORG_TOKEN=$(random_secret)
MARKETING_LOCAL_BILLING_TOKEN=$(random_secret)
MARKETING_LOCAL_AUDIT_TOKEN=$(random_secret)
MARKETING_LOCAL_ENCRYPTION_KEY=$(openssl rand -base64 32 | tr -d '\n')
MARKETING_LOCAL_ATTESTATION_KEYS_JSON=$(node -e 'const c=require("crypto"); const {publicKey}=c.generateKeyPairSync("ed25519"); const raw=Buffer.from(publicKey.export({format:"jwk"}).x,"base64url").toString("base64"); process.stdout.write(JSON.stringify([{issuer:"conversation-core",kid:"marketing-local-smoke",public_key:raw}]));')
INTEGRATION_CORE_HOST_PORT=${INTEGRATION_CORE_HOST_PORT:-33026}
MARKETING_CORE_HOST_PORT=${MARKETING_CORE_HOST_PORT:-33165}
export MARKETING_LOCAL_INTEGRATION_API_KEY MARKETING_LOCAL_MARKETING_API_KEY MARKETING_LOCAL_CONTROL_API_KEY
export MARKETING_LOCAL_ORG_TOKEN MARKETING_LOCAL_BILLING_TOKEN MARKETING_LOCAL_AUDIT_TOKEN
export MARKETING_LOCAL_ENCRYPTION_KEY MARKETING_LOCAL_ATTESTATION_KEYS_JSON INTEGRATION_CORE_HOST_PORT MARKETING_CORE_HOST_PORT

cleanup() {
  docker compose -p "$project_name" -f "$compose_file" down --remove-orphans >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

docker compose -p "$project_name" -f "$compose_file" config --quiet
docker compose -p "$project_name" -f "$compose_file" up --detach --build --wait

printf '%s\n' "Checking Integration Core page..."
curl --fail --silent --show-error "http://127.0.0.1:$INTEGRATION_CORE_HOST_PORT/" | grep -F "<title>Integration Core</title>" >/dev/null
printf '%s\n' "Checking Marketing Core page..."
curl --fail --silent --show-error "http://127.0.0.1:$MARKETING_CORE_HOST_PORT/" | grep -F "<title>Marketing Core</title>" >/dev/null
printf '%s\n' "Checking Integration Core health..."
curl --fail --silent --show-error "http://127.0.0.1:$INTEGRATION_CORE_HOST_PORT/health" | grep -F '"status":"ok"' >/dev/null
printf '%s\n' "Checking Marketing Core health..."
curl --fail --silent --show-error "http://127.0.0.1:$MARKETING_CORE_HOST_PORT/health" | grep -F '"status":"ok"' >/dev/null

printf '%s\n' "Checking selected-site Pulse fails closed without an OAuth connection..."
pulse_url="http://127.0.0.1:$MARKETING_CORE_HOST_PORT/api/v1/marketing/pulse?quarter=Q1&site=https%3A%2F%2Faquatiq.com%2F"
curl --fail --silent --show-error \
  --header "X-Internal-API-Key: $MARKETING_LOCAL_MARKETING_API_KEY" \
  "$pulse_url" | grep -F '"reason":"not_configured"' >/dev/null
unauthorized_status=$(curl --silent --show-error --output /dev/null --write-out '%{http_code}' "$pulse_url")
if [ "$unauthorized_status" != "401" ]; then
  printf '%s\n' "Pulse authorization guard returned HTTP $unauthorized_status" >&2
  exit 1
fi

response_file=$(mktemp)
printf '%s\n' "Checking readiness requires tenant-scoped authorization..."
status=$(curl --silent --show-error --output "$response_file" --write-out '%{http_code}' \
  --request POST \
  --header "X-Internal-API-Key: $MARKETING_LOCAL_INTEGRATION_API_KEY" \
  "http://127.0.0.1:$INTEGRATION_CORE_HOST_PORT/api/v1/connections/not-a-connection/readiness?operation=ads.insights")
if [ "$status" != "403" ]; then
  printf '%s\n' "Readiness authorization guard returned HTTP $status" >&2
  sed -n '1p' "$response_file" >&2
  exit 1
fi
grep -F '"code":"scoped_service_token_required"' "$response_file" >/dev/null
rm -f "$response_file"

printf '%s\n' "Marketing local Docker smoke test passed."
