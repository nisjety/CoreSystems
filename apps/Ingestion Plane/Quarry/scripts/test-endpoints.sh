#!/bin/bash

# Quarry API Endpoint Testing Script
# Tests all endpoints with proper validation and timing

set -e

TARGET_URL="${1:-https://example.com}"
BASE_URL="http://localhost:9090"
API_KEY="dev-test-key-12345"

BOLD='\033[1m'
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BOLD}=== QUARRY API ENDPOINT TESTS ===${NC}"
echo -e "${BOLD}Target URL: $TARGET_URL${NC}\n"

# Test counter
PASSED=0
FAILED=0
TOTAL=0

# Helper function for testing
test_endpoint() {
    local name="$1"
    local method="$2"
    local endpoint="$3"
    local data="$4"
    local expected_status="$5"
    
    TOTAL=$((TOTAL + 1))
    echo -e "${BOLD}Test $TOTAL: $name${NC}"
    
    # Extract timing
    START_TIME=$(date +%s%N)
    
    if [ "$method" = "GET" ]; then
        RESPONSE=$(curl -s -w "\n%{http_code}" -H "X-API-Key: $API_KEY" "$BASE_URL$endpoint" || echo -e "\n503")
    else
        RESPONSE=$(curl -s -w "\n%{http_code}" -X "$method" -H "Content-Type: application/json" -H "X-API-Key: $API_KEY" -d "$data" "$BASE_URL$endpoint" || echo -e "\n503")
    fi
    
    END_TIME=$(date +%s%N)
    DURATION=$(( (END_TIME - START_TIME) / 1000000 ))
    
    HTTP_CODE=$(echo "$RESPONSE" | tail -n1)
    RESPONSE_BODY=$(echo "$RESPONSE" | sed '$d')
    
    if [[ "$expected_status" == *"$HTTP_CODE"* ]]; then
        echo -e "${GREEN}✓ PASSED${NC} (${DURATION}ms, HTTP $HTTP_CODE)"
        PASSED=$((PASSED + 1))
        if [ -n "$RESPONSE_BODY" ]; then
            # Try to pretty print if JSON
            if echo "$RESPONSE_BODY" | jq -e . >/dev/null 2>&1; then
                echo "$RESPONSE_BODY" | jq -C '.'
            else
                echo "$RESPONSE_BODY" | head -n 5
                [ $(echo "$RESPONSE_BODY" | wc -l) -gt 5 ] && echo "..."
            fi
        fi
    else
        echo -e "${RED}✗ FAILED${NC} (Expected $expected_status, got $HTTP_CODE)"
        FAILED=$((FAILED + 1))
        echo "Response: $RESPONSE_BODY"
    fi
    echo ""
}

test_assert() {
  local name="$1"
  local condition="$2"
  local details="$3"

  TOTAL=$((TOTAL + 1))
  echo -e "${BOLD}Test $TOTAL: $name${NC}"

  if eval "$condition"; then
    echo -e "${GREEN}✓ PASSED${NC}"
    PASSED=$((PASSED + 1))
  else
    echo -e "${RED}✗ FAILED${NC}"
    FAILED=$((FAILED + 1))
    echo "$details"
  fi
  echo ""
}

# 1. Health Check
test_endpoint "Health Check" "GET" "/health" "" "200"

# 2. Ready Check
test_endpoint "Readiness Check" "GET" "/ready" "" "200"

# 3. List Modules
test_endpoint "List Available Modules" "GET" "/v1/modules" "" "200"

# 4. Scrape Endpoint (Explicit Collection for robustness)
test_endpoint "Scrape - $TARGET_URL" "POST" "/v1/scrape" \
"{
  \"url\": \"$TARGET_URL\",
  \"collection\": \"quick\",
  \"maxAge\": 60000,
  \"formats\": [\"markdown\"]
}" "200"

# 5. Parameter Validation for Change Tracking
test_endpoint "Change Tracking Parameter Validation" "GET" "/v1/change/latest" "" "400"

# 6. Crawl Endpoint (Scheduled mode to avoid initiation timeout)
echo -e "${BOLD}Test $((TOTAL + 1)): Initiating Crawl Job (Scheduled Mode)...${NC}"
CRAWL_RESPONSE=$(curl -s -X POST -H "Content-Type: application/json" -H "X-API-Key: $API_KEY" \
-d "{
  \"url\": \"$TARGET_URL\",
  \"mode\": \"scheduled\",
  \"maxPages\": 2
}" "$BASE_URL/v1/crawl")

JOB_ID=$(echo "$CRAWL_RESPONSE" | jq -r '.job.id // .id // .jobId' 2>/dev/null || echo "")

if [ -n "$JOB_ID" ] && [ "$JOB_ID" != "null" ]; then
    TOTAL=$((TOTAL + 1))
    echo -e "Test $TOTAL: Crawl Initiation"
    echo -e "${GREEN}✓ PASSED${NC} (Job ID: $JOB_ID)"
    PASSED=$((PASSED + 1))
    echo "$CRAWL_RESPONSE" | jq -C '.'
    echo ""
    
    # 7. Get Job Status
    sleep 2
    test_endpoint "Get Job Status" "GET" "/v1/jobs/$JOB_ID" "" "200"
    
    # 8. SSE Stream Test (Heartbeat)
    TOTAL=$((TOTAL + 1))
    echo -e "${BOLD}Test $TOTAL: SSE Stream (heartbeat check)${NC}"
    echo "Connecting to stream..."
    # Capture first few events
    SSE_DATA=$(curl -N -s -H "Accept: text/event-stream" -H "X-API-Key: $API_KEY" "$BASE_URL/v1/jobs/$JOB_ID/stream" | head -n 5)
    
    if echo "$SSE_DATA" | grep -q "event:"; then
        echo -e "${GREEN}✓ PASSED${NC}"
        PASSED=$((PASSED + 1))
        echo -e "Captured events:\n$SSE_DATA"
    else
        echo -e "${RED}✗ FAILED${NC} (No SSE events received or connection closed)"
        FAILED=$((FAILED + 1))
        echo "Raw SSE Data: $SSE_DATA"
    fi
    echo ""
else
    TOTAL=$((TOTAL + 1))
    echo -e "${RED}✗ FAILED${NC} (No job ID returned for crawl)"
    FAILED=$((FAILED + 1))
    echo "Response: $CRAWL_RESPONSE"
    echo ""
fi

# Summary
echo -e "\n${BOLD}=== TEST SUMMARY ===${NC}"
echo -e "Total Tests: $TOTAL"
echo -e "${GREEN}Passed: $PASSED${NC}"
if [ $FAILED -gt 0 ]; then
    echo -e "${RED}Failed: $FAILED${NC}"
else
    echo -e "Failed: 0"
fi

SUCCESS_RATE=$(echo "scale=2; ($PASSED / $TOTAL) * 100" | bc)
echo -e "Success Rate: ${SUCCESS_RATE}%"

if [ $FAILED -eq 0 ]; then
    echo -e "\n${GREEN}${BOLD}ALL TESTS PASSED!${NC}"
    exit 0
else
    echo -e "\n${RED}${BOLD}SOME TESTS FAILED!${NC}"
    exit 1
fi
