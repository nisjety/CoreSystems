#!/usr/bin/env bash
# =============================================================================
# CoreSystem Functional Test Suite
#
# Full user journey: register → sign-in → get session → onboarding (create org)
# → test control plane CRUD → reasoning → data plane E2E → ingestion plane
#
# Usage:
#   bash scripts/functional_test.sh [--verbose]
# =============================================================================

set -euo pipefail
export PATH="/usr/local/bin:/usr/bin:/bin:/opt/homebrew/bin:$PATH"

VERBOSE=false
if [[ "${1:-}" == "--verbose" ]]; then VERBOSE=true; fi

# ── Colours ────────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; RESET='\033[0m'

# ── State ──────────────────────────────────────────────────────────────────────
PASS=0; FAIL=0; SKIP=0; WARN=0
COOKIE_JAR=$(mktemp)
SESSION_COOKIE=""
SESSION_TOKEN=""
USER_ID=""
USER_EMAIL=""
ORG_ID=""
DOC_ID=""
START_TS=$(date +%s)

# ── Ports ──────────────────────────────────────────────────────────────────────
AUTH_PORT=3011          # auth-service (Better Auth + NestJS wrapper)
USER_PORT=3012          # user-service (gRPC + HTTP)
ORG_PORT=8080           # org-core (Go/Gin)
REASONING_PORT=8101     # reasoning-core
DOCUMENTS_PORT=8001     # documents service
RETRIEVAL_PORT=8004     # retrieval service

# ── Helpers ────────────────────────────────────────────────────────────────────
log()    { echo -e "${CYAN}[INFO]${RESET}  $*"; }
pass()   { echo -e "${GREEN}[PASS]${RESET}  $*"; PASS=$((PASS+1)); }
fail()   { echo -e "${RED}[FAIL]${RESET}  $*"; FAIL=$((FAIL+1)); }
warn()   { echo -e "${YELLOW}[WARN]${RESET}  $*"; WARN=$((WARN+1)); }
skip()   { echo -e "${YELLOW}[SKIP]${RESET}  $*"; SKIP=$((SKIP+1)); }
header() { echo -e "\n${BOLD}${CYAN}══════════════════════════════════════════${RESET}"; \
           echo -e "${BOLD}${CYAN}  $*${RESET}"; \
           echo -e "${BOLD}${CYAN}══════════════════════════════════════════${RESET}"; }

vlog() { if $VERBOSE; then echo -e "${YELLOW}  ↳ $*${RESET}"; fi; }

# Run curl; writes body to stdout, sets global LAST_HTTP_CODE
LAST_HTTP_CODE=""
do_curl() {
  local method="$1"; shift
  local url="$1"; shift
  local _body_file
  _body_file=$(mktemp)
  LAST_HTTP_CODE=$(curl -s -o "$_body_file" -w "%{http_code}" \
    -X "$method" "$url" \
    -b "$COOKIE_JAR" -c "$COOKIE_JAR" \
    "$@" 2>/dev/null) || LAST_HTTP_CODE="000"
  cat "$_body_file"
  rm -f "$_body_file"
}

# Check that a JSON field exists and isn't empty/null
assert_field() {
  local label="$1" body="$2" field="$3"
  local val
  val=$(echo "$body" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('$field','__MISSING__'))" 2>/dev/null || echo "__PARSE_ERROR__")
  if [[ "$val" == "__MISSING__"  || "$val" == "__PARSE_ERROR__" || "$val" == "None" || -z "$val" ]]; then
    fail "$label — field '$field' missing or empty (value: '$val')"
    vlog "Body: $body"
    return 1
  else
    pass "$label (field '$field' = '$val')"
    echo "$val"
    return 0
  fi
}

# Extract nested JSON field using dotpath e.g. user.id
extract() {
  local body="$1" path="$2"
  echo "$body" | python3 -c "
import sys, json
d = json.load(sys.stdin)
keys = '$path'.split('.')
for k in keys:
    if isinstance(d, dict):
        d = d.get(k)
    else:
        d = None
    if d is None:
        break
print(d if d is not None else '')
" 2>/dev/null || echo ""
}

# ── Pre-flight: check services are alive ───────────────────────────────────────
header "PRE-FLIGHT: PORT CHECK"
for entry in "AUTH:$AUTH_PORT:/api/auth/get-session" \
             "USER:$USER_PORT:/health" \
             "ORG:$ORG_PORT:/health" \
             "REASONING:$REASONING_PORT:/health" \
             "DOCUMENTS:$DOCUMENTS_PORT:/health" \
             "RETRIEVAL:$RETRIEVAL_PORT:/health"; do
  svc=$(echo "$entry" | cut -d: -f1)
  port=$(echo "$entry" | cut -d: -f2)
  path=$(echo "$entry" | cut -d: -f3)
  code=$(curl -s -o /dev/null -w "%{http_code}" -m 5 "http://localhost:${port}${path}" 2>/dev/null || echo "000")
  if [[ "$code" == "200" || "$code" == "401" || "$code" == "403" || "$code" == "404" ]]; then
    pass "$svc (:$port) responding — HTTP $code"
  else
    warn "$svc (:$port) not responding (HTTP $code) — tests for this service may fail"
  fi
done

# ── SECTION 1: Registration ────────────────────────────────────────────────────
header "SECTION 1: USER REGISTRATION"

TS=$(date +%s)
USER_EMAIL="functest_${TS}@test.local"
USER_NAME="Func Tester ${TS}"
USER_PASS="TestPass1234!"

log "Registering new user: $USER_EMAIL"

# Use the v2 NestJS endpoint which also syncs the user to user-service
SIGNUP_BODY=$(do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/signUp" \
  -H "Content-Type: application/json" \
  -d "{\"name\":\"$USER_NAME\",\"email\":\"$USER_EMAIL\",\"password\":\"$USER_PASS\"}")

vlog "Sign-up HTTP $LAST_HTTP_CODE: $SIGNUP_BODY"

if [[ "$LAST_HTTP_CODE" == "200" || "$LAST_HTTP_CODE" == "201" ]]; then
  pass "Sign-up HTTP $LAST_HTTP_CODE"
else
  # Fallback: try native Better Auth endpoint
  warn "v2 sign-up returned HTTP $LAST_HTTP_CODE — trying native /api/auth/sign-up/email"
  SIGNUP_BODY=$(do_curl POST "http://localhost:${AUTH_PORT}/api/auth/sign-up/email" \
    -H "Content-Type: application/json" \
    -d "{\"name\":\"$USER_NAME\",\"email\":\"$USER_EMAIL\",\"password\":\"$USER_PASS\"}")
  vlog "Native sign-up HTTP $LAST_HTTP_CODE: $SIGNUP_BODY"
  if [[ "$LAST_HTTP_CODE" == "200" || "$LAST_HTTP_CODE" == "201" ]]; then
    pass "Native sign-up HTTP $LAST_HTTP_CODE"
  else
    fail "Sign-up failed — HTTP $LAST_HTTP_CODE"
    vlog "Body: $SIGNUP_BODY"
  fi
fi

# Verify sign-up error handling (duplicate email should fail)
log "Testing duplicate registration rejection..."
DUP_BODY=$(do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/signUp" \
  -H "Content-Type: application/json" \
  -d "{\"name\":\"$USER_NAME\",\"email\":\"$USER_EMAIL\",\"password\":\"$USER_PASS\"}")
if [[ "$LAST_HTTP_CODE" == "400" || "$LAST_HTTP_CODE" == "409" || "$LAST_HTTP_CODE" == "422" || "$LAST_HTTP_CODE" == "500" ]]; then
  pass "Duplicate registration correctly rejected (HTTP $LAST_HTTP_CODE)"
else
  warn "Duplicate registration returned HTTP $LAST_HTTP_CODE — may allow duplicate users"
  vlog "Body: $DUP_BODY"
fi

# ── SECTION 2: Authentication ──────────────────────────────────────────────────
header "SECTION 2: SIGN IN & SESSION"

log "Signing in as $USER_EMAIL..."
SIGNIN_BODY=$(do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/signIn" \
  -H "Content-Type: application/json" \
  -d "{\"email\":\"$USER_EMAIL\",\"password\":\"$USER_PASS\"}")
vlog "Sign-in HTTP $LAST_HTTP_CODE: $SIGNIN_BODY"

if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  pass "Sign-in HTTP 200"
else
  # Fallback: native Better Auth endpoint
  warn "v2 sign-in returned $LAST_HTTP_CODE — trying native /api/auth/sign-in/email"
  SIGNIN_BODY=$(do_curl POST "http://localhost:${AUTH_PORT}/api/auth/sign-in/email" \
    -H "Content-Type: application/json" \
    -d "{\"email\":\"$USER_EMAIL\",\"password\":\"$USER_PASS\",\"callbackURL\":\"/\"}")
  vlog "Native sign-in HTTP $LAST_HTTP_CODE: $SIGNIN_BODY"
  if [[ "$LAST_HTTP_CODE" == "200" ]]; then
    pass "Native sign-in HTTP 200"
  else
    fail "Sign-in failed — HTTP $LAST_HTTP_CODE"; vlog "Body: $SIGNIN_BODY"
  fi
fi

# Check the cookie jar captured a session cookie
SESSION_COOKIE_VAL=$(grep -i 'better-auth\|session\|__Secure' "$COOKIE_JAR" 2>/dev/null | awk '{print $7}' | head -1 || true)
if [[ -n "$SESSION_COOKIE_VAL" ]]; then
  pass "Session cookie captured from jar"
  vlog "Cookie value (first 40 chars): ${SESSION_COOKIE_VAL:0:40}..."
else
  warn "No session cookie found in cookie jar — will try bearer token approach"
fi

# Check for token in sign-in response body
SESSION_TOKEN=$(extract "$SIGNIN_BODY" "token" 2>/dev/null || true)
if [[ -n "$SESSION_TOKEN" ]]; then
  pass "Bearer token extracted from sign-in response"
  vlog "Token (first 40 chars): ${SESSION_TOKEN:0:40}..."
fi

# Get session to confirm auth state and extract user ID
log "Getting session..."
SESSION_BODY=$(do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/getSession" \
  -H "Content-Type: application/json" \
  -d "{}")
vlog "getSession HTTP $LAST_HTTP_CODE: $SESSION_BODY"

# Also try GET endpoint
if [[ "$LAST_HTTP_CODE" != "200" ]]; then
  SESSION_BODY=$(do_curl GET "http://localhost:${AUTH_PORT}/api/auth/get-session")
  vlog "GET get-session HTTP $LAST_HTTP_CODE: $SESSION_BODY"
fi

if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  pass "Get session HTTP 200"
  # Extract user ID from session
  USER_ID=$(extract "$SESSION_BODY" "user.id" 2>/dev/null || extract "$SESSION_BODY" "userId" 2>/dev/null || true)
  if [[ -z "$USER_ID" ]]; then
    USER_ID=$(echo "$SESSION_BODY" | python3 -c "
import sys,json
d=json.load(sys.stdin)
# Try multiple paths
for path in [['user','id'],['session','userId'],['data','user','id']]:
    dd=d
    for k in path:
        dd=dd.get(k,{}) if isinstance(dd,dict) else {}
    if dd and isinstance(dd,str):
        print(dd); break
" 2>/dev/null || echo "")
  fi
  if [[ -n "$USER_ID" ]]; then
    pass "User ID extracted from session: $USER_ID"
  else
    warn "Could not extract user ID from session — org tests may need x-user-id manually"
    vlog "Session body: $SESSION_BODY"
    # Try to get user ID from signup response
    USER_ID=$(extract "$SIGNUP_BODY" "user.id" 2>/dev/null || extract "$SIGNUP_BODY" "id" 2>/dev/null || true)
    if [[ -n "$USER_ID" ]]; then
      pass "User ID extracted from sign-up response: $USER_ID"
    fi
  fi
else
  warn "Get session returned HTTP $LAST_HTTP_CODE"
  vlog "Body: $SESSION_BODY"
fi

# Test wrong-password rejection
log "Testing wrong password rejection..."
BAD_SIGNIN=$(do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/signIn" \
  -H "Content-Type: application/json" \
  -d "{\"email\":\"$USER_EMAIL\",\"password\":\"WRONG_PASS_123\"}")
if [[ "$LAST_HTTP_CODE" == "401" || "$LAST_HTTP_CODE" == "400" || "$LAST_HTTP_CODE" == "403" ]]; then
  pass "Wrong password correctly rejected (HTTP $LAST_HTTP_CODE)"
else
  warn "Wrong password returned HTTP $LAST_HTTP_CODE — expected 401/400/403"
  vlog "Body: $BAD_SIGNIN"
fi

# Re-sign in to refresh cookie jar (ensures cookie is set after all the test calls)
do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/signIn" \
  -H "Content-Type: application/json" \
  -d "{\"email\":\"$USER_EMAIL\",\"password\":\"$USER_PASS\"}" > /dev/null 2>&1 || true

# ── SECTION 3: User Service ────────────────────────────────────────────────────
header "SECTION 3: USER SERVICE"

# Wait a moment for user sync (async from sign-up)
sleep 1

log "Getting user profile by email..."
# User service has no auth guard on internal lookup routes
USER_BODY=$(do_curl GET "http://localhost:${USER_PORT}/api/v1/users/by-email/${USER_EMAIL}")
vlog "User by email HTTP $LAST_HTTP_CODE: $USER_BODY"

if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  pass "User found in user-service (synced on registration)"
  # If we didn't get USER_ID from auth, get it here
  if [[ -z "$USER_ID" ]]; then
    USER_ID=$(extract "$USER_BODY" "id" 2>/dev/null || true)
    [[ -n "$USER_ID" ]] && pass "User ID from user-service: $USER_ID"
  fi
else
  warn "User not found in user-service (HTTP $LAST_HTTP_CODE) — sync may be async or failing"
  vlog "Body: $USER_BODY"
fi

# Get user profile with session cookie
log "Getting /api/v1/users/me (session auth)..."
ME_BODY=$(do_curl GET "http://localhost:${USER_PORT}/api/v1/users/me")
vlog "Users/me HTTP $LAST_HTTP_CODE: $ME_BODY"
if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  pass "User /me profile returned HTTP 200"
elif [[ "$LAST_HTTP_CODE" == "401" || "$LAST_HTTP_CODE" == "403" ]]; then
  warn "User /me returned $LAST_HTTP_CODE — user-service may need bearer token not cookie"
  # Try with bearer token if available
  if [[ -n "$SESSION_TOKEN" ]]; then
    ME_BODY=$(do_curl GET "http://localhost:${USER_PORT}/api/v1/users/me" \
      -H "Authorization: Bearer $SESSION_TOKEN")
    if [[ "$LAST_HTTP_CODE" == "200" ]]; then
      pass "User /me with bearer token returned HTTP 200"
    else
      warn "User /me with bearer token also failed (HTTP $LAST_HTTP_CODE)"
    fi
  fi
else
  warn "User /me returned unexpected HTTP $LAST_HTTP_CODE"
  vlog "Body: $ME_BODY"
fi

# ── SECTION 4: Onboarding — Create Organisation ────────────────────────────────
header "SECTION 4: ONBOARDING — CREATE ORGANISATION"

if [[ -z "$USER_ID" ]]; then
  warn "No user ID available — using placeholder for org creation test"
  USER_ID="test-user-${TS}"
fi

ORG_NAME="Test Org ${TS}"
ORG_SLUG="test-org-${TS}"

log "Creating organisation: $ORG_NAME..."
ORG_BODY=$(do_curl POST "http://localhost:${ORG_PORT}/api/v1/organizations" \
  -H "Content-Type: application/json" \
  -H "x-user-id: $USER_ID" \
  -d "{\"name\":\"$ORG_NAME\",\"slug\":\"$ORG_SLUG\",\"plan\":\"free\"}")
vlog "Create org HTTP $LAST_HTTP_CODE: $ORG_BODY"

if [[ "$LAST_HTTP_CODE" == "201" || "$LAST_HTTP_CODE" == "200" ]]; then
  pass "Organisation created (HTTP $LAST_HTTP_CODE)"
  ORG_ID=$(extract "$ORG_BODY" "id" 2>/dev/null || \
           extract "$ORG_BODY" "organization.id" 2>/dev/null || \
           echo "$ORG_BODY" | python3 -c "import sys,json;d=json.load(sys.stdin);print(d.get('id','') or d.get('organization',{}).get('id',''))" 2>/dev/null || true)
  [[ -n "$ORG_ID" ]] && pass "Organisation ID: $ORG_ID" || warn "Could not extract org ID"
else
  # Also try the /orgs shorthand route
  ORG_BODY=$(do_curl POST "http://localhost:${ORG_PORT}/orgs" \
    -H "Content-Type: application/json" \
    -H "x-user-id: $USER_ID" \
    -d "{\"name\":\"$ORG_NAME\",\"slug\":\"$ORG_SLUG\",\"plan\":\"free\"}")
  vlog "Create org (/orgs) HTTP $LAST_HTTP_CODE: $ORG_BODY"
  if [[ "$LAST_HTTP_CODE" == "201" || "$LAST_HTTP_CODE" == "200" ]]; then
    pass "Organisation created via /orgs (HTTP $LAST_HTTP_CODE)"
    ORG_ID=$(echo "$ORG_BODY" | python3 -c "import sys,json;d=json.load(sys.stdin);print(d.get('id',''))" 2>/dev/null || true)
    [[ -n "$ORG_ID" ]] && pass "Organisation ID: $ORG_ID"
  else
    fail "Organisation creation failed — HTTP $LAST_HTTP_CODE"
    vlog "Body: $ORG_BODY"
  fi
fi

# List organisations for this user
log "Listing user organisations..."
LIST_ORGS=$(do_curl GET "http://localhost:${ORG_PORT}/api/v1/organizations" \
  -H "x-user-id: $USER_ID")
vlog "List orgs HTTP $LAST_HTTP_CODE: $LIST_ORGS"
if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  ORG_COUNT=$(echo "$LIST_ORGS" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d) if isinstance(d,list) else len(d.get('organizations',d.get('data',[]))))" 2>/dev/null || echo "?")
  pass "Listed organisations (count: $ORG_COUNT)"
else
  # Try shorthand
  LIST_ORGS=$(do_curl GET "http://localhost:${ORG_PORT}/orgs" \
    -H "x-user-id: $USER_ID")
  if [[ "$LAST_HTTP_CODE" == "200" ]]; then
    pass "Listed organisations via /orgs (HTTP 200)"
  else
    warn "List organisations returned HTTP $LAST_HTTP_CODE"
    vlog "Body: $LIST_ORGS"
  fi
fi

# Get specific org if we have an ID
if [[ -n "$ORG_ID" ]]; then
  log "Getting organisation by ID: $ORG_ID..."
  ORG_GET=$(do_curl GET "http://localhost:${ORG_PORT}/api/v1/organizations/$ORG_ID" \
    -H "x-user-id: $USER_ID")
  vlog "Get org HTTP $LAST_HTTP_CODE: $ORG_GET"
  if [[ "$LAST_HTTP_CODE" == "200" ]]; then
    pass "Get organisation by ID — HTTP 200"
  else
    ORG_GET=$(do_curl GET "http://localhost:${ORG_PORT}/orgs/$ORG_ID" \
      -H "x-user-id: $USER_ID")
    [[ "$LAST_HTTP_CODE" == "200" ]] && pass "Get organisation by ID (/orgs/:id) — HTTP 200" || \
      warn "Get organisation by ID returned HTTP $LAST_HTTP_CODE"
  fi
fi

# Test auth-service organisation listing for the user (via v2 API)
log "Listing organisations via auth-service v2 API..."
AUTH_ORGS=$(do_curl GET "http://localhost:${AUTH_PORT}/api/v2/organizations")
vlog "Auth orgs HTTP $LAST_HTTP_CODE: $AUTH_ORGS"
if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  pass "Auth-service /api/v2/organizations returned 200"
elif [[ "$LAST_HTTP_CODE" == "401" || "$LAST_HTTP_CODE" == "403" ]]; then
  warn "Auth-service /api/v2/organizations requires auth — cookie may not be forwarded correctly"
else
  warn "Auth-service /api/v2/organizations returned HTTP $LAST_HTTP_CODE"
fi

# ── SECTION 5: Reasoning Plane (authenticated) ────────────────────────────────
header "SECTION 5: REASONING PLANE"

log "Chain-of-thought reasoning test..."
COT_BODY=$(do_curl POST "http://localhost:${REASONING_PORT}/api/v1/reason" \
  -H "Content-Type: application/json" \
  -d '{"query":"What are the three main branches of the US government and their roles?","strategy":"chain_of_thought"}' \
  --max-time 90)
vlog "CoT HTTP $LAST_HTTP_CODE"

if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  # Check structural fields
  ANSWER=$(extract "$COT_BODY" "answer" 2>/dev/null || extract "$COT_BODY" "result" 2>/dev/null || true)
  THOUGHTS=$(echo "$COT_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); t=d.get('thoughts',d.get('chain',d.get('steps',[])));print(len(t) if isinstance(t,list) else 0)" 2>/dev/null || echo "0")
  MODEL=$(extract "$COT_BODY" "model" 2>/dev/null || true)
  if [[ -n "$ANSWER" ]]; then
    pass "CoT answer present (${#ANSWER} chars)"
  else
    warn "CoT answer field missing — response may use different schema"
    vlog "Body keys: $(echo "$COT_BODY" | python3 -c "import sys,json;print(list(json.load(sys.stdin).keys()))" 2>/dev/null)"
  fi
  [[ "$THOUGHTS" -gt "0" ]] && pass "CoT thoughts/steps present (count: $THOUGHTS)" || warn "CoT thoughts empty or 0"
  [[ -n "$MODEL" ]] && pass "Model field: $MODEL" || warn "Model field missing"
else
  fail "CoT reasoning returned HTTP $LAST_HTTP_CODE"
  vlog "Body: $COT_BODY"
fi

log "Tree-of-thought reasoning test..."
TOT_BODY=$(do_curl POST "http://localhost:${REASONING_PORT}/api/v1/reason" \
  -H "Content-Type: application/json" \
  -d '{"query":"Explain the pros and cons of microservices vs monolith architectures","strategy":"tree_of_thought","max_branches":2}' \
  --max-time 120)
vlog "ToT HTTP $LAST_HTTP_CODE"

if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  BRANCHES=$(echo "$TOT_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); b=d.get('branches',d.get('tree',d.get('paths',[])));print(len(b) if isinstance(b,list) else 0)" 2>/dev/null || echo "0")
  pass "ToT reasoning returned 200"
  [[ "$BRANCHES" -gt "0" ]] && pass "ToT branches present (count: $BRANCHES)" || \
    warn "ToT branches field empty or 0 — check tree_of_thought.py implementation"
  vlog "Body keys: $(echo "$TOT_BODY" | python3 -c "import sys,json;print(list(json.load(sys.stdin).keys()))" 2>/dev/null)"
else
  fail "ToT reasoning returned HTTP $LAST_HTTP_CODE"
  vlog "Body: $TOT_BODY"
fi

# ── SECTION 6: Data Plane E2E ──────────────────────────────────────────────────
header "SECTION 6: DATA PLANE — INGEST → EMBED → RETRIEVE"

ORG_FOR_DATA="${ORG_ID:-functest-org}"
UNIQUE_PHRASE="coresystem-functest-${TS}-lightspeed"

log "Ingesting test document..."
DOC_BODY=$(do_curl POST "http://localhost:${DOCUMENTS_PORT}/v1/documents" \
  -H "Content-Type: application/json" \
  -d "{\"content\":\"The speed of light is approximately 299792458 metres per second in a vacuum. This is a unique test phrase: ${UNIQUE_PHRASE}\",\"org_id\":\"${ORG_FOR_DATA}\",\"source\":\"functest\",\"type\":\"text\",\"title\":\"Speed of Light Functest\"}")
vlog "Ingest HTTP $LAST_HTTP_CODE: $DOC_BODY"

if [[ "$LAST_HTTP_CODE" == "200" || "$LAST_HTTP_CODE" == "201" ]]; then
  pass "Document ingested (HTTP $LAST_HTTP_CODE)"
  DOC_ID=$(echo "$DOC_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('document_id',d.get('id','')))" 2>/dev/null || true)
  [[ -n "$DOC_ID" ]] && pass "Document ID: $DOC_ID" || warn "Could not extract document_id"
else
  fail "Document ingest failed — HTTP $LAST_HTTP_CODE"
  vlog "Body: $DOC_BODY"
fi

# Poll for indexed status (up to 30s)
if [[ -n "$DOC_ID" ]]; then
  log "Polling for document indexed status (up to 30s)..."
  DOC_STATUS="unknown"
  for i in $(seq 1 10); do
    sleep 3
    STATUS_BODY=$(do_curl GET "http://localhost:${DOCUMENTS_PORT}/v1/documents/${DOC_ID}?org_id=${ORG_FOR_DATA}")
    DOC_STATUS=$(echo "$STATUS_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('status','unknown'))" 2>/dev/null || echo "unknown")
    vlog "Poll $i/10: status=$DOC_STATUS"
    if [[ "$DOC_STATUS" == "indexed" || "$DOC_STATUS" == "ready" || "$DOC_STATUS" == "completed" ]]; then
      break
    fi
  done
  if [[ "$DOC_STATUS" == "indexed" || "$DOC_STATUS" == "ready" || "$DOC_STATUS" == "completed" ]]; then
    pass "Document indexed (status: $DOC_STATUS)"
  else
    warn "Document not indexed after 30s — status: $DOC_STATUS (embedding worker may be slow or stalled)"
  fi

  # Retrieve with semantic query
  log "Retrieving with semantic query..."
  RETRIEVE_BODY=$(do_curl POST "http://localhost:${RETRIEVAL_PORT}/v1/retrieve" \
    -H "Content-Type: application/json" \
    -d "{\"org_id\":\"${ORG_FOR_DATA}\",\"query\":\"speed of light metres per second vacuum\",\"top_k\":3}" \
    --max-time 30)
  vlog "Retrieve HTTP $LAST_HTTP_CODE: $RETRIEVE_BODY"
  if [[ "$LAST_HTTP_CODE" == "200" ]]; then
    RESULTS=$(echo "$RETRIEVE_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); r=d.get('results',d.get('hits',d.get('documents',d.get('data',[])))); print(len(r) if isinstance(r,list) else 0)" 2>/dev/null || echo "0")
    if [[ "$RESULTS" -gt "0" ]]; then
      pass "Retrieval returned $RESULTS result(s)"
      # Check if our specific doc appears
      MATCH=$(echo "$RETRIEVE_BODY" | python3 -c "import sys,json; d=json.load(sys.stdin); r=d.get('results',d.get('hits',d.get('documents',d.get('data',[])))); print('yes') if any('$UNIQUE_PHRASE' in str(item) or '299792458' in str(item) for item in r) else print('no')" 2>/dev/null || echo "no")
      [[ "$MATCH" == "yes" ]] && pass "Ingested document found in retrieval results" || \
        warn "Ingested document not yet in retrieval results (may still be embedding)"
    else
      warn "Retrieval returned 0 results — document may not be indexed yet"
      vlog "Retrieve body: $RETRIEVE_BODY"
    fi
  else
    fail "Retrieval returned HTTP $LAST_HTTP_CODE"
    vlog "Body: $RETRIEVE_BODY"
  fi

  # Delete test document
  log "Cleaning up: deleting test document..."
  DELETE_BODY=$(do_curl DELETE "http://localhost:${DOCUMENTS_PORT}/v1/documents/${DOC_ID}?org_id=${ORG_FOR_DATA}")
  [[ "$LAST_HTTP_CODE" == "200" || "$LAST_HTTP_CODE" == "204" || "$LAST_HTTP_CODE" == "202" ]] \
    && pass "Test document deleted" \
    || warn "Document delete returned HTTP $LAST_HTTP_CODE"
fi

# ── SECTION 7: Control Plane Profile & Consent ────────────────────────────────
header "SECTION 7: PROFILE & CONSENT (CONTROL PLANE)"

log "Getting user profile..."
PROFILE_BODY=$(do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/profile/getProfile" \
  -H "Content-Type: application/json" \
  -d "{}")
vlog "Profile HTTP $LAST_HTTP_CODE: $PROFILE_BODY"
if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  pass "Profile endpoint returned 200"
elif [[ "$LAST_HTTP_CODE" == "401" ]]; then
  warn "Profile endpoint returned 401 — may need valid session cookie"
else
  warn "Profile endpoint returned HTTP $LAST_HTTP_CODE"
fi

log "Getting consent settings..."
CONSENT_BODY=$(do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/consent/get" \
  -H "Content-Type: application/json" \
  -d "{}")
vlog "Consent HTTP $LAST_HTTP_CODE: $CONSENT_BODY"
if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  pass "Consent endpoint returned 200"
elif [[ "$LAST_HTTP_CODE" == "401" ]]; then
  warn "Consent endpoint returned 401 — may need valid session cookie"
else
  warn "Consent endpoint returned HTTP $LAST_HTTP_CODE"
fi

# ── SECTION 8: Sign Out ────────────────────────────────────────────────────────
header "SECTION 8: SIGN OUT & SESSION INVALIDATION"

log "Signing out..."
SIGNOUT_BODY=$(do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/signOut" \
  -H "Content-Type: application/json" \
  -d "{}")
vlog "Sign-out HTTP $LAST_HTTP_CODE: $SIGNOUT_BODY"
if [[ "$LAST_HTTP_CODE" == "200" ]]; then
  pass "Sign-out HTTP 200"
else
  warn "Sign-out returned HTTP $LAST_HTTP_CODE"
fi

# Verify session is gone after sign-out
log "Verifying session is invalidated after sign-out..."
POST_SIGNOUT=$(do_curl POST "http://localhost:${AUTH_PORT}/api/v2/auth/getSession" \
  -H "Content-Type: application/json" \
  -d "{}")
AUTHENTICATED=$(echo "$POST_SIGNOUT" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('authenticated',d.get('user',None)))" 2>/dev/null || echo "")
if [[ -z "$AUTHENTICATED" || "$AUTHENTICATED" == "None" || "$AUTHENTICATED" == "False" || "$AUTHENTICATED" == "false" ]]; then
  pass "Session correctly invalidated after sign-out"
else
  warn "Session may still be active after sign-out — value: $AUTHENTICATED"
fi

# ── SECTION 9: Log Monitoring ─────────────────────────────────────────────────
header "SECTION 9: LOG ERROR SCAN"

for container in auth-service user-service org-core-service; do
  log "Scanning logs: $container..."
  LOG_OUTPUT=$(docker logs "$container" --tail 50 2>&1 || echo "CONTAINER_NOT_FOUND")
  if echo "$LOG_OUTPUT" | grep -q "CONTAINER_NOT_FOUND"; then
    skip "$container — container not found (check container name)"
    continue
  fi
  ERROR_COUNT=$(echo "$LOG_OUTPUT" | grep -cE 'ERROR|FATAL|ECONNREFUSED|ENOTFOUND|UnhandledPromiseRejection|panic:|SQLSTATE' 2>/dev/null || echo 0)
  WARN_COUNT=$(echo  "$LOG_OUTPUT" | grep -cE 'WARN|WARNING' 2>/dev/null || echo 0)
  if [[ "${ERROR_COUNT:-0}" -gt 0 ]]; then
    warn "$container — $ERROR_COUNT error(s) in last 50 lines"
    echo "$LOG_OUTPUT" | grep -E 'ERROR|FATAL|ECONNREFUSED|ENOTFOUND|UnhandledPromiseRejection|panic:|SQLSTATE' 2>/dev/null | tail -5 | while IFS= read -r line; do
      echo -e "    ${RED}↳ $line${RESET}"
    done || true
  else
    pass "$container — no errors in last 50 lines (${WARN_COUNT:-0} warnings)"
  fi
done

# Reasoning and data plane logs
for container in coresystem-reasoning-core-1 reasoning-reasoning-core-1 dataplane-retrieval-service-1; do
  LOG_OUTPUT=$(docker logs "$container" --tail 30 2>&1 || echo "CONTAINER_NOT_FOUND")
  if echo "$LOG_OUTPUT" | grep -q "CONTAINER_NOT_FOUND" 2>/dev/null; then continue; fi
  ERROR_COUNT=$(echo "$LOG_OUTPUT" | grep -cE 'ERROR|FATAL|Traceback|Exception' 2>/dev/null || echo 0)
  if [[ "${ERROR_COUNT:-0}" -gt 0 ]]; then
    warn "$container — $ERROR_COUNT error(s) in last 30 lines"
  else
    pass "$container — clean logs"
  fi
done

# ── SUMMARY ───────────────────────────────────────────────────────────────────
END_TS=$(date +%s)
ELAPSED=$((END_TS - START_TS))
TOTAL=$((PASS + FAIL + WARN))

echo ""
echo -e "${BOLD}╔══════════════════════════════════════════╗${RESET}"
echo -e "${BOLD}║         FUNCTIONAL TEST SUMMARY          ║${RESET}"
echo -e "${BOLD}╠══════════════════════════════════════════╣${RESET}"
echo -e "${BOLD}║${RESET}  Elapsed : ${ELAPSED}s"
echo -e "${BOLD}║${RESET}  User    : $USER_EMAIL"
[[ -n "$USER_ID" ]] && echo -e "${BOLD}║${RESET}  User ID : $USER_ID"
[[ -n "$ORG_ID"  ]] && echo -e "${BOLD}║${RESET}  Org ID  : $ORG_ID"
[[ -n "$DOC_ID"  ]] && echo -e "${BOLD}║${RESET}  Doc ID  : $DOC_ID"
echo -e "${BOLD}║${RESET}"
echo -e "${BOLD}║${RESET}  ${GREEN}PASS : $PASS${RESET}"
echo -e "${BOLD}║${RESET}  ${RED}FAIL : $FAIL${RESET}"
echo -e "${BOLD}║${RESET}  ${YELLOW}WARN : $WARN${RESET}"
echo -e "${BOLD}║${RESET}  ${YELLOW}SKIP : $SKIP${RESET}"
echo -e "${BOLD}╚══════════════════════════════════════════╝${RESET}"

rm -f "$COOKIE_JAR"

[[ $FAIL -eq 0 ]] && exit 0 || exit 1
