#!/usr/bin/env bash
set -Eeuo pipefail

TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$TEST_DIR/../bootstrap_runtime_environment.sh"
REPO_ROOT="$(cd "$TEST_DIR/../.." && pwd)"
FIXTURE="$(mktemp -d)"
trap 'rm -rf "$FIXTURE"' EXIT

passes=0
failures=0

pass() { printf 'ok - %s\n' "$1"; passes=$((passes + 1)); }
fail() { printf 'not ok - %s\n' "$1" >&2; failures=$((failures + 1)); }

assert_equal() {
  local description="$1" expected="$2" actual="$3"
  if [[ "$expected" == "$actual" ]]; then pass "$description"; else fail "$description"; fi
}

assert_compose_required_vars() {
  local description="$1" compose_file="$2" fixture_env="$3" key value
  local missing=()
  while IFS= read -r key; do
    [[ -n "$key" ]] || continue
    value="$(value_of "$fixture_env" "$key")"
    [[ -n "$value" ]] || value="$(value_of "$FIXTURE/.env" "$key")"
    [[ -n "$value" ]] || missing+=("$key")
  done < <(grep -oE '\$\{[A-Z][A-Z0-9_]*:\?' "$compose_file" | sed -E 's/^\$\{([^:]+):\?$/\1/' | sort -u)
  if (( ${#missing[@]} == 0 )); then
    pass "$description"
  else
    printf 'missing required vars for %s: %s\n' "$description" "${missing[*]}" >&2
    fail "$description"
  fi
}

value_of() {
  local file="$1" key="$2"
  awk -v key="$key" 'index($0, key "=") == 1 { print substr($0, length(key) + 2); exit }' "$file"
}

for relative in \
  '.env' \
  'apps/Data Plane v2/.env' \
  'apps/Control Plane/.env' \
  'apps/Ingestion Plane/.env' \
  'apps/Model Plane/deploy/.env' \
  'apps/Application Plane/.env' \
  'apps/Frontend Plane/velionv3/.env'; do
  mkdir -p "$FIXTURE/$(dirname "$relative")"
  printf '# fixture\n' > "$FIXTURE/$relative"
done

printf 'INTERNAL_API_KEY=%s\n' "$(openssl rand -hex 32)" >> "$FIXTURE/apps/Control Plane/.env"
printf 'QUARRY_RUNTIME_AUTH_TOKEN=%s\n' "$(openssl rand -hex 32)" >> "$FIXTURE/apps/Ingestion Plane/.env"
printf 'APPLICATION_PLANE_DB_PASSWORD=application-plane-db-secret\n' >> "$FIXTURE/apps/Application Plane/.env"

before_hash="$(shasum -a 256 "$FIXTURE/apps/Control Plane/.env" | awk '{print $1}')"
CORE_ROOT_OVERRIDE="$FIXTURE" "$SCRIPT" --dry-run >/dev/null
after_hash="$(shasum -a 256 "$FIXTURE/apps/Control Plane/.env" | awk '{print $1}')"
assert_equal "dry-run does not modify dotenv files" "$before_hash" "$after_hash"

CORE_ROOT_OVERRIDE="$FIXTURE" "$SCRIPT" >/dev/null

control="$FIXTURE/apps/Control Plane/.env"
data="$FIXTURE/apps/Data Plane v2/.env"
ingestion="$FIXTURE/apps/Ingestion Plane/.env"
model="$FIXTURE/apps/Model Plane/deploy/.env"
application="$FIXTURE/apps/Application Plane/.env"
frontend="$FIXTURE/apps/Frontend Plane/velionv3/.env"

assert_equal "fleet internal key is synchronized to Data" "$(value_of "$control" INTERNAL_API_KEY)" "$(value_of "$data" INTERNAL_API_KEY)"
assert_equal "fleet internal key is synchronized to Frontend" "$(value_of "$control" INTERNAL_API_KEY)" "$(value_of "$frontend" INTERNAL_API_KEY)"
assert_equal "gateway session credential matches Control" "$(value_of "$control" SESSION_CORE_SERVICE_TOKEN)" "$(value_of "$frontend" SESSION_CORE_SERVICE_TOKEN)"
assert_equal "gateway user credential matches Control" "$(value_of "$control" USER_CORE_GATEWAY_TOKEN)" "$(value_of "$frontend" USER_CORE_GATEWAY_TOKEN)"
assert_equal "Application consumes Model NATS credential" "$(value_of "$model" MODEL_NATS_TOKEN)" "$(value_of "$application" MODEL_PLANE_NATS_TOKEN)"

if [[ "$(printf '%s' "$(value_of "$ingestion" INTEGRATION_CREDENTIALS_ENCRYPTION_KEY)" | openssl base64 -d -A | wc -c | tr -d ' ')" == "32" ]]; then
  pass "integration credential encryption key decodes to 32 bytes"
else
  fail "integration credential encryption key decodes to 32 bytes"
fi

assert_compose_required_vars "root Compose required variables are bootstrapped" "$REPO_ROOT/docker-compose.yml" "$FIXTURE/.env"
assert_compose_required_vars "Data Compose required variables are bootstrapped" "$REPO_ROOT/apps/Data Plane v2/docker-compose.yml" "$data"
assert_compose_required_vars "Control Compose required variables are bootstrapped" "$REPO_ROOT/apps/Control Plane/docker-compose.yml" "$control"
assert_compose_required_vars "Ingestion Compose required variables are bootstrapped" "$REPO_ROOT/apps/Ingestion Plane/docker-compose.yml" "$ingestion"
assert_compose_required_vars "Model Compose required variables are bootstrapped" "$REPO_ROOT/apps/Model Plane/deploy/docker-compose.yml" "$model"
assert_compose_required_vars "Application Compose required variables are bootstrapped" "$REPO_ROOT/apps/Application Plane/docker-compose.yml" "$application"
assert_compose_required_vars "Frontend Compose required variables are bootstrapped" "$REPO_ROOT/apps/Frontend Plane/velionv3/docker-compose.yml" "$frontend"

data_dockerignore="$REPO_ROOT/apps/Data Plane v2/.dockerignore"
if grep -Fxq '.secrets/' "$data_dockerignore" &&
  grep -Fxq '**/.secrets/' "$data_dockerignore" &&
  grep -Fxq '**/*.pem' "$data_dockerignore" &&
  grep -Fxq '**/*.key' "$data_dockerignore"; then
  pass "Data build context excludes private event-signing keys"
else
  fail "Data build context excludes private event-signing keys"
fi

principal_json="$(value_of "$control" PLANE_SERVICE_PRINCIPALS_JSON)"
if jq -e '
  .["retrieval-engine"].allowAnyOrg == true and
  .["retrieval-engine"].scopes == ["data:authorization:decide"] and
  .["imports-core"].scopes == ["documents:write"] and
  .["shipping-core"].scopes == ["documents:write"] and
  .["model-execution"].scopes == ["integration:read", "integration:write", "shipping:read", "shipping:write"] and
  .["quarry-edge"].audiences == ["data-plane", "model-gateway"] and
  .["quarry-edge"].scopes == ["documents:write", "data:read", "models:invoke"] and
  .["model-gateway"].scopes == ["scrape:read", "search:read"] and
  .["execution-core"].scopes == ["browser:execute", "search:read", "extract:read", "scrape:write"] and
  .["integration-corev2"].scopes == ["documents:write"] and
  .["finspo-core"].scopes == ["documents:write"]
' <<<"$principal_json" >/dev/null; then
  pass "service-principal registry has bounded cross-plane policies"
else
  fail "service-principal registry has bounded cross-plane policies"
fi

if [[ "$(value_of "$application" APPLICATION_PLANE_DB_PASSWORD)" != "application-plane-db-secret" ]]; then
  pass "known placeholder secrets are replaced"
else
  fail "known placeholder secrets are replaced"
fi

for domain in documents index embedding wiki; do
  private="$FIXTURE/apps/Data Plane v2/.secrets/event-keys/$domain-events.pem"
  public="$FIXTURE/apps/Data Plane v2/.secrets/event-keys/$domain-events.pub"
  if [[ -s "$private" && -s "$public" && "$(stat -f '%Lp' "$private")" == "600" && "$(stat -f '%Lp' "$public")" == "644" ]]; then
    pass "$domain event-signing keypair exists with safe permissions"
  else
    fail "$domain event-signing keypair exists with safe permissions"
  fi
done

first_hashes="$(find "$FIXTURE" -type f -exec shasum -a 256 {} + | sort)"
CORE_ROOT_OVERRIDE="$FIXTURE" "$SCRIPT" >/dev/null
second_hashes="$(find "$FIXTURE" -type f -exec shasum -a 256 {} + | sort)"
assert_equal "bootstrap is idempotent" "$first_hashes" "$second_hashes"

old_data_nats="$(value_of "$data" DATAPLANE_NATS_TOKEN)"
old_model_nats="$(value_of "$model" MODEL_NATS_TOKEN)"
CORE_ROOT_OVERRIDE="$FIXTURE" "$SCRIPT" --rotate-nats >/dev/null
new_data_nats="$(value_of "$data" DATAPLANE_NATS_TOKEN)"
new_model_nats="$(value_of "$model" MODEL_NATS_TOKEN)"
if [[ -n "$new_data_nats" && "$new_data_nats" != "$old_data_nats" ]]; then
  pass "NATS rotation replaces the Data Plane token"
else
  fail "NATS rotation replaces the Data Plane token"
fi
if [[ -n "$new_model_nats" && "$new_model_nats" != "$old_model_nats" ]]; then
  pass "NATS rotation replaces the Model Plane token"
else
  fail "NATS rotation replaces the Model Plane token"
fi
assert_equal "rotated Model NATS token remains synchronized to Application" "$new_model_nats" "$(value_of "$application" MODEL_PLANE_NATS_TOKEN)"

for file in "$FIXTURE/.env" "$data" "$control" "$ingestion" "$model" "$application" "$frontend"; do
  if [[ "$(stat -f '%Lp' "$file")" == "600" ]]; then
    pass "$(basename "$(dirname "$file")")/$(basename "$file") is mode 600"
  else
    fail "$file is mode 600"
  fi
done

printf '\n%d passed, %d failed\n' "$passes" "$failures"
(( failures == 0 ))
