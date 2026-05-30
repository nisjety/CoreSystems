#!/bin/bash

# ========================================
# Organization Invitation Flow Test
# ========================================
# Quick test for member invitations
# ========================================

set -e

GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

AUTH_URL="${AUTH_URL:-http://localhost:3001}"
ORPC_URL="${ORPC_URL:-http://localhost:3001/api/v2}"

echo -e "${BLUE}╔════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║   Organization Invitation Test        ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════╝${NC}"
echo ""

# Check if organization ID provided
if [ -z "$1" ]; then
  echo -e "${YELLOW}Usage: $0 <organization-id> [member-email]${NC}"
  echo ""
  echo "Examples:"
  echo "  $0 org_123abc                    # Invite test-member@example.com"
  echo "  $0 org_123abc new@example.com    # Invite specific email"
  echo ""
  echo -e "${BLUE}Available organizations:${NC}"
  docker exec aquatiq-postgres-local psql -U aquatiq -d auth_service -t -c \
    "SELECT id, name FROM organization ORDER BY created_at DESC LIMIT 5;"
  exit 1
fi

ORG_ID="$1"
MEMBER_EMAIL="${2:-test-member-$(date +%s)@example.com}"

echo -e "${YELLOW}📋 Test Configuration:${NC}"
echo "   Organization ID: $ORG_ID"
echo "   Member Email: $MEMBER_EMAIL"
echo ""

# Get owner session (assumes you're logged in)
if [ ! -f /tmp/owner-cookies.txt ]; then
  echo -e "${YELLOW}🔐 Please provide owner credentials:${NC}"
  read -p "Owner email: " OWNER_EMAIL
  read -sp "Owner password: " OWNER_PASSWORD
  echo ""
  
  curl -s -X POST "$AUTH_URL/api/v2/auth/signIn" \
    -H "Content-Type: application/json" \
    -d '{
      "email": "'"$OWNER_EMAIL"'",
      "password": "'"$OWNER_PASSWORD"'"
    }' -c /tmp/owner-cookies.txt > /dev/null
  
  echo -e "${GREEN}✅ Signed in${NC}"
fi

# Send invitation
echo -e "${YELLOW}✉️  Sending invitation...${NC}"
INVITE_RESULT=$(curl -s -X POST "$AUTH_URL/api/auth/organization/inviteMember" \
  -H "Content-Type: application/json" \
  -b /tmp/owner-cookies.txt \
  -d '{
    "email": "'"$MEMBER_EMAIL"'",
    "organizationId": "'"$ORG_ID"'"
  }')

echo "$INVITE_RESULT" | jq .

if [ "$(echo "$INVITE_RESULT" | jq -r '.success')" = "true" ]; then
  echo ""
  echo -e "${GREEN}✅ Invitation sent successfully!${NC}"
  echo ""
  echo -e "${BLUE}Next steps:${NC}"
  echo "1. Check invitation in database:"
  echo "   docker exec aquatiq-postgres-local psql -U aquatiq -d auth_service -c \\"
  echo "     \"SELECT id, email, role, status, expires_at FROM organization_invitation WHERE organization_id = '$ORG_ID' ORDER BY created_at DESC LIMIT 5;\""
  echo ""
  echo "2. Accept invitation (if user exists):"
  echo "   curl -X POST $AUTH_URL/api/v2/auth/organization/accept-invitation \\"
  echo "     -H 'Content-Type: application/json' \\"
  echo "     -b member-cookies.txt \\"
  echo "     -d '{\"body\": {\"invitationId\": \"INVITATION_ID\"}}'"
else
  echo ""
  echo -e "${RED}❌ Invitation failed${NC}"
  echo ""
  echo -e "${BLUE}Troubleshooting:${NC}"
  echo "• Verify organization exists in auth_service database"
  echo "• Check if you have owner/admin role"
  echo "• Ensure Better Auth organization plugin is enabled"
fi

echo ""
