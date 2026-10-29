#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${1:-http://localhost:3026}"
API_BASE="$BASE_URL/api/v1"

env_file_value() {
  local key="$1"
  [[ -f .env ]] || return 1
  awk -F= -v key="$key" '$1 == key {print substr($0, index($0, "=") + 1)}' .env \
    | tail -n 1 \
    | sed -e 's/^["'\'']//' -e 's/["'\'']$//'
}

if [[ "${2:-}" != "" ]]; then
  INTERNAL_KEY="$2"
elif [[ "${INTERNAL_API_KEY:-}" != "" ]]; then
  INTERNAL_KEY="$INTERNAL_API_KEY"
else
  INTERNAL_KEY="$(env_file_value INTERNAL_API_KEY || true)"
  INTERNAL_KEY="${INTERNAL_KEY:-dev-super-secret-internal-api-key}"
fi

PASS_COUNT=0
FAIL_COUNT=0
STATUS=""
BODY=""

pass() {
  printf 'PASS: %s\n' "$1"
  PASS_COUNT=$((PASS_COUNT + 1))
}

fail() {
  printf 'FAIL: %s\n' "$1"
  FAIL_COUNT=$((FAIL_COUNT + 1))
  if [[ "${BODY:-}" != "" ]]; then
    printf '      body: %s\n' "$(printf '%s' "$BODY" | head -c 300 | tr '\n' ' ')"
  fi
}

request() {
  local method="$1"
  local url="$2"
  shift 2

  local payload=""
  if [[ $# -gt 0 && "$1" != -* ]]; then
    payload="$1"
    shift
  fi

  local body_file
  body_file="$(mktemp)"
  if [[ "$payload" != "" ]]; then
    STATUS="$(curl -sS -o "$body_file" -w "%{http_code}" -X "$method" "$url" \
      -H "Content-Type: application/json" \
      "$@" \
      -d "$payload" 2>/dev/null || true)"
  else
    STATUS="$(curl -sS -o "$body_file" -w "%{http_code}" -X "$method" "$url" \
      "$@" 2>/dev/null || true)"
  fi
  BODY="$(cat "$body_file")"
  rm -f "$body_file"
}

assert_status() {
  local name="$1"
  local expected="$2"
  if [[ "$STATUS" == "$expected" ]]; then
    pass "$name returned HTTP $expected"
  else
    fail "$name returned HTTP ${STATUS:-000}, expected $expected"
  fi
}

assert_status_any() {
  local name="$1"
  shift
  local allowed=("$@")
  local code
  for code in "${allowed[@]}"; do
    if [[ "$STATUS" == "$code" ]]; then
      pass "$name returned accepted HTTP $STATUS"
      return
    fi
  done
  fail "$name returned HTTP ${STATUS:-000}, expected one of: ${allowed[*]}"
}

printf 'Integration API smoke validation\n'
printf '%s\n' '================================'
printf 'Base URL: %s\n\n' "$BASE_URL"

printf 'Service readiness\n'
printf '%s\n' '-----------------'
for _ in $(seq 1 30); do
  request GET "$BASE_URL/health"
  [[ "$STATUS" == "200" ]] && break
  sleep 1
done
assert_status "health endpoint" "200"

if printf '%s' "$BODY" | jq -e '(.success == true and .data.status == "ok") or (.status == "ok" and .service == "integration-corev2")' >/dev/null 2>&1; then
  pass "health payload matches current integration-corev2 contract"
else
  fail "health payload did not match current contract"
fi

printf '\nProvider catalog\n'
printf '%s\n' '----------------'
request GET "$API_BASE/providers"
assert_status "provider catalog" "200"
if printf '%s' "$BODY" | jq -e '.success == true and (.data.providers | length) >= 1' >/dev/null 2>&1; then
  provider_count="$(printf '%s' "$BODY" | jq -r '.data.providers | length')"
  pass "provider catalog returned $provider_count providers"
else
  fail "provider catalog payload missing providers"
fi

printf '\nAuthentication boundaries\n'
printf '%s\n' '-------------------------'
for endpoint in /connections /connections/nonexistent-id /connect-sessions/test/status /scim/tokens; do
  request GET "$API_BASE$endpoint"
  assert_status "missing auth on $endpoint" "401"
done

request GET "$API_BASE/connections" "" -H "Authorization: Bearer invalid-token"
assert_status_any "invalid bearer token" "401" "503"

request GET "$API_BASE/connections" "" -H "x-internal-api-key: $INTERNAL_KEY"
assert_status "internal API key access" "200"
if printf '%s' "$BODY" | jq -e '.success == true and (.data.connections | type == "array")' >/dev/null 2>&1; then
  pass "connections payload contains an array"
else
  fail "connections payload did not contain an array"
fi

request POST "$API_BASE/providers/microsoft/connect-session" '{}' -H "x-internal-api-key: $INTERNAL_KEY"
assert_status "empty connect-session validation" "400"

request GET "$API_BASE/connections" "" -H "x-internal-api-key: wrong-key"
assert_status "wrong internal API key" "401"

printf '\nMutation edge cases\n'
printf '%s\n' '-------------------'
valid_payload='{"organizationId":"smoke-test-org","workspaceId":"smoke-test-ws","userId":"smoke-test-user","userEmail":"smoke@test.dev","bundles":["onboarding"]}'
request POST "$API_BASE/providers/google/connect-session" "$valid_payload" -H "x-internal-api-key: $INTERNAL_KEY"
assert_status_any "valid connect-session handler" "200" "400" "502"

request GET "$API_BASE/connections/invalid-id" "" -H "x-internal-api-key: $INTERNAL_KEY"
assert_status "invalid connection lookup" "404"

request POST "$API_BASE/webhooks/github" '{"repository":{"name":"demo"}}'
assert_status "webhook without signature" "401"

request POST "$API_BASE/webhooks/github" '{"repository":{"name":"demo"}}' -H "x-hub-signature-256: sha256=invalid"
assert_status "webhook with invalid signature" "401"

printf '\nSummary\n'
printf '%s\n' '-------'
printf 'Passed: %s\n' "$PASS_COUNT"
printf 'Failed: %s\n' "$FAIL_COUNT"

if (( FAIL_COUNT > 0 )); then
  exit 1
fi
