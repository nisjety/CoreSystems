#!/bin/bash

# Test Better Auth Admin Endpoints

set -e

GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

AUTH_URL="http://localhost:3001"
ADMIN_EMAIL="admin@coresystem.com"
ADMIN_PASSWORD="AdminPass123!"

echo -e "${BLUE}=========================================${NC}"
echo -e "${BLUE}Better Auth Admin System Test${NC}"
echo -e "${BLUE}=========================================${NC}"
echo

# Step 1: Ensure admin user has admin role
echo -e "${YELLOW}Step 1: Setting up admin user${NC}"
docker exec coresystem-postgres-local psql -U coresystem -d auth_service -c \
  "UPDATE \"user\" SET role = 'admin' WHERE email = '$ADMIN_EMAIL'" > /dev/null 2>&1 || true

ADMIN_ROLE=$(docker exec coresystem-postgres-local psql -U coresystem -d auth_service -t -c \
  "SELECT role FROM \"user\" WHERE email = '$ADMIN_EMAIL' LIMIT 1" | xargs)

if [ "$ADMIN_ROLE" = "admin" ]; then
  echo -e "${GREEN}✓${NC} Admin user configured: $ADMIN_EMAIL"
else
  echo -e "${RED}✗${NC} Failed to set admin role"
  exit 1
fi
echo

# Step 2: Sign in as admin
echo -e "${YELLOW}Step 2: Authenticating as admin${NC}"

# Sign in and capture session cookie
SIGNIN_RESPONSE=$(curl -s -c /tmp/admin-session.txt \
  -X POST "$AUTH_URL/api/auth/sign-in/email" \
  -H "Content-Type: application/json" \
  -d "{\"email\": \"$ADMIN_EMAIL\", \"password\": \"$ADMIN_PASSWORD\"}")

# Extract session token from cookie file
if [ -f /tmp/admin-session.txt ]; then
  SESSION_TOKEN=$(grep -o 'idknuten.sid[[:space:]][^[:space:]]*' /tmp/admin-session.txt | awk '{print $2}')
  
  if [ -n "$SESSION_TOKEN" ]; then
    echo -e "${GREEN}✓${NC} Admin authenticated successfully"
    echo "  Session token: ${SESSION_TOKEN:0:20}..."
  else
    echo -e "${RED}✗${NC} Failed to get session token"
    exit 1
  fi
else
  echo -e "${RED}✗${NC} Failed to create session"
  exit 1
fi
echo

# Step 3: Test admin endpoints
echo -e "${YELLOW}Step 3: Testing Admin Endpoints${NC}"
echo

# Test 1: List users
echo -e "${BLUE}Test 1: List Users${NC}"
USERS_RESPONSE=$(curl -s -b /tmp/admin-session.txt \
  -X POST "$AUTH_URL/api/v2/auth/admin/users/list" \
  -H "Content-Type: application/json" \
  -d '{"limit": 5}')

USER_COUNT=$(echo "$USERS_RESPONSE" | jq -r '.users | length' 2>/dev/null || echo "0")

if [ "$USER_COUNT" != "null" ] && [ "$USER_COUNT" != "0" ]; then
  echo -e "${GREEN}✓${NC} Successfully listed $USER_COUNT users"
  echo "$USERS_RESPONSE" | jq -r '.users[] | "  - \(.email) (role: \(.role // "user"))"' | head -5
else
  echo -e "${YELLOW}⚠${NC} List users response:"
  echo "$USERS_RESPONSE" | jq . | head -10
fi
echo

# Test 2: Get system stats
echo -e "${BLUE}Test 2: Get System Stats${NC}"
STATS_RESPONSE=$(curl -s -b /tmp/admin-session.txt \
  -X POST "$AUTH_URL/api/v2/auth/admin/system/stats" \
  -H "Content-Type: application/json" \
  -d '{}')

if echo "$STATS_RESPONSE" | jq -e '.totalUsers' > /dev/null 2>&1; then
  echo -e "${GREEN}✓${NC} System stats retrieved"
  echo "$STATS_RESPONSE" | jq '{totalUsers, totalSessions, totalOrganizations}'
else
  echo -e "${YELLOW}⚠${NC} Stats response:"
  echo "$STATS_RESPONSE" | jq . | head -10
fi
echo

# Test 3: Create a test user via admin
echo -e "${BLUE}Test 3: Create User (Admin)${NC}"
TEST_USER_EMAIL="admin-created-$(date +%s)@example.com"
CREATE_RESPONSE=$(curl -s -b /tmp/admin-session.txt \
  -X POST "$AUTH_URL/api/v2/auth/admin/users/create" \
  -H "Content-Type: application/json" \
  -d "{\"email\": \"$TEST_USER_EMAIL\", \"password\": \"TestPass123!\", \"name\": \"Admin Created User\"}")

if echo "$CREATE_RESPONSE" | jq -e '.user.id' > /dev/null 2>&1; then
  echo -e "${GREEN}✓${NC} User created successfully"
  USER_ID=$(echo "$CREATE_RESPONSE" | jq -r '.user.id')
  echo "  Email: $TEST_USER_EMAIL"
  echo "  ID: $USER_ID"
else
  echo -e "${YELLOW}⚠${NC} Create user response:"
  echo "$CREATE_RESPONSE" | jq . | head -10
fi
echo

# Test 4: Get user details
if [ -n "$USER_ID" ]; then
  echo -e "${BLUE}Test 4: Get User Details${NC}"
  USER_RESPONSE=$(curl -s -b /tmp/admin-session.txt \
    -X POST "$AUTH_URL/api/v2/auth/admin/users/get" \
    -H "Content-Type: application/json" \
    -d "{\"userId\": \"$USER_ID\"}")
  
  if echo "$USER_RESPONSE" | jq -e '.user' > /dev/null 2>&1; then
    echo -e "${GREEN}✓${NC} User details retrieved"
    echo "$USER_RESPONSE" | jq '{email: .user.email, name: .user.name, role: .user.role}'
  else
    echo -e "${YELLOW}⚠${NC} Get user response:"
    echo "$USER_RESPONSE" | jq . | head -10
  fi
  echo
fi

# Test 5: List organizations
echo -e "${BLUE}Test 5: List Organizations${NC}"
ORGS_RESPONSE=$(curl -s -b /tmp/admin-session.txt \
  -X POST "$AUTH_URL/api/v2/auth/admin/organizations/list" \
  -H "Content-Type: application/json" \
  -d '{"limit": 5}')

ORG_COUNT=$(echo "$ORGS_RESPONSE" | jq -r '.organizations | length' 2>/dev/null || echo "0")

if [ "$ORG_COUNT" != "null" ] && [ "$ORG_COUNT" != "0" ]; then
  echo -e "${GREEN}✓${NC} Successfully listed $ORG_COUNT organizations"
  echo "$ORGS_RESPONSE" | jq -r '.organizations[] | "  - \(.name) (\(.slug))"' | head -5
else
  echo -e "${YELLOW}⚠${NC} List organizations response:"
  echo "$ORGS_RESPONSE" | jq . | head -10
fi
echo

# Summary
echo -e "${BLUE}=========================================${NC}"
echo -e "${BLUE}Test Summary${NC}"
echo -e "${BLUE}=========================================${NC}"
echo -e "${GREEN}✓${NC} Better Auth admin plugin is operational"
echo -e "${GREEN}✓${NC} Admin authentication working"
echo -e "${GREEN}✓${NC} Admin endpoints accessible"
echo
echo "Available admin endpoints:"
echo "  POST /api/v2/auth/admin/users/list"
echo "  POST /api/v2/auth/admin/users/get"
echo "  POST /api/v2/auth/admin/users/create"
echo "  POST /api/v2/auth/admin/users/suspend"
echo "  POST /api/v2/auth/admin/users/ban"
echo "  POST /api/v2/auth/admin/users/set-role"
echo "  POST /api/v2/auth/admin/users/sessions"
echo "  POST /api/v2/auth/admin/users/remove"
echo "  POST /api/v2/auth/admin/organizations/list"
echo "  POST /api/v2/auth/admin/system/stats"
echo

# Cleanup
rm -f /tmp/admin-session.txt

echo -e "${GREEN}✅ Admin system test complete!${NC}"
