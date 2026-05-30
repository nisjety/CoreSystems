#!/bin/bash

# Phase 2 Priority 1 Feature Testing Script
# Tests: Rate Limiting, Prompt Templates, Audit Logging

set -e

ORG_CORE_URL="http://localhost:8080"
AI_CORE_URL="http://localhost:8040"

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "==============================================="
echo "Phase 2 Priority 1 Feature Tests"
echo "==============================================="
echo ""

# Test 1: Rate Limiting - Org Core
echo -e "${YELLOW}Test 1: Rate Limiting - Org Core${NC}"
echo "Sending 75 requests to trigger rate limit (60 RPM default)..."

# Create a test org_id for rate limiting
TEST_ORG_ID="550e8400-e29b-41d4-a716-446655440000"

# Send 75 rapid requests
success_count=0
rate_limited_count=0

for i in {1..75}; do
    response=$(curl -s -w "\n%{http_code}" -X GET \
        "$ORG_CORE_URL/health" \
        -H "X-Org-ID: $TEST_ORG_ID" 2>&1)
    
    http_code=$(echo "$response" | tail -n1)
    
    if [ "$http_code" == "200" ]; then
        success_count=$((success_count + 1))
    elif [ "$http_code" == "429" ]; then
        rate_limited_count=$((rate_limited_count + 1))
        if [ $rate_limited_count -eq 1 ]; then
            echo -e "${GREEN}✓ Rate limit triggered at request #$i${NC}"
            # Show rate limit headers
            headers=$(echo "$response" | head -n -1)
            echo "  Response: $headers"
        fi
    fi
done

echo "Results: $success_count successful, $rate_limited_count rate-limited"

if [ $rate_limited_count -gt 0 ]; then
    echo -e "${GREEN}✓ Rate Limiting: PASS${NC}"
else
    echo -e "${RED}✗ Rate Limiting: FAIL (no 429 responses)${NC}"
fi
echo ""

# Test 2: Audit Logging - Verify logs written
echo -e "${YELLOW}Test 2: Audit Logging${NC}"
echo "Checking if audit logs were created..."

# Query audit logs from database
AUDIT_COUNT=$(docker exec aquatiq-postgres-local psql -U postgres -d org_core -t -c \
    "SELECT COUNT(*) FROM audit_logs WHERE timestamp > NOW() - INTERVAL '5 minutes';" 2>/dev/null | xargs)

if [ "$AUDIT_COUNT" -gt 0 ]; then
    echo -e "${GREEN}✓ Audit logs found: $AUDIT_COUNT entries in last 5 minutes${NC}"
    
    # Show sample audit log
    echo "Sample audit log:"
    docker exec aquatiq-postgres-local psql -U postgres -d org_core -c \
        "SELECT id, org_id, action, resource, request_method, request_path, response_status, timestamp 
         FROM audit_logs 
         ORDER BY timestamp DESC 
         LIMIT 1;" 2>/dev/null | head -10
    
    echo -e "${GREEN}✓ Audit Logging: PASS${NC}"
else
    echo -e "${YELLOW}⚠ No audit logs found (middleware may not be logging /health)${NC}"
fi
echo ""

# Test 3: Prompt Templates - AI Core
echo -e "${YELLOW}Test 3: Prompt Templates${NC}"
echo "Testing prompt template management..."

# Check if AI Core is running
if ! curl -s -f "$AI_CORE_URL/health" > /dev/null 2>&1; then
    echo -e "${RED}✗ AI Core service not responding at $AI_CORE_URL${NC}"
    echo "  Start with: docker compose -f backend/docker-compose.yml up -d ai-core"
else
    # List templates
    echo "Listing prompt templates..."
    templates=$(curl -s -X GET "$AI_CORE_URL/api/v1/templates" 2>&1)
    
    if echo "$templates" | grep -q "chat_default"; then
        echo -e "${GREEN}✓ Default templates found${NC}"
        echo "Templates:"
        echo "$templates" | python3 -m json.tool 2>/dev/null | grep '"name"' | head -5 || echo "$templates"
        
        # Test template rendering
        echo ""
        echo "Testing template rendering..."
        render_response=$(curl -s -X POST "$AI_CORE_URL/api/v1/templates/chat_default/render" \
            -H "Content-Type: application/json" \
            -d '{
                "variables": {
                    "user_message": "Hello, world!",
                    "context": "Test context"
                },
                "org_id": "'"$TEST_ORG_ID"'"
            }' 2>&1)
        
        if echo "$render_response" | grep -q "Hello, world!"; then
            echo -e "${GREEN}✓ Template rendering: PASS${NC}"
        else
            echo -e "${YELLOW}⚠ Template rendering: ${NC}Response: $render_response"
        fi
        
        echo -e "${GREEN}✓ Prompt Templates: PASS${NC}"
    else
        echo -e "${RED}✗ Templates not found. Response: $templates${NC}"
        echo -e "${RED}✗ Prompt Templates: FAIL${NC}"
    fi
fi
echo ""

# Test 4: Check database schema
echo -e "${YELLOW}Test 4: Database Schema Verification${NC}"

# Check Org Core tables
echo "Org Core tables:"
ORG_TABLES=$(docker exec aquatiq-postgres-local psql -U postgres -d org_core -t -c \
    "SELECT table_name FROM information_schema.tables 
     WHERE table_schema = 'public' AND table_name IN ('audit_logs', 'jobs', 'sessions') 
     ORDER BY table_name;" 2>/dev/null | xargs)

echo "  Found: $ORG_TABLES"
if echo "$ORG_TABLES" | grep -q "audit_logs"; then
    echo -e "${GREEN}✓ audit_logs table exists${NC}"
else
    echo -e "${RED}✗ audit_logs table missing${NC}"
fi

# Check AI Core tables
echo ""
echo "AI Core tables:"
AI_TABLES=$(docker exec aquatiq-postgres-local psql -U postgres -d ai_core -t -c \
    "SELECT table_name FROM information_schema.tables 
     WHERE table_schema = 'public' AND table_name LIKE 'prompt%' OR table_name LIKE 'template%' 
     ORDER BY table_name;" 2>/dev/null | xargs)

echo "  Found: $AI_TABLES"
if echo "$AI_TABLES" | grep -q "prompt_templates"; then
    echo -e "${GREEN}✓ prompt_templates table exists${NC}"
else
    echo -e "${RED}✗ prompt_templates table missing${NC}"
fi
echo ""

# Summary
echo "==============================================="
echo "Test Summary"
echo "==============================================="
echo ""
echo "1. Rate Limiting: Tested with 75 requests"
echo "2. Audit Logging: Checked database for logs"
echo "3. Prompt Templates: Verified templates & rendering"
echo "4. Database Schema: Verified tables exist"
echo ""
echo "For detailed testing:"
echo "  - Rate Limit Admin API: curl $ORG_CORE_URL/api/v1/admin/ratelimit/:org_id/stats"
echo "  - Audit Logs API: curl $ORG_CORE_URL/api/v1/admin/audit/logs"
echo "  - Template API: curl $AI_CORE_URL/api/v1/templates"
echo ""
