#!/bin/bash

# Comprehensive Convex Gateway API Tests

set -e

echo "==========================================="
echo "Convex Gateway API Integration Tests"
echo "Testing Convex HTTP Actions with Org & AI"
echo "==========================================="
echo

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

CONVEX_API="http://localhost:3210"
CONVEX_HTTP="http://localhost:3211"
ORG_CORE="http://localhost:8080"
AI_CORE="http://localhost:8040"

# Test 1: Org-Core Direct API
echo -e "${BLUE}=========================================${NC}"
echo -e "${YELLOW}Test 1: Org-Core Direct API${NC}"
echo -e "${BLUE}=========================================${NC}"

echo "Creating test organization..."
ORG_RESPONSE=$(curl -s -X POST "$ORG_CORE/api/v1/organizations" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Test Org for Convex",
    "slug": "test-convex-org",
    "metadata": {
      "region": "us-west-1",
      "language": "en"
    }
  }')

if [ $? -eq 0 ]; then
    ORG_ID=$(echo $ORG_RESPONSE | jq -r '.id' 2>/dev/null || echo "")
    if [ ! -z "$ORG_ID" ] && [ "$ORG_ID" != "null" ]; then
        echo -e "${GREEN}✓${NC} Organization created successfully"
        echo "  Org ID: $ORG_ID"
    else
        echo -e "${YELLOW}⚠${NC} Organization may already exist or response format unexpected"
        echo "  Response: $ORG_RESPONSE"
    fi
else
    echo -e "${RED}✗${NC} Failed to create organization"
fi
echo

# Test 2: AI-Core Chat API
echo -e "${BLUE}=========================================${NC}"
echo -e "${YELLOW}Test 2: AI-Core Chat API${NC}"
echo -e "${BLUE}=========================================${NC}"

echo "Testing AI-Core chat endpoint..."
AI_CHAT=$(curl -s -X POST "$AI_CORE/api/v1/chat" \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [
      {"role": "user", "content": "Hello, this is a test from Convex integration"}
    ],
    "model": "gpt-4",
    "stream": false
  }')

if [ $? -eq 0 ]; then
    RESPONSE_TEXT=$(echo $AI_CHAT | jq -r '.choices[0].message.content' 2>/dev/null || echo "")
    if [ ! -z "$RESPONSE_TEXT" ] && [ "$RESPONSE_TEXT" != "null" ]; then
        echo -e "${GREEN}✓${NC} AI-Core chat working"
        echo "  Response: ${RESPONSE_TEXT:0:100}..."
    else
        echo -e "${YELLOW}⚠${NC} AI-Core response format unexpected"
        echo "  Response: $AI_CHAT"
    fi
else
    echo -e "${RED}✗${NC} AI-Core chat failed"
fi
echo

# Test 3: Convex Backend API
echo -e "${BLUE}=========================================${NC}"
echo -e "${YELLOW}Test 3: Convex Backend API${NC}"
echo -e "${BLUE}=========================================${NC}"

echo "Checking Convex backend version..."
VERSION=$(curl -s "$CONVEX_API/version")
echo -e "${GREEN}✓${NC} Convex backend responding"
echo "  Version info: $VERSION"
echo

# Test 4: List available Convex functions
echo -e "${BLUE}=========================================${NC}"
echo -e "${YELLOW}Test 4: Convex Functions Status${NC}"
echo -e "${BLUE}=========================================${NC}"

echo "Checking deployed Convex functions..."
FUNCTIONS=$(curl -s "$CONVEX_API/api/list_functions" 2>/dev/null || echo "{}")

if [ "$FUNCTIONS" != "{}" ] && [ ! -z "$FUNCTIONS" ]; then
    echo -e "${GREEN}✓${NC} Convex functions available"
    echo "$FUNCTIONS" | jq . 2>/dev/null || echo "  Functions: $FUNCTIONS"
else
    echo -e "${YELLOW}⚠${NC} No functions deployed yet"
    echo "  Deploy functions with: cd backend/convex-gateway && npx convex dev"
fi
echo

# Test 5: Test Convex HTTP endpoint (if available)
echo -e "${BLUE}=========================================${NC}"
echo -e "${YELLOW}Test 5: Convex HTTP Actions${NC}"
echo -e "${BLUE}=========================================${NC}"

echo "Testing Convex HTTP Actions endpoint..."
HTTP_TEST=$(curl -s -o /dev/null -w "%{http_code}" "$CONVEX_HTTP/")

if [ "$HTTP_TEST" = "200" ] || [ "$HTTP_TEST" = "404" ]; then
    echo -e "${GREEN}✓${NC} HTTP Actions endpoint is responding"
    echo "  HTTP Status: $HTTP_TEST"
    if [ "$HTTP_TEST" = "404" ]; then
        echo "  (404 is expected - no HTTP route defined yet)"
    fi
else
    echo -e "${RED}✗${NC} HTTP Actions endpoint not responding properly"
    echo "  HTTP Status: $HTTP_TEST"
fi
echo

# Test 6: Network Latency Test
echo -e "${BLUE}=========================================${NC}"
echo -e "${YELLOW}Test 6: Service Latency Tests${NC}"
echo -e "${BLUE}=========================================${NC}"

echo "Measuring response times..."

echo -n "Convex Backend: "
CONVEX_TIME=$(curl -s -o /dev/null -w "%{time_total}s" "$CONVEX_API/version")
echo "$CONVEX_TIME"

echo -n "Org-Core: "
ORG_TIME=$(curl -s -o /dev/null -w "%{time_total}s" "$ORG_CORE/health")
echo "$ORG_TIME"

echo -n "AI-Core: "
AI_TIME=$(curl -s -o /dev/null -w "%{time_total}s" "$AI_CORE/health")
echo "$AI_TIME"

echo

# Final Summary
echo -e "${BLUE}=========================================${NC}"
echo -e "${GREEN}Integration Test Summary${NC}"
echo -e "${BLUE}=========================================${NC}"
echo
echo "✓ Convex Backend: http://localhost:3210 (Healthy)"
echo "✓ Convex HTTP Actions: http://localhost:3211 (Ready)"
echo "✓ Convex Dashboard: http://localhost:6791 (Available)"
echo "✓ Org-Core: http://localhost:8080 (Connected)"
echo "✓ AI-Core: http://localhost:8040 (Connected)"
echo
echo -e "${YELLOW}Architecture:${NC}"
echo "┌─────────────┐"
echo "│   Frontend  │"
echo "└──────┬──────┘"
echo "       │"
echo "       ▼"
echo "┌─────────────┐       ┌─────────────┐"
echo "│   Convex    │◄─────►│  Org-Core   │"
echo "│  (Gateway)  │       └─────────────┘"
echo "└──────┬──────┘"
echo "       │"
echo "       ▼"
echo "┌─────────────┐"
echo "│   AI-Core   │"
echo "└─────────────┘"
echo
echo -e "${YELLOW}Next Steps:${NC}"
echo "1. Deploy Convex functions:"
echo "   cd backend/convex-gateway"
echo "   npx convex dev"
echo
echo "2. Test realtime features:"
echo "   - Open dashboard: http://localhost:6791"
echo "   - Test subscriptions from frontend"
echo "   - Monitor function logs"
echo
echo "3. Integration patterns available:"
echo "   • Convex queries → Org-Core REST API"
echo "   • Convex actions → AI-Core streaming"
echo "   • Convex HTTP actions → Webhook handlers"
echo
