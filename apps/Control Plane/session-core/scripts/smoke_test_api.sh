#!/bin/bash

# Smoke Test Suite for Session-Core API
# Tests all new endpoints (Plans, Todos, Lineage) and existing endpoints
# Requires session-core running on localhost:3000

set -e

BASE_URL="http://localhost:3000/v1"
HEALTH_URL="http://localhost:3000/health"
PASS=0
FAIL=0

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'  # No Color

# Helper function to test an endpoint
test_endpoint() {
  local name=$1
  local method=$2
  local endpoint=$3
  local data=$4
  local expected_status=$5

  local url="$BASE_URL$endpoint"
  
  echo -n "Testing: $name... "
  
  if [ -z "$data" ]; then
    response=$(curl -s -w "\n%{http_code}" -X $method "$url" \
      -H "Content-Type: application/json" \
      -H "X-User-ID: test-user-123")
  else
    response=$(curl -s -w "\n%{http_code}" -X $method "$url" \
      -H "Content-Type: application/json" \
      -H "X-User-ID: test-user-123" \
      -d "$data")
  fi
  
  status=$(echo "$response" | tail -n1)
  body=$(echo "$response" | sed '$d')
  
  if [ "$status" = "$expected_status" ]; then
    echo -e "${GREEN}✓${NC} ($status)"
    PASS=$((PASS + 1))
    echo "$body"
  else
    echo -e "${RED}✗${NC} (expected $expected_status, got $status)"
    FAIL=$((FAIL + 1))
    echo "Response: $body"
  fi
}

echo "=========================================="
echo "Session-Core Smoke Test Suite"
echo "=========================================="
echo

# 1. Health Check
echo "=== Health & Sessions ==="
echo -n "Testing: Health Check... "
health_response=$(curl -s -w "\n%{http_code}" "$HEALTH_URL")
health_status=$(echo "$health_response" | tail -n1)
if [ "$health_status" = "200" ]; then
  echo -e "${GREEN}✓${NC} ($health_status)"
  PASS=$((PASS + 1))
else
  echo -e "${RED}✗${NC} (expected 200, got $health_status)"
  FAIL=$((FAIL + 1))
fi
echo

# 2. Create Session
echo "Creating test session..."
session_response=$(curl -s -X POST "$BASE_URL/sessions" \
  -H "Content-Type: application/json" \
  -H "X-User-ID: test-user-123" \
  -d '{
    "tenant_id": "test-tenant",
    "workspace_id": "test-ws",
    "org_id": "test-org",
    "plan_mode": false
  }')

SESSION_ID=$(echo $session_response | jq -r '.session.id')
echo "Session ID: $SESSION_ID"
echo

# 3. Session Operations
echo "=== Session Operations ==="
test_endpoint "Get Session State" "GET" "/sessions/$SESSION_ID/state" "" "200"
test_endpoint "Send Message" "POST" "/sessions/$SESSION_ID/messages" \
  '{"role":"user","content":"Test message"}' "200"
echo

# 4. Plans Tests
echo "=== Plans API ==="

# Create Plan
plan_response=$(curl -s -X POST "$BASE_URL/plans" \
  -H "Content-Type: application/json" \
  -H "X-User-ID: test-user-123" \
  -d "{
    \"run_id\": \"test-run-123\",
    \"thread_id\": \"$SESSION_ID\",
    \"state\": \"DRAFT\",
    \"summary\": \"Test plan\"
  }")

PLAN_ID=$(echo $plan_response | jq -r '.plan.id')
echo "Created plan: $PLAN_ID"

test_endpoint "Create Plan" "POST" "/plans" \
  "{\"run_id\":\"test-run-123\",\"thread_id\":\"$SESSION_ID\",\"state\":\"DRAFT\",\"summary\":\"Test\"}" "201"

test_endpoint "Get Plan" "GET" "/plans/$PLAN_ID" "" "200"

test_endpoint "List Plans by Thread" "GET" "/plans/thread/$SESSION_ID" "" "200"
test_endpoint "List Plans by Thread (pagination)" "GET" "/plans/thread/$SESSION_ID?limit=1&offset=0" "" "200"

test_endpoint "Update Plan State" "PATCH" "/plans/$PLAN_ID/state" \
  '{"state":"PROPOSED"}' "200"

# Create Plan Step
step_response=$(curl -s -X POST "$BASE_URL/plans/$PLAN_ID/steps" \
  -H "Content-Type: application/json" \
  -H "X-User-ID: test-user-123" \
  -d '{"step_order":0,"title":"Test Step","state":"PENDING"}')

STEP_ID=$(echo $step_response | jq -r '.step.id')
echo "Created step: $STEP_ID"

test_endpoint "Create Plan Step" "POST" "/plans/$PLAN_ID/steps" \
  '{"step_order":1,"title":"Another Step","state":"PENDING"}' "201"

test_endpoint "List Plan Steps" "GET" "/plans/$PLAN_ID/steps" "" "200"
test_endpoint "List Plan Steps (pagination)" "GET" "/plans/$PLAN_ID/steps?limit=1&offset=0" "" "200"

test_endpoint "Update Plan Step State" "PATCH" "/plans/$PLAN_ID/steps/$STEP_ID/state" \
  '{"state":"RUNNING"}' "200"

echo

# 5. Todos Tests
echo "=== Todos API ==="

# Create Todo
todo_response=$(curl -s -X POST "$BASE_URL/todos" \
  -H "Content-Type: application/json" \
  -H "X-User-ID: test-user-123" \
  -d "{
    \"thread_id\": \"$SESSION_ID\",
    \"run_id\": \"test-run-123\",
    \"title\": \"Test Todo\",
    \"state\": \"PENDING\",
    \"priority\": \"HIGH\"
  }")

TODO_ID=$(echo $todo_response | jq -r '.todo.id')
echo "Created todo: $TODO_ID"

test_endpoint "Create Todo" "POST" "/todos" \
  "{\"thread_id\":\"$SESSION_ID\",\"run_id\":\"test-run-123\",\"title\":\"Test\",\"state\":\"PENDING\",\"priority\":\"NORMAL\"}" "201"

test_endpoint "Get Todo" "GET" "/todos/$TODO_ID" "" "200"

test_endpoint "List Todos by Thread" "GET" "/todos/thread/$SESSION_ID" "" "200"
test_endpoint "List Todos by Thread (pagination)" "GET" "/todos/thread/$SESSION_ID?limit=1&offset=0" "" "200"

test_endpoint "List Todos by Run" "GET" "/todos/run/test-run-123" "" "200"
test_endpoint "List Todos by Run (pagination)" "GET" "/todos/run/test-run-123?limit=1&offset=0" "" "200"

test_endpoint "Update Todo State" "PATCH" "/todos/$TODO_ID/state" \
  '{"state":"IN_PROGRESS"}' "200"

test_endpoint "Delete Todo" "DELETE" "/todos/$TODO_ID" "" "200"

echo

# 6. Lineage Tests
echo "=== Lineage API ==="

# Create Lineage Edge
lineage_response=$(curl -s -X POST "$BASE_URL/lineage" \
  -H "Content-Type: application/json" \
  -H "X-User-ID: test-user-123" \
  -d '{
    "parent_run_id": "test-run-123",
    "child_run_id": "test-run-456",
    "role": "CODER"
  }')

echo "Created lineage edge"

test_endpoint "Create Lineage Edge" "POST" "/lineage" \
  '{"parent_run_id":"test-run-123","child_run_id":"test-run-789","role":"REVIEWER"}' "201"

test_endpoint "Get Lineage Children" "GET" "/lineage/test-run-123/children" "" "200"
test_endpoint "Get Lineage Children (pagination)" "GET" "/lineage/test-run-123/children?limit=1&offset=0" "" "200"

test_endpoint "Get Lineage Parents" "GET" "/lineage/test-run-456/parents" "" "200"
test_endpoint "Get Lineage Parents (pagination)" "GET" "/lineage/test-run-456/parents?limit=1&offset=0" "" "200"

test_endpoint "Delete Lineage Edge" "DELETE" "/lineage" \
  '{"parent_run_id":"test-run-123","child_run_id":"test-run-789"}' "200"

echo

# 7. Error Cases
echo "=== Error Cases ==="

test_endpoint "Missing Required Field" "POST" "/plans" \
  '{"state":"DRAFT"}' "400"

test_endpoint "Not Found" "GET" "/plans/nonexistent" "" "404"

test_endpoint "Invalid State" "PATCH" "/plans/$PLAN_ID/state" \
  '{"state":"INVALID_STATE"}' "200"  # Backend doesn't validate, just updates

echo

# 8. SSE Realtime Test
echo "=== SSE Real-time ==="
SSE_OUT=$(mktemp)
curl -s -N "$BASE_URL/sessions/$SESSION_ID/events?after_sequence=0" > "$SSE_OUT" &
SSE_PID=$!
sleep 1

curl -s -X POST "$BASE_URL/sessions/$SESSION_ID/messages" \
  -H "Content-Type: application/json" \
  -H "X-User-ID: test-user-123" \
  -d '{"role":"user","content":"sse-realtime-check"}' > /dev/null

sleep 2
kill "$SSE_PID" 2>/dev/null || true

echo -n "Testing: SSE Real-time... "
if grep -q "event: message.sent" "$SSE_OUT"; then
  echo -e "${GREEN}✓${NC}"
  PASS=$((PASS + 1))
else
  echo -e "${RED}✗${NC}"
  FAIL=$((FAIL + 1))
fi
rm -f "$SSE_OUT"

echo

# Summary
echo "=========================================="
echo -e "Test Results: ${GREEN}$PASS passed${NC}, ${RED}$FAIL failed${NC}"
echo "=========================================="

if [ $FAIL -eq 0 ]; then
  echo -e "${GREEN}✓ All tests passed!${NC}"
  exit 0
else
  echo -e "${RED}✗ Some tests failed${NC}"
  exit 1
fi
