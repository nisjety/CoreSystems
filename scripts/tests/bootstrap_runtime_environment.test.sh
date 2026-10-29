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

# Variables provisioned OUTSIDE the central bootstrap, so this test must not
# treat their absence from the bootstrap fixture as a gap.
#
# The fleet is provisioned in two layers: this central bootstrap owns
# cross-plane shared credentials and the Auth Core principal registry, while
# each plane's run-*.sh generates that plane's own NATS/GDPR/DB/service-token
# secrets (see the `required_credentials` arrays and `set_if_missing` calls
# there). The fixture only runs the central bootstrap, so without this the test
# reported ~100 per-plane-runner-owned variables as "missing" — testing a
# contract the system never claimed. This set is parsed from the actual runner
# scripts, so it cannot silently drift from them.
runner_provisioned_vars() {
  {
    # set_if_missing VAR ...
    grep -rhoE 'set_if_missing[[:space:]]+[A-Z][A-Z0-9_]{3,}' "$REPO_ROOT"/apps/*/scripts/run-*.sh 2>/dev/null \
      | grep -oE '[A-Z][A-Z0-9_]{3,}'
    # ensure_secret/ensure_value/ensure_env "$X" VAR  (second all-caps token)
    grep -rhoE 'ensure_(secret|value|env)[^\n]*' "$REPO_ROOT"/apps/*/scripts/run-*.sh 2>/dev/null \
      | grep -oE '[A-Z][A-Z0-9_]{4,}'
    # required_credentials=( ... ) / required_generated=( ... ) array bodies
    awk '/required_(credentials|generated|env|secrets)[[:space:]]*(\+?=)?[[:space:]]*\(/,/\)/' \
      "$REPO_ROOT"/apps/*/scripts/run-*.sh 2>/dev/null | grep -oE '[A-Z][A-Z0-9_]{4,}'
  } | sort -u
}

# Build provenance is supplied by CI or the per-plane runners' set_if_missing,
# never minted by the bootstrap — a git SHA is not a secret to generate.
CI_PROVENANCE_VARS=$'BUILD_DATE\nSOURCE_REVISION'

assert_compose_required_vars() {
  local description="$1" compose_file="$2" fixture_env="$3" key value
  local -a missing=()
  local runner_vars ci_vars
  runner_vars="$(runner_provisioned_vars)"
  ci_vars="$CI_PROVENANCE_VARS"
  while IFS= read -r key; do
    [[ -n "$key" ]] || continue
    value="$(value_of "$fixture_env" "$key")"
    [[ -n "$value" ]] || value="$(value_of "$FIXTURE/.env" "$key")"
    # Satisfied if the central bootstrap produced it, a per-plane runner owns
    # it, or it is CI-supplied provenance.
    if [[ -z "$value" ]] \
      && ! grep -qxF "$key" <<<"$runner_vars" \
      && ! grep -qxF "$key" <<<"$ci_vars"; then
      missing+=("$key")
    fi
  done < <(grep -oE '\$\{[A-Z][A-Z0-9_]*:\?' "$compose_file" | sed -E 's/^\$\{([^:]+):\?$/\1/' | sort -u)
  if (( ${#missing[@]} == 0 )); then
    pass "$description"
  else
    printf 'unprovisioned required vars for %s: %s\n' "$description" "${missing[*]}" >&2
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
  'apps/Frontend Plane/verevonv3/.env'; do
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
frontend="$FIXTURE/apps/Frontend Plane/verevonv3/.env"

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
assert_compose_required_vars "Frontend Compose required variables are bootstrapped" "$REPO_ROOT/apps/Frontend Plane/verevonv3/docker-compose.yml" "$frontend"

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

# Assert the INVARIANTS Auth Core actually enforces, not a frozen scope list.
#
# This used to pin exact scope arrays for a registry that included shipping-core
# and integration-corev2 — neither of which the live fleet carries any more — so
# it had become a test of a superseded contract. Worse, an exact-array assertion
# fails on a harmless reordering while saying nothing about the two rules that
# genuinely take the fleet down: Auth Core's parseRegistry throws on the FIRST
# bad entry and runs PER REQUEST, so one malformed principal 503s every mint
# while /health stays green.
if jq -e '
  # Every entry must declare per-audience scoping and retention.
  (to_entries | all(.value | has("scopesByAudience") and has("retentionByAudience")))
  # The union of scopesByAudience must EXACTLY equal the flat scopes array.
  and (to_entries | all(
        (.value.scopesByAudience | to_entries | map(.value) | add | unique)
        == (.value.scopes | unique)))
  # scopesByAudience and retentionByAudience must key exactly the audiences.
  and (to_entries | all(
        (.value.scopesByAudience | keys | sort) == (.value.audiences | sort)))
  and (to_entries | all(
        (.value.retentionByAudience | keys | sort) == (.value.audiences | sort)))
  # Retention is a closed vocabulary; anything else invalidates the entry.
  and (to_entries | all(
        .value.retentionByAudience | to_entries | all(
          .value == "zdr" or .value == "persistent")))
' <<<"$principal_json" >/dev/null; then
  pass "service-principal registry satisfies Auth Core's per-audience invariants"
else
  fail "service-principal registry satisfies Auth Core's per-audience invariants"
fi

# No two principals may share a credential. conversation-core was aliased to
# integration-corev2's key, so one leaked secret granted both identities —
# confirmed live by SHA-256 fingerprint before it was split.
principal_count="$(jq -r 'length' <<<"$principal_json")"
distinct_credentials="$(jq -r '[.[].credential] | unique | length' <<<"$principal_json")"
if [[ "$principal_count" == "$distinct_credentials" ]]; then
  pass "every service principal has its own credential ($principal_count principals)"
else
  fail "service principals share credentials ($principal_count principals, $distinct_credentials distinct secrets)"
fi

# Scope bounds that must not silently widen. Checked as membership, so adding a
# NEW audience to a principal does not fail this, but granting one of these
# identities something outside its remit does.
if jq -e '
  (.["imports-core"].scopes == ["documents:write"])
  and (.["finspo-core"].scopes == ["documents:write"])
  and (.["retrieval-engine"].allowAnyOrg == true)
  and (.["graph-index"].scopes == ["inference:invoke"])
  and (.["embedding-engine"].scopes == ["inference:invoke"])
  # conversation-core may write provider replies but must never touch documents.
  and (.["conversation-core"].scopes | index("documents:write") | not)
  # A ZDR-audience grant must not license retention downstream.
  and (.["conversation-core"].retentionByAudience.ingestion == "zdr")
  and (.["model-execution"].retentionByAudience.ingestion == "zdr")
' <<<"$principal_json" >/dev/null; then
  pass "service-principal registry keeps cross-plane scopes bounded"
else
  fail "service-principal registry keeps cross-plane scopes bounded"
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
