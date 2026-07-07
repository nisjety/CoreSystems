#!/usr/bin/env bash
set -euo pipefail

AUTH_URL="${AUTH_URL:-http://localhost:3011}"
USER_URL="${USER_URL:-http://localhost:3012}"
ORG_URL="${ORG_URL:-http://localhost:18080}"
BILLING_URL="${BILLING_URL:-http://localhost:3014}"
SESSION_URL="${SESSION_URL:-http://localhost:3015}"
AUDIT_URL="${AUDIT_URL:-http://localhost:8187}"
PG_CONTAINER="${PG_CONTAINER:-controlplane-postgres}"

PASS_COUNT=0
FAIL_COUNT=0

pass() {
  printf 'PASS: %s\n' "$1"
  PASS_COUNT=$((PASS_COUNT + 1))
}

fail() {
  printf 'FAIL: %s\n' "$1"
  FAIL_COUNT=$((FAIL_COUNT + 1))
}

curl_status() {
  local url="$1"
  local body_file
  body_file="$(mktemp)"
  HTTP_BODY_FILE="$body_file"
  HTTP_STATUS="$(curl -sS --max-time 5 -o "$body_file" -w "%{http_code}" "$url" 2>/dev/null || true)"
}

assert_http() {
  local name="$1"
  local url="$2"
  local expected="$3"

  curl_status "$url"
  if [[ "$HTTP_STATUS" == "$expected" ]]; then
    pass "$name returned HTTP $expected"
  else
    fail "$name returned HTTP ${HTTP_STATUS:-000}, expected $expected"
    printf '      url: %s\n' "$url"
    printf '      body: %s\n' "$(head -c 300 "$HTTP_BODY_FILE" | tr '\n' ' ')"
  fi
  rm -f "$HTTP_BODY_FILE"
}

printf 'Control Plane smoke validation\n'
printf '%s\n\n' '================================'

printf 'Service health checks\n'
printf '%s\n' '---------------------'
assert_http "auth-core session endpoint" "$AUTH_URL/api/auth/get-session" "200"
assert_http "user-core health" "$USER_URL/health" "200"
assert_http "org-core health" "$ORG_URL/health" "200"
assert_http "billing-core health" "$BILLING_URL/health" "200"
assert_http "session-core health" "$SESSION_URL/health" "200"
assert_http "audit-core health" "$AUDIT_URL/healthz" "200"

printf '\nDatabase checks\n'
printf '%s\n' '---------------'
if ! docker inspect "$PG_CONTAINER" >/dev/null 2>&1; then
  fail "Postgres container '$PG_CONTAINER' exists"
else
  pass "Postgres container '$PG_CONTAINER' exists"

  db_ping="$(docker exec "$PG_CONTAINER" sh -lc 'psql -U "$POSTGRES_USER" -d "${POSTGRES_DB:-controlplane}" -tAq -v ON_ERROR_STOP=1 -c "select 1"' 2>/dev/null || true)"
  if [[ "$db_ping" == "1" ]]; then
    pass "Postgres accepts SQL on configured control-plane database"
  else
    fail "Postgres SQL readiness check failed"
  fi

  table_count="$(docker exec "$PG_CONTAINER" sh -lc 'psql -U "$POSTGRES_USER" -d "${POSTGRES_DB:-controlplane}" -tAq -v ON_ERROR_STOP=1 -c "select count(*) from pg_tables"' 2>/dev/null || true)"
  if [[ "$table_count" =~ ^[0-9]+$ ]]; then
    pass "Postgres catalog is readable ($table_count tables visible)"
  else
    fail "Postgres catalog table count query failed"
  fi
fi

printf '\nSummary\n'
printf '%s\n' '-------'
printf 'Passed: %s\n' "$PASS_COUNT"
printf 'Failed: %s\n' "$FAIL_COUNT"

if (( FAIL_COUNT > 0 )); then
  exit 1
fi
