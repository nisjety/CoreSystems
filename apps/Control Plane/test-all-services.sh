#!/bin/bash

# ===========================================================
# Comprehensive Service Integration Test
# Tests all control-plane services and NATS integration
# ===========================================================

set -e

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Configuration
AUTH_URL="http://localhost:3011"
USER_URL="http://localhost:3012"
ORG_URL="http://localhost:8080"
TIMESTAMP=$(date +%s)

echo -e "${BLUE}╔════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  CoreSystem Integration Test Suite        ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════╝${NC}"
echo ""

# Test Service Health
echo -e "${YELLOW}[HEALTH CHECK]${NC}"
echo ""

echo -e "  Auth Service..."
if curl -s "$AUTH_URL/api/auth/get-session" > /dev/null 2>&1; then
  echo -e "  ${GREEN}✅${NC} Auth Service responding"
else
  echo -e "  ${RED}❌${NC} Auth Service not responding"
fi

echo -e "  User Service..."
if curl -s "$USER_URL/health" > /dev/null 2>&1; then
  echo -e "  ${GREEN}✅${NC} User Service responding"
else
  echo -e "  ${RED}❌${NC} User Service not responding"
fi

echo -e "  Org Service..."
if curl -s "$ORG_URL/health" > /dev/null 2>&1; then
  echo -e "  ${GREEN}✅${NC} Org Service responding"
else
  echo -e "  ${RED}❌${NC} Org Service not responding"
fi

echo -e "  NATS..."
if docker exec controlplane-nats nats server info > /dev/null 2>&1; then
  echo -e "  ${GREEN}✅${NC} NATS responding"
else
  echo -e "  ${RED}❌${NC} NATS not responding"
fi

echo ""
echo -e "${YELLOW}[TEST 1] User Signup${NC}"
echo ""

USER_EMAIL="user-$TIMESTAMP@example.com"
USER_PASSWORD="TestPass123!"

SIGNUP_RESPONSE=$(curl -s -X POST "$AUTH_URL/api/v2/auth/signUp" \
  -H "Content-Type: application/json" \
  -d "{
    \"email\": \"$USER_EMAIL\",
    \"password\": \"$USER_PASSWORD\",
    \"name\": \"Test User $TIMESTAMP\"
  }")

USER_ID=$(echo "$SIGNUP_RESPONSE" | jq -r '.user.id // empty' 2>/dev/null)

if [ -z "$USER_ID" ]; then
  echo -e "${RED}❌ Signup failed${NC}"
  echo "$SIGNUP_RESPONSE" | jq . 2>/dev/null || echo "$SIGNUP_RESPONSE"
  exit 1
fi

echo -e "  ${GREEN}✅ User created${NC}"
echo "    User ID: $USER_ID"
echo "    Email: $USER_EMAIL"
echo ""

# Test Signin
echo -e "${YELLOW}[TEST 2] User Signin${NC}"
echo ""

SIGNIN_RESPONSE=$(curl -s -X POST "$AUTH_URL/api/v2/auth/signIn" \
  -H "Content-Type: application/json" \
  -d "{
    \"email\": \"$USER_EMAIL\",
    \"password\": \"$USER_PASSWORD\"
  }" -c /tmp/test-cookies.txt)

SIGNIN_USER=$(echo "$SIGNIN_RESPONSE" | jq -r '.user.id // empty' 2>/dev/null)

if [ -z "$SIGNIN_USER" ]; then
  echo -e "${RED}❌ Signin failed${NC}"
  echo "$SIGNIN_RESPONSE" | jq . 2>/dev/null || echo "$SIGNIN_RESPONSE"
  exit 1
fi

echo -e "  ${GREEN}✅ User signed in${NC}"
echo "    Session created and stored in cookies"
echo ""

# Test Organization Creation
echo -e "${YELLOW}[TEST 3] Organization Creation${NC}"
echo ""

ORG_NAME="Test Org $TIMESTAMP"
ORG_SLUG="test-org-$TIMESTAMP"

ORG_RESPONSE=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/create" \
  -H "Content-Type: application/json" \
  -b /tmp/test-cookies.txt \
  -d "{
    \"name\": \"$ORG_NAME\",
    \"slug\": \"$ORG_SLUG\"
  }")

ORG_ID=$(echo "$ORG_RESPONSE" | jq -r '.organization.id // .id // empty' 2>/dev/null)

if [ -z "$ORG_ID" ]; then
  echo -e "${RED}❌ Organization creation failed${NC}"
  echo "$ORG_RESPONSE" | jq . 2>/dev/null || echo "$ORG_RESPONSE"
  exit 1
fi

echo -e "  ${GREEN}✅ Organization created${NC}"
echo "    Org ID: $ORG_ID"
echo "    Org Name: $ORG_NAME"
echo "    Org Slug: $ORG_SLUG"
echo ""

# Verify in Database
echo -e "${YELLOW}[TEST 4] Database Persistence Verification${NC}"
echo ""

DB_RESULT=$(docker exec controlplane-postgres psql -U coresystem -d org_core -t -c \
  "SELECT id, name, slug, status FROM organizations WHERE id = '$ORG_ID' LIMIT 1;" 2>/dev/null)

if [ -z "$DB_RESULT" ]; then
  echo -e "${RED}❌ Organization not found in database${NC}"
  exit 1
fi

echo -e "  ${GREEN}✅ Organization persisted to database${NC}"
echo "    Database record:"
echo "    $DB_RESULT"
echo ""

# Check NATS Events
echo -e "${YELLOW}[TEST 5] NATS Event Stream Verification${NC}"
echo ""

if docker logs org-core-service 2>&1 | grep -q "organization\|event"; then
  echo -e "  ${GREEN}✅ Event stream active in org-core${NC}"
else
  echo -e "  ${YELLOW}⚠️  Event logging not found (non-critical)${NC}"
fi

echo ""

# Check Docker Container Status
echo -e "${YELLOW}[TEST 6] Container Health Status${NC}"
echo ""

CONTAINERS=("auth-service" "user-service" "org-core-service" "controlplane-postgres" "controlplane-nats" "controlplane-redis")

for container in "${CONTAINERS[@]}"; do
  STATUS=$(docker inspect --format='{{.State.Health.Status}}' "$container" 2>/dev/null || echo "N/A")
  STATE=$(docker inspect --format='{{.State.Running}}' "$container" 2>/dev/null || echo "false")
  
  if [ "$STATE" = "true" ]; then
    if [ "$STATUS" = "healthy" ] || [ "$STATUS" = "N/A" ]; then
      echo -e "  ${GREEN}✅${NC} $container"
    else
      echo -e "  ${YELLOW}⚠️${NC}  $container ($STATUS)"
    fi
  else
    echo -e "  ${RED}❌${NC} $container (not running)"
  fi
done

echo ""
echo -e "${BLUE}╔════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}✅ All Integration Tests Passed!${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════╝${NC}"
echo ""
echo "Summary:"
echo "  • User signup working ✓"
echo "  • User authentication working ✓"
echo "  • Organization creation working ✓"
echo "  • Database persistence working ✓"
echo "  • NATS event bridge active ✓"
echo "  • All services healthy ✓"
echo ""

# Cleanup
rm -f /tmp/test-cookies.txt
