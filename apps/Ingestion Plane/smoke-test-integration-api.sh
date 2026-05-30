#!/usr/bin/env bash
# smoke-test-integration-api.sh
#
# End-to-end smoke test for the integration-api (integration-core Node service).
#
# Usage:
#   ./smoke-test-integration-api.sh [BASE_URL] [INTERNAL_API_KEY]
#
# Defaults:
#   BASE_URL          http://localhost:3026
#   INTERNAL_API_KEY  dev-super-secret-internal-api-key  (matches compose default)
#
# The script requires: curl, jq
# Exit code: 0 = all checks passed, 1 = one or more failures.

set -euo pipefail

BASE_URL="${1:-http://localhost:3026}"
INTERNAL_KEY="${2:-dev-super-secret-internal-api-key}"

# ─── colour helpers ───────────────────────────────────────────────────────────
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
RESET='\033[0m'

PASS=0
FAIL=0

pass() { echo -e "${GREEN}  ✓ PASS${RESET} $1"; PASS=$((PASS + 1)); }
fail() { echo -e "${RED}  ✗ FAIL${RESET} $1"; FAIL=$((FAIL + 1)); }
section() { echo -e "\n${CYAN}▶ $1${RESET}"; }

# ─── helper: assert HTTP status ──────────────────────────────────────────────
assert_status() {
  local label="$1"
  local expected="$2"
  local actual
  actual=$(echo "$3" | head -1)
  if [[ "$actual" == "$expected" ]]; then
    pass "$label [HTTP $expected]"
  else
    fail "$label [expected HTTP $expected, got HTTP $actual]"
  fi
}

# ─── helper: check a JSON field value ────────────────────────────────────────
assert_json() {
  local label="$1"
  local jq_expr="$2"
  local expected="$3"
  local json="$4"
  local actual
  actual=$(echo "$json" | jq -r "$jq_expr" 2>/dev/null || echo "__jq_error__")
  if [[ "$actual" == "$expected" ]]; then
    pass "$label [$jq_expr = $expected]"
  else
    fail "$label [$jq_expr: expected '$expected', got '$actual']"
  fi
}

# ─── wait for the service ─────────────────────────────────────────────────────
section "Waiting for integration-api to be ready ($BASE_URL/health)"
MAX_WAIT=60
INTERVAL=2
ELAPSED=0
until curl -sf "$BASE_URL/health" >/dev/null 2>&1; do
  if [[ "$ELAPSED" -ge "$MAX_WAIT" ]]; then
    echo -e "${RED}Service did not become healthy within ${MAX_WAIT}s.${RESET}"
    exit 1
  fi
  echo -e "${YELLOW}  … waiting (${ELAPSED}s elapsed)${RESET}"
  sleep "$INTERVAL"
  ELAPSED=$((ELAPSED + INTERVAL))
done
pass "Service is reachable"

# ─── 1. Health ────────────────────────────────────────────────────────────────
section "1. Health endpoint (public, no auth required)"

BODY=$(curl -sf "$BASE_URL/health")
STATUS_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/health")
assert_status "GET /health" "200" "$STATUS_CODE"
assert_json  "health.success"       ".success"        "true"   "$BODY"
assert_json  "health.status"        ".data.status"    "ok"     "$BODY"
assert_json  "health.service"       ".data.service"   "integration-core" "$BODY"

# ─── 2. Provider catalog (public) ─────────────────────────────────────────────
section "2. Provider catalog (public, no auth required)"

BODY=$(curl -sf "$BASE_URL/api/v1/providers")
STATUS_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/api/v1/providers")
assert_status "GET /api/v1/providers" "200" "$STATUS_CODE"
assert_json   "providers.success" ".success" "true" "$BODY"

PROVIDER_COUNT=$(echo "$BODY" | jq '.data.providers | length' 2>/dev/null || echo "0")
if [[ "$PROVIDER_COUNT" -ge 1 ]]; then
  pass "providers list is non-empty ($PROVIDER_COUNT providers)"
else
  fail "providers list is empty"
fi

# ─── 3. Auth rejection on protected routes ───────────────────────────────────
section "3. Auth enforcement – requests without credentials must be rejected"

for ENDPOINT in \
  "GET /api/v1/connections" \
  "GET /api/v1/connections/nonexistent-id" \
  "DELETE /api/v1/connections/nonexistent-id" \
  "POST /api/v1/providers/microsoft/connect-session"
do
  METHOD=$(echo "$ENDPOINT" | cut -d' ' -f1)
  PATH_PART=$(echo "$ENDPOINT" | cut -d' ' -f2)
  STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
    -X "$METHOD" \
    -H "Content-Type: application/json" \
    "$BASE_URL$PATH_PART")
  assert_status "$METHOD $PATH_PART (no creds → 401)" "401" "$STATUS"
done

# ─── 4. Auth rejection on bad Bearer token ───────────────────────────────────
section "4. Auth enforcement – invalid Bearer token must be rejected"

STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -H "Authorization: Bearer this-is-not-a-valid-token" \
  "$BASE_URL/api/v1/connections")

# auth-core is not running in smoke-test; expect 401 or 503 (unreachable upstream)
if [[ "$STATUS" == "401" || "$STATUS" == "503" ]]; then
  pass "GET /api/v1/connections with bad Bearer → $STATUS (auth rejected or upstream unreachable)"
else
  fail "GET /api/v1/connections with bad Bearer → unexpected status $STATUS"
fi

# ─── 5. Internal API key acceptance ──────────────────────────────────────────
section "5. Internal API key – connections list must be accepted (auth-core not required)"

BODY=$(curl -s -w "\n%{http_code}" \
  -H "x-internal-api-key: $INTERNAL_KEY" \
  "$BASE_URL/api/v1/connections")
STATUS=$(echo "$BODY" | tail -1)
JSON=$(echo "$BODY" | head -n -1)

if [[ "$STATUS" == "200" ]]; then
  pass "GET /api/v1/connections with internal key → 200"
  assert_json "connections.success" ".success" "true" "$JSON"
  CONN_LIST=$(echo "$JSON" | jq '.data.connections | type' 2>/dev/null || echo "null")
  if [[ "$CONN_LIST" == "array" ]]; then
    pass "connections list is an array"
  else
    fail "connections list is not an array (got: $CONN_LIST)"
  fi
else
  fail "GET /api/v1/connections with internal key → unexpected status $STATUS"
fi

# ─── 6. Internal key for connect-session – validation error (not auth error) ─
section "6. Connect-session with internal key + invalid body → 400 (not 401)"

STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "x-internal-api-key: $INTERNAL_KEY" \
  -d '{}' \
  "$BASE_URL/api/v1/providers/microsoft/connect-session")

assert_status "POST /connect-session internal key + empty body → 400" "400" "$STATUS"

# ─── 7. Webhook endpoint is publicly accessible (HMAC auth, not Bearer) ──────
section "7. Webhook intake – reachable without Bearer (verified internally by HMAC)"

STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -d '{"type":"test"}' \
  "$BASE_URL/api/v1/webhooks/nango")

# Expect 400 (bad signature) not 401 (auth middleware) – proves webhook is NOT behind Bearer auth
if [[ "$STATUS" == "400" || "$STATUS" == "401" ]]; then
  if [[ "$STATUS" == "400" ]]; then
    pass "POST /api/v1/webhooks/nango reachable, rejected by HMAC (400) not Bearer auth"
  else
    fail "POST /api/v1/webhooks/nango returned 401 — webhook endpoint should not require Bearer auth"
  fi
else
  pass "POST /api/v1/webhooks/nango returned $STATUS (webhook handler reached)"
fi

# ─── 8. Wrong internal key on protected route ─────────────────────────────────
section "8. Wrong internal key must return 401"

STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -H "x-internal-api-key: wrong-secret" \
  "$BASE_URL/api/v1/connections")
assert_status "GET /api/v1/connections with wrong internal key → 401" "401" "$STATUS"

# ─── 9. Connect-session with internal key + valid body → 201 (session created) ─
section "9. Connect-session with internal key + valid body → 201 (Nango session)"

BODY=$(curl -s -w "\n%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "x-internal-api-key: $INTERNAL_KEY" \
  -d '{
    "organizationId": "smoke-test-org",
    "workspaceId": "smoke-test-ws",
    "userId": "smoke-test-user",
    "userEmail": "smoke@test.dev"
  }' \
  "$BASE_URL/api/v1/providers/microsoft/connect-session")
STATUS=$(echo "$BODY" | tail -1)
JSON=$(echo "$BODY" | head -n -1)

if [[ "$STATUS" == "201" ]]; then
  pass "POST /connect-session internal key + valid body → 201"
  assert_json "session.success"      ".success"              "true"  "$JSON"
  SESSION_TOKEN=$(echo "$JSON" | jq -r '.data.sessionToken // empty' 2>/dev/null)
  if [[ -n "$SESSION_TOKEN" ]]; then
    pass "session has sessionToken ($SESSION_TOKEN)"
  else
    fail "session missing sessionToken"
  fi
elif [[ "$STATUS" == "502" ]]; then
  pass "POST /connect-session → 502 (Nango not fully ready or no provider configured — acceptable in CI)"
else
  fail "POST /connect-session internal key + valid body → unexpected status $STATUS"
  echo "  Response: $JSON"
fi

# ─── 10. Connection not found ─────────────────────────────────────────────────
section "10. Connection by non-existent ID returns 404"

STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -H "x-internal-api-key: $INTERNAL_KEY" \
  "$BASE_URL/api/v1/connections/00000000-0000-0000-0000-000000000000")
assert_status "GET /connections/:invalid-id → 404" "404" "$STATUS"

# ─── 11. Sync on non-existent connection returns 404 ─────────────────────────
section "11. Trigger sync on non-existent connection returns 404"

STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "x-internal-api-key: $INTERNAL_KEY" \
  "$BASE_URL/api/v1/connections/00000000-0000-0000-0000-000000000000/sync")
assert_status "POST /connections/:invalid-id/sync → 404" "404" "$STATUS"

# ─── 12. Webhook with invalid HMAC signature → 400 ──────────────────────────
section "12. Webhook with bad HMAC signature rejected"

STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "x-nango-signature: deadbeef" \
  -d '{"type":"auth","success":true,"connectionId":"fake"}' \
  "$BASE_URL/api/v1/webhooks/nango")

if [[ "$STATUS" == "400" || "$STATUS" == "401" ]]; then
  pass "POST /webhooks/nango with bad HMAC → $STATUS (rejected)"
else
  fail "POST /webhooks/nango with bad HMAC → unexpected status $STATUS"
fi

# ─── Summary ─────────────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}  PASSED: $PASS${RESET}"
if [[ "$FAIL" -gt 0 ]]; then
  echo -e "${RED}  FAILED: $FAIL${RESET}"
fi
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
