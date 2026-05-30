#!/bin/bash

set -e

echo "🔍 Debug Invitation Test Script"
echo "================================"

# Variables
OWNER_EMAIL="debug-owner-$(date +%s)@example.com"
MEMBER_EMAIL="debug-member-$(date +%s)@example.com"
ORG_NAME="Debug Org $(date +%s)"
ORG_SLUG="debug-org-$(date +%s)"
AUTH_URL="http://localhost:3001"

echo "📧 Owner: $OWNER_EMAIL"
echo "📧 Member: $MEMBER_EMAIL"
echo "🏢 Organization: $ORG_NAME"
echo

# Step 1: Register owner
echo "1️⃣ Registering owner..."
curl -s -X POST "$AUTH_URL/api/auth/sign-up/email" \
  -H "Content-Type: application/json" \
  -c /tmp/debug-owner.cookies \
  -d "{
    \"email\": \"$OWNER_EMAIL\",
    \"password\": \"TestPass123\",
    \"name\": \"Debug Owner\"
  }" > /dev/null

echo "✅ Owner registered"

# Step 2: Sign in owner
echo "2️⃣ Signing in owner..."
curl -s -X POST "$AUTH_URL/api/auth/sign-in/email" \
  -H "Content-Type: application/json" \
  -b /tmp/debug-owner.cookies -c /tmp/debug-owner.cookies \
  -d "{
    \"email\": \"$OWNER_EMAIL\",
    \"password\": \"TestPass123\"
  }" > /dev/null

echo "✅ Owner signed in"

# Step 3: Create organization
echo "3️⃣ Creating organization..."
ORG_RESULT=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/create" \
  -H "Content-Type: application/json" \
  -b /tmp/debug-owner.cookies \
  -d "{
    \"name\": \"$ORG_NAME\",
    \"slug\": \"$ORG_SLUG\"
  }")

echo "Organization creation result: $ORG_RESULT"

ORG_ID=$(echo "$ORG_RESULT" | jq -r '.organization.id')
echo "✅ Organization created: $ORG_ID"

# Step 4: Verify organization membership
echo "4️⃣ Checking organization membership..."
MEMBER_RESULT=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/list" \
  -H "Content-Type: application/json" \
  -b /tmp/debug-owner.cookies)

echo "Membership result: $MEMBER_RESULT"

# Step 5: Try different invitation formats
echo "5️⃣ Testing different invitation formats..."

echo "Format A - Standard parameters:"
INVITE_A=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/invite-member" \
  -H "Content-Type: application/json" \
  -b /tmp/debug-owner.cookies \
  -d "{
    \"email\": \"$MEMBER_EMAIL\",
    \"role\": \"member\",
    \"organizationId\": \"$ORG_ID\"
  }")
echo "Result A: $INVITE_A"

echo "Format B - With resend flag:"
INVITE_B=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/invite-member" \
  -H "Content-Type: application/json" \
  -b /tmp/debug-owner.cookies \
  -d "{
    \"email\": \"$MEMBER_EMAIL\",
    \"role\": \"member\",
    \"organizationId\": \"$ORG_ID\",
    \"resend\": false
  }")
echo "Result B: $INVITE_B"

echo "Format C - Array role:"
INVITE_C=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/invite-member" \
  -H "Content-Type: application/json" \
  -b /tmp/debug-owner.cookies \
  -d "{
    \"email\": \"$MEMBER_EMAIL\",
    \"role\": [\"member\"],
    \"organizationId\": \"$ORG_ID\"
  }")
echo "Result C: $INVITE_C"

echo "Format D - Without organizationId (should use active org):"
INVITE_D=$(curl -s -X POST "$AUTH_URL/api/v2/auth/organization/invite-member" \
  -H "Content-Type: application/json" \
  -b /tmp/debug-owner.cookies \
  -d "{
    \"email\": \"$MEMBER_EMAIL\",
    \"role\": \"member\"
  }")
echo "Result D: $INVITE_D"

echo
echo "6️⃣ Testing Better Auth direct API..."

# Test direct Better Auth API with session
echo "Direct Better Auth API test:"
DIRECT_RESULT=$(curl -s -X POST "$AUTH_URL/api/auth/organization/invite-member" \
  -H "Content-Type: application/json" \
  -b /tmp/debug-owner.cookies \
  -d "{
    \"email\": \"$MEMBER_EMAIL\",
    \"role\": \"member\",
    \"organizationId\": \"$ORG_ID\"
  }")
echo "Direct Result: $DIRECT_RESULT"

echo
echo "✅ Debug test completed"
echo "Check auth service logs with: docker logs auth-service | tail -50"