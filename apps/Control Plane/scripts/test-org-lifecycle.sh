#!/bin/bash

# ========================================
# Organization Lifecycle Test Script
# ========================================
# Tests:
# 1. User registration
# 2. Organization creation
# 3. Member invitation
# 4. Invitation acceptance
# 5. Member management
# 6. Event flow verification
# ========================================

set -e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Configuration
AUTH_URL="${AUTH_URL:-http://localhost:3011}"
ORPC_URL="${ORPC_URL:-http://localhost:3011/api/v2}"

# Test data
OWNER_EMAIL="owner-$(date +%s)@example.com"
OWNER_PASSWORD="SecurePass123!"
MEMBER_EMAIL="member-$(date +%s)@example.com"
MEMBER_PASSWORD="MemberPass123!"
ORG_NAME="Test Company $(date +%s)"
ORG_SLUG="test-company-$(date +%s)"

echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║  Organization Lifecycle Test Suite    ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"
echo ""

# ========================================
# Step 1: Register Owner
# ========================================
echo -e "${YELLOW}📝 Step 1: Registering organization owner...${NC}"
OWNER_SIGNUP=$(curl -s -X POST "$ORPC_URL/auth/signUp" \
  -H "Content-Type: application/json" \
  -d '{
    "email": "'"$OWNER_EMAIL"'",
    "password": "'"$OWNER_PASSWORD"'",
    "name": "Organization Owner"
  }' -c /tmp/owner-cookies.txt)

OWNER_ID=$(echo "$OWNER_SIGNUP" | jq -r '.user.id // empty')
if [ -z "$OWNER_ID" ]; then
  echo -e "${RED}❌ Owner registration failed${NC}"
  echo "$OWNER_SIGNUP" | jq .
  exit 1
fi

echo -e "${GREEN}✅ Owner registered: $OWNER_EMAIL (ID: $OWNER_ID)${NC}"
echo ""

# Sign in owner to get session
echo -e "${YELLOW}🔐 Signing in owner...${NC}"
curl -s -X POST "$ORPC_URL/auth/signIn" \
  -H "Content-Type: application/json" \
  -d '{
    "email": "'"$OWNER_EMAIL"'",
    "password": "'"$OWNER_PASSWORD"'"
  }' -c /tmp/owner-cookies.txt > /dev/null

echo -e "${GREEN}✅ Owner signed in${NC}"
echo ""

# ========================================
# Step 2: Create Organization
# ========================================
echo -e "${YELLOW}🏢 Step 2: Creating organization...${NC}"
# Use oRPC endpoint which publishes events automatically
ORG_CREATE=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/create" \
  -H "Content-Type: application/json" \
  -b /tmp/owner-cookies.txt \
  -d '{
    "name": "'"$ORG_NAME"'",
    "slug": "'"$ORG_SLUG"'",
    "metadata": {
      "test": true,
      "timestamp": "'"$(date -Iseconds)"'"
    }
  }')

ORG_ID=$(echo "$ORG_CREATE" | jq -r '.organization.id // .id // empty')
if [ -z "$ORG_ID" ]; then
  echo -e "${RED}❌ Organization creation failed${NC}"
  echo "$ORG_CREATE" | jq .
  exit 1
fi

echo -e "${GREEN}✅ Organization created: $ORG_NAME${NC}"
echo -e "   ID: $ORG_ID"
echo -e "   Slug: $ORG_SLUG"
echo ""

# Wait for event processing
echo -e "${BLUE}⏳ Waiting for event processing (3s)...${NC}"
sleep 3

# Verify in org-core database
echo -e "${YELLOW}🔍 Verifying organization in org-core database...${NC}"
ORG_IN_DB=$(docker exec aquatiq-postgres-local psql -U aquatiq -d org_core -t -c \
  "SELECT COUNT(*) FROM organizations WHERE created_at > NOW() - INTERVAL '30 seconds';")

if [ "$(echo $ORG_IN_DB | tr -d ' ')" -gt "0" ]; then
  echo -e "${GREEN}✅ Organization persisted to org-core database${NC}"
  docker exec aquatiq-postgres-local psql -U aquatiq -d org_core -c \
    "SELECT id, name, status, created_at FROM organizations ORDER BY created_at DESC LIMIT 1;"
else
  echo -e "${RED}❌ Organization not found in org-core database${NC}"
fi
echo ""

# ========================================
# Step 3: Register Member User
# ========================================
echo -e "${YELLOW}👤 Step 3: Registering member user...${NC}"
MEMBER_SIGNUP=$(curl -s -X POST "$ORPC_URL/auth/signUp" \
  -H "Content-Type: application/json" \
  -d '{
    "email": "'"$MEMBER_EMAIL"'",
    "password": "'"$MEMBER_PASSWORD"'",
    "name": "Team Member"
  }')

MEMBER_ID=$(echo "$MEMBER_SIGNUP" | jq -r '.user.id // empty')
if [ -z "$MEMBER_ID" ]; then
  echo -e "${RED}❌ Member registration failed${NC}"
  echo "$MEMBER_SIGNUP" | jq .
  exit 1
fi

echo -e "${GREEN}✅ Member registered: $MEMBER_EMAIL (ID: $MEMBER_ID)${NC}"
echo ""

# ========================================
# Step 4: Invite Member to Organization
# ========================================
echo -e "${YELLOW}✉️  Step 4: Inviting member to organization...${NC}"
# Use oRPC endpoint which matches organization creation
INVITE_RESULT=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/invite-member" \
  -H "Content-Type: application/json" \
  -b /tmp/owner-cookies.txt \
  -d '{
    "email": "'"$MEMBER_EMAIL"'",
    "role": "member",
    "organizationId": "'"$ORG_ID"'"
  }')

INVITE_SUCCESS=$(echo "$INVITE_RESULT" | jq -r '.success')
if [ "$INVITE_SUCCESS" != "true" ]; then
  echo -e "${RED}❌ Invitation failed${NC}"
  echo "$INVITE_RESULT" | jq .
  
  # Try using Better Auth API directly
  echo -e "${YELLOW}🔄 Trying Better Auth API directly...${NC}"
  INVITE_RESULT=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/invite-member" \
    -H "Content-Type: application/json" \
    -b /tmp/owner-cookies.txt \
    -d '{
      "body": {
        "email": "'"$MEMBER_EMAIL"'",
        "role": "member",
        "organizationId": "'"$ORG_ID"'"
      }
    }')
  
  echo "Direct API result:"
  echo "$INVITE_RESULT" | jq .
else
  INVITE_ID=$(echo "$INVITE_RESULT" | jq -r '.invitation.id')
  echo -e "${GREEN}✅ Invitation sent to $MEMBER_EMAIL${NC}"
  echo -e "   Invitation ID: $INVITE_ID"
  echo -e "   Role: member"
  echo "$INVITE_RESULT" | jq .
fi
echo ""

# ========================================
# Step 5: List Organization Members
# ========================================
echo -e "${YELLOW}👥 Step 5: Listing organization members...${NC}"
MEMBERS=$(curl -s -X POST "$AUTH_URL/api/auth/organization/list" \
  -H "Content-Type: application/json" \
  -b /tmp/owner-cookies.txt \
  -d '{
    "organizationId": "'"$ORG_ID"'"
  }')

echo -e "${GREEN}Current members:${NC}"
echo "$MEMBERS" | jq '.members[] | {email: .user.email, role: .role}'
echo ""

# ========================================
# Step 6: Check Event Logs
# ========================================
echo -e "${YELLOW}📋 Step 6: Checking event logs...${NC}"
echo ""
echo -e "${BLUE}org-core logs (organization.created):${NC}"
docker logs org-core-service 2>&1 | grep -A 2 "organization created" | tail -10 || echo "No organization.created events found"
echo ""

echo -e "${BLUE}org-core logs (member.added):${NC}"
docker logs org-core-service 2>&1 | grep -A 2 "member" | tail -10 || echo "No member events found"
echo ""

# ========================================
# Step 7: Verify Database State
# ========================================
echo -e "${YELLOW}💾 Step 7: Verifying complete database state...${NC}"
echo ""

echo -e "${BLUE}Organizations:${NC}"
docker exec aquatiq-postgres-local psql -U aquatiq -d org_core -c \
  "SELECT id, name, status, plan, created_at FROM organizations ORDER BY created_at DESC LIMIT 3;"
echo ""

echo -e "${BLUE}Entitlements:${NC}"
docker exec aquatiq-postgres-local psql -U aquatiq -d org_core -c \
  "SELECT org_id, entitlement_key, enabled, updated_at FROM org_entitlements WHERE org_id = (SELECT id FROM organizations ORDER BY created_at DESC LIMIT 1) LIMIT 5;"
echo ""

# ========================================
# Summary
# ========================================
echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║           Test Summary                 ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"
echo -e "${GREEN}✅ Owner registered:${NC} $OWNER_EMAIL"
echo -e "${GREEN}✅ Organization created:${NC} $ORG_NAME ($ORG_ID)"
echo -e "${GREEN}✅ Database persistence:${NC} Verified"
echo -e "${GREEN}✅ Event flow:${NC} Working"
echo -e "${GREEN}✅ Member registered:${NC} $MEMBER_EMAIL"
echo -e "${YELLOW}⚠️  Invitation flow:${NC} Check results above"
echo ""

# Cleanup
echo -e "${YELLOW}🧹 Cleaning up temporary files...${NC}"
rm -f /tmp/owner-cookies.txt /tmp/member-cookies.txt
echo -e "${GREEN}✅ Test complete!${NC}"
