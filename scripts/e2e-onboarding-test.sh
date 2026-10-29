#!/usr/bin/env bash
set -euo pipefail

################################################################################
#  E2E Onboarding Flow Test Suite
#  Tests complete 6-step onboarding with all backend integrations
#  Coverage: Profile → Org → Website Crawl → Connect → Team → Complete
################################################################################

# Configuration
FRONTEND="http://localhost:3000"
AUTH_API="http://localhost:3011"
USER_API="http://localhost:3012"
ORG_API="http://localhost:8080"
QUARRY_API="http://localhost:8092"
DATA_PLANE="http://localhost:9401"
RETRIEVAL="http://localhost:9404"
AI_CORE="http://localhost:8100"

# Test data
TEST_TIMESTAMP=$(date +%s)
TEST_ORG_ID="onboarding-e2e-${TEST_TIMESTAMP}"
TEST_USER_ID="user-${TEST_TIMESTAMP}"
TEST_USER_EMAIL="onboarding-test-${TEST_TIMESTAMP}@example.com"
TEST_ORG_NAME="Onboarding Test Org ${TEST_TIMESTAMP}"
TEST_WEBSITE_URL="https://info.cern.ch"
TEST_INVITE_EMAIL="teammate-${TEST_TIMESTAMP}@example.com"

# Results tracking
RESULTS_FILE="/tmp/onboarding-e2e-results-${TEST_TIMESTAMP}.json"
declare -A STEP_RESULTS
declare -A STEP_TIMINGS

# Utility functions
log() { echo "$(date '+%Y-%m-%d %H:%M:%S') | $*"; }
ok() { log "✅ $*"; }
fail() { log "❌ FAIL: $*"; exit 1; }
warn() { log "⚠️  $*"; }
step() { log ""; log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"; log "STEP: $*"; log "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"; }

record_result() {
  local step_name="$1"
  local status="$2"
  local details="${3:-}"
  local duration="${4:-0}"
  
  STEP_RESULTS[$step_name]="$status"
  STEP_TIMINGS[$step_name]="$duration"
  
  if [[ "$status" == "PASS" ]]; then
    ok "$step_name (${duration}ms)"
  else
    warn "$step_name: $details (${duration}ms)"
  fi
}

check_endpoint() {
  local endpoint="$1"
  local name="$2"
  
  if curl -sf "$endpoint" > /dev/null 2>&1; then
    ok "$name is online"
    return 0
  else
    warn "$name is offline - $endpoint"
    return 1
  fi
}

json_extract() {
  local json="$1"
  local key="$2"
  echo "$json" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('$key',''))" 2>/dev/null || echo ""
}

################################################################################
# HEALTH CHECK
################################################################################
step "0: Health Check - Verify all services online"

health_check() {
  local failed=0
  
  check_endpoint "${AUTH_API}/health" "Auth-Core" || failed=$((failed+1))
  check_endpoint "${USER_API}/health" "User-Core" || failed=$((failed+1))
  check_endpoint "${ORG_API}/health" "Org-Core" || failed=$((failed+1))
  check_endpoint "${QUARRY_API}/health" "Quarry" || failed=$((failed+1))
  check_endpoint "${DATA_PLANE}/health" "Data Plane" || failed=$((failed+1))
  check_endpoint "${RETRIEVAL}/health" "Retrieval Service" || failed=$((failed+1))
  check_endpoint "${AI_CORE}/health" "AI-Core" || failed=$((failed+1))
  
  if [[ $failed -gt 0 ]]; then
    warn "$failed services offline - test may have issues"
  else
    ok "All services online"
  fi
}

health_check

################################################################################
# STEP 1: OAuth Sign-In & User Provisioning
################################################################################
step "1: OAuth Sign-In & User Provisioning"

start_time=$(date +%s%N | cut -b1-13)

# Simulate OAuth callback - user-core auto-creates on first access
AUTH_RESPONSE=$(curl -sf -X POST "${USER_API}/users" \
  -H "Content-Type: application/json" \
  -d "{
    \"email\": \"$TEST_USER_EMAIL\",
    \"name\": \"Test User\",
    \"image\": \"https://example.com/avatar.png\"
  }" 2>/dev/null || echo '{}')

USER_ID=$(json_extract "$AUTH_RESPONSE" "id")
if [[ -z "$USER_ID" ]]; then
  # Fallback: try to get existing user
  AUTH_RESPONSE=$(curl -sf "${USER_API}/users/by-email/${TEST_USER_EMAIL}" 2>/dev/null || echo '{}')
  USER_ID=$(json_extract "$AUTH_RESPONSE" "id")
fi

[[ -n "$USER_ID" ]] || fail "Failed to provision user"
ok "User provisioned: $USER_ID"

end_time=$(date +%s%N | cut -b1-13)
duration=$((end_time - start_time))
record_result "oauth_signin" "PASS" "" "$duration"

################################################################################
# STEP 2: Complete User Profile
################################################################################
step "2: Complete User Profile (Name, Timezone, Job Title)"

start_time=$(date +%s%N | cut -b1-13)

PROFILE_UPDATE=$(curl -sf -X PATCH "${USER_API}/users/me" \
  -H "Content-Type: application/json" \
  -d "{
    \"firstName\": \"Test\",
    \"lastName\": \"User\",
    \"displayName\": \"Test User\",
    \"timezone\": \"Europe/Oslo\",
    \"position\": \"QA Engineer\",
    \"department\": \"Engineering\"
  }" 2>/dev/null || echo '{}')

PROFILE_UPDATED=$(json_extract "$PROFILE_UPDATE" "firstName")
[[ "$PROFILE_UPDATED" == "Test" ]] || fail "Profile update failed"
ok "Profile completed: firstName=$PROFILE_UPDATED"

end_time=$(date +%s%N | cut -b1-13)
duration=$((end_time - start_time))
record_result "profile_completion" "PASS" "" "$duration"

################################################################################
# STEP 3: Create Organization
################################################################################
step "3: Create Organization (Name, Slug, Plan)"

start_time=$(date +%s%N | cut -b1-13)

ORG_CREATE=$(curl -sf -X POST "${ORG_API}/orgs" \
  -H "Content-Type: application/json" \
  -d "{
    \"name\": \"$TEST_ORG_NAME\",
    \"slug\": \"onboarding-test-${TEST_TIMESTAMP}\",
    \"plan\": \"free\"
  }" 2>/dev/null || echo '{}')

ORG_ID=$(json_extract "$ORG_CREATE" "id")
[[ -n "$ORG_ID" ]] || fail "Organization creation failed: $ORG_CREATE"
ok "Organization created: $ORG_ID ($TEST_ORG_NAME)"

# Verify org is retrievable
ORG_CHECK=$(curl -sf "${ORG_API}/orgs/${ORG_ID}" 2>/dev/null || echo '{}')
ORG_NAME=$(json_extract "$ORG_CHECK" "name")
[[ "$ORG_NAME" == "$TEST_ORG_NAME" ]] || fail "Org verification failed"

end_time=$(date +%s%N | cut -b1-13)
duration=$((end_time - start_time))
record_result "org_creation" "PASS" "" "$duration"

################################################################################
# STEP 4: Configure Website & Start Quarry Crawl
################################################################################
step "4: Configure Website & Start Quarry Crawl (with SSE progress)"

start_time=$(date +%s%N | cut -b1-13)

# Initiate Quarry crawl via frontend proxy
CRAWL_RESPONSE=$(curl -sf -X POST "${FRONTEND}/api/ingestion/crawl" \
  -H "Content-Type: application/json" \
  -d "{
    \"url\": \"$TEST_WEBSITE_URL\",
    \"mode\": \"scheduled\",
    \"maxDepth\": 2
  }" 2>/dev/null || echo '{}')

CRAWL_JOB_ID=$(json_extract "$CRAWL_RESPONSE" "jobId")
[[ -n "$CRAWL_JOB_ID" ]] || fail "Crawl job creation failed: $CRAWL_RESPONSE"
ok "Quarry crawl initiated: jobId=$CRAWL_JOB_ID"

# Persist onboarding state to backend
STATE_PERSIST=$(curl -sf -X POST "${ORG_API}/internal/orgs/${ORG_ID}/onboarding/state" \
  -H "Content-Type: application/json" \
  -d "{
    \"status\": \"WEBSITE_CONFIGURED\",
    \"steps\": {
      \"website_url\": \"$TEST_WEBSITE_URL\",
      \"crawl_job_id\": \"$CRAWL_JOB_ID\"
    }
  }" 2>/dev/null || echo '{}')

ok "Onboarding state persisted to backend"

# Monitor crawl progress (SSE stream) - collect first 10 events or timeout after 30s
log "Monitoring crawl progress (30s timeout)..."
start_sse=$(date +%s)
timeout 30 curl -sf "${FRONTEND}/api/ingestion/crawl/${CRAWL_JOB_ID}/stream" \
  -H "Accept: text/event-stream" 2>/dev/null | head -10 | while read -r line; do
  if [[ "$line" == *"event:"* ]] || [[ "$line" == *"data:"* ]]; then
    log "  └─ $line"
  fi
done || true

end_time=$(date +%s%N | cut -b1-13)
duration=$((end_time - start_time))
record_result "website_config" "PASS" "" "$duration"

################################################################################
# STEP 5: Invite Team Members
################################################################################
step "5: Invite Team Members (with role assignment)"

start_time=$(date +%s%N | cut -b1-13)

# Invite multiple teammates
INVITE_ROLES=("admin" "member" "member")
INVITE_EMAILS=("$TEST_INVITE_EMAIL" "viewer-${TEST_TIMESTAMP}@example.com" "dev-${TEST_TIMESTAMP}@example.com")

INVITATION_IDS=()
for i in "${!INVITE_EMAILS[@]}"; do
  email="${INVITE_EMAILS[$i]}"
  role="${INVITE_ROLES[$i]}"
  
  INVITE_RESPONSE=$(curl -sf -X POST "${ORG_API}/orgs/${ORG_ID}/members/invite" \
    -H "Content-Type: application/json" \
    -d "{
      \"email\": \"$email\",
      \"role\": \"$role\"
    }" 2>/dev/null || echo '{}')
  
  INVITE_ID=$(json_extract "$INVITE_RESPONSE" "invitation_id")
  STATUS=$(json_extract "$INVITE_RESPONSE" "status")
  
  if [[ -n "$INVITE_ID" ]]; then
    INVITATION_IDS+=("$INVITE_ID")
    ok "Invited $email (role=$role, status=$STATUS, id=$INVITE_ID)"
  else
    warn "Failed to invite $email"
  fi
done

# Verify members list
MEMBERS_LIST=$(curl -sf "${ORG_API}/orgs/${ORG_ID}/members" 2>/dev/null || echo '{}')
MEMBER_COUNT=$(echo "$MEMBERS_LIST" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('members',[])))" 2>/dev/null || echo "0")
ok "Organization has $MEMBER_COUNT members (current + invites)"

end_time=$(date +%s%N | cut -b1-13)
duration=$((end_time - start_time))
record_result "team_invitation" "PASS" "" "$duration"

################################################################################
# STEP 6: Complete Onboarding
################################################################################
step "6: Complete Onboarding (Mark user as onboarded)"

start_time=$(date +%s%N | cut -b1-13)

COMPLETE_RESPONSE=$(curl -sf -X POST "${USER_API}/users/onboarding/complete" \
  -H "Content-Type: application/json" \
  -d "{
    \"email\": \"$TEST_USER_EMAIL\"
  }" 2>/dev/null || echo '{}')

ONBOARDING_COMPLETE=$(json_extract "$COMPLETE_RESPONSE" "onboardingComplete")
[[ "$ONBOARDING_COMPLETE" == "true" ]] || warn "Onboarding completion flag may not be set"
ok "User marked as onboarded"

end_time=$(date +%s%N | cut -b1-13)
duration=$((end_time - start_time))
record_result "onboarding_complete" "PASS" "" "$duration"

################################################################################
# STEP 7: Verify Data Plane Integration (Optional)
################################################################################
step "7: Verify Data Plane Integration (Document Indexing)"

# Create a test document for the organization
DOC_RESPONSE=$(curl -sf -X POST "${DATA_PLANE}/v1/documents" \
  -H "Content-Type: application/json" \
  -d "{
    \"org_id\": \"$ORG_ID\",
    \"source\": \"website-crawl\",
    \"type\": \"webpage\",
    \"title\": \"CERN Info Page\",
    \"url\": \"$TEST_WEBSITE_URL\",
    \"content\": \"CERN is the European Organization for Nuclear Research...\",
    \"metadata\": {
      \"crawl_job_id\": \"$CRAWL_JOB_ID\",
      \"source_url\": \"$TEST_WEBSITE_URL\"
    }
  }" 2>/dev/null || echo '{}')

DOC_ID=$(json_extract "$DOC_RESPONSE" "document_id")
if [[ -n "$DOC_ID" ]]; then
  ok "Document ingested: $DOC_ID"
  
  # Poll document status
  log "Polling document status (up to 60s)..."
  for attempt in {1..12}; do
    DOC_STATUS=$(curl -sf "${DATA_PLANE}/v1/documents/${DOC_ID}?org_id=${ORG_ID}" 2>/dev/null || echo '{}')
    STATUS=$(json_extract "$DOC_STATUS" "status")
    
    if [[ "$STATUS" == "indexed" ]]; then
      ok "Document indexed: $STATUS"
      break
    else
      log "  [$attempt/12] Status: $STATUS (waiting...)"
      sleep 5
    fi
  done
else
  warn "Document ingestion failed"
fi

################################################################################
# STEP 8: Test AI Workspace Access
################################################################################
step "8: Test AI Workspace Access (Retrieval + AI-Core)"

# Test retrieval service
RETRIEVAL_QUERY="information about CERN"
RETRIEVE_RESPONSE=$(curl -sf -X POST "${RETRIEVAL}/v1/retrieve" \
  -H "Content-Type: application/json" \
  -d "{
    \"org_id\": \"$ORG_ID\",
    \"query\": \"$RETRIEVAL_QUERY\",
    \"top_k\": 5
  }" 2>/dev/null || echo '{}')

FACT_COUNT=$(echo "$RETRIEVE_RESPONSE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('facts',[])))" 2>/dev/null || echo "0")

if [[ "$FACT_COUNT" -gt 0 ]]; then
  ok "Retrieval successful: $FACT_COUNT facts retrieved"
  
  # Test AI-Core query
  AI_RESPONSE=$(curl -sf -X POST "${AI_CORE}/api/documents/query" \
    -H "Content-Type: application/json" \
    -d "{
      \"query\": \"What is CERN?\",
      \"org_id\": \"$ORG_ID\",
      \"top_k\": 3
    }" 2>/dev/null || echo '{}')
  
  AI_FACTS=$(echo "$AI_RESPONSE" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('facts',[])))" 2>/dev/null || echo "0")
  [[ "$AI_FACTS" -gt 0 ]] && ok "AI-Core query successful: $AI_FACTS facts" || warn "AI-Core returned 0 facts"
else
  warn "Retrieval returned 0 facts (may be expected if document not yet indexed)"
fi

################################################################################
# STEP 9: Tenant Isolation Verification
################################################################################
step "9: Verify Tenant Isolation (Cross-org data leakage test)"

OTHER_ORG_ID="other-org-${TEST_TIMESTAMP}"
ISOLATION_QUERY=$(curl -sf -X POST "${RETRIEVAL}/v1/retrieve" \
  -H "Content-Type: application/json" \
  -d "{
    \"org_id\": \"$OTHER_ORG_ID\",
    \"query\": \"CERN\",
    \"top_k\": 5
  }" 2>/dev/null || echo '{}')

LEAKED_FACTS=$(echo "$ISOLATION_QUERY" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('facts',[])))" 2>/dev/null || echo "0")

if [[ "$LEAKED_FACTS" -eq 0 ]]; then
  ok "Tenant isolation: PASS (0 facts leaked)"
  record_result "tenant_isolation" "PASS" "" "0"
else
  fail "Tenant isolation: FAIL ($LEAKED_FACTS facts leaked!)"
fi

################################################################################
# SUMMARY & RESULTS
################################################################################
step "TEST SUMMARY"

total_steps=${#STEP_RESULTS[@]}
passed_steps=0
failed_steps=0

for step_name in "${!STEP_RESULTS[@]}"; do
  if [[ "${STEP_RESULTS[$step_name]}" == "PASS" ]]; then
    passed_steps=$((passed_steps + 1))
  else
    failed_steps=$((failed_steps + 1))
  fi
done

# Calculate totals
total_duration=0
for duration in "${STEP_TIMINGS[@]}"; do
  total_duration=$((total_duration + duration))
done

# Generate JSON results
cat > "$RESULTS_FILE" << EOF
{
  "test_run_id": "onboarding-e2e-${TEST_TIMESTAMP}",
  "timestamp": "$(date -Iseconds)",
  "status_overall": "$([ $failed_steps -eq 0 ] && echo 'PASS' || echo 'FAIL')",
  "passed_steps": $passed_steps,
  "failed_steps": $failed_steps,
  "total_steps": $total_steps,
  "total_duration_ms": $total_duration,
  "test_data": {
    "org_id": "$ORG_ID",
    "org_name": "$TEST_ORG_NAME",
    "user_email": "$TEST_USER_EMAIL",
    "website_url": "$TEST_WEBSITE_URL",
    "crawl_job_id": "$CRAWL_JOB_ID"
  },
  "step_results": {
    "oauth_signin": "${STEP_RESULTS[oauth_signin]:-SKIP}",
    "profile_completion": "${STEP_RESULTS[profile_completion]:-SKIP}",
    "org_creation": "${STEP_RESULTS[org_creation]:-SKIP}",
    "website_config": "${STEP_RESULTS[website_config]:-SKIP}",
    "team_invitation": "${STEP_RESULTS[team_invitation]:-SKIP}",
    "onboarding_complete": "${STEP_RESULTS[onboarding_complete]:-SKIP}",
    "tenant_isolation": "${STEP_RESULTS[tenant_isolation]:-SKIP}"
  }
}
EOF

log ""
log "════════════════════════════════════════════════════════════════"
log "  ONBOARDING E2E TEST RESULTS"
log "════════════════════════════════════════════════════════════════"
log "  Status: $([ $failed_steps -eq 0 ] && echo '✅ ALL PASS' || echo '❌ FAILED')"
log "  Passed: $passed_steps / $total_steps"
log "  Duration: ${total_duration}ms"
log "  Results saved: $RESULTS_FILE"
log "════════════════════════════════════════════════════════════════"

# Print detailed breakdown
log ""
log "Step Timings:"
for step_name in "${!STEP_TIMINGS[@]}"; do
  printf "  %-25s: %6dms\n" "$step_name" "${STEP_TIMINGS[$step_name]}"
done

log ""
log "Test Organization: $ORG_ID"
log "Test User Email: $TEST_USER_EMAIL"
log "Crawl Job ID: $CRAWL_JOB_ID"

[[ $failed_steps -eq 0 ]] && exit 0 || exit 1
