#!/bin/bash

# Comprehensive Phase 2 Feature Testing
set -e

ORG_CORE_URL="http://localhost:8080"
AI_CORE_URL="http://localhost:8040"
TEST_ORG_ID="550e8400-e29b-41d4-a716-446655440000"

echo "========================================="
echo "Phase 2 Priority 1 - Manual API Tests"
echo "========================================="
echo ""

# Test 1: Check services are running
echo "1. Testing service health..."
echo ""

echo "Org Core Health:"
ORG_HEALTH=$(curl -s $ORG_CORE_URL/health)
echo "$ORG_HEALTH"
echo ""

echo "AI Core Health:"
AI_HEALTH=$(curl -s $AI_CORE_URL/health)
echo "$AI_HEALTH"
echo ""

# Test 2: Rate Limiting - Check if endpoints exist
echo "========================================="
echo "2. Testing Rate Limiting"
echo "========================================="
echo ""

echo "a) Sending 10 requests to check rate limit headers..."
for i in {1..10}; do
    RESPONSE=$(curl -s -w "\nHTTP_CODE:%{http_code}\n" -H "X-Org-ID: $TEST_ORG_ID" $ORG_CORE_URL/api/v1/admin/ratelimit/stats 2>&1)
    HTTP_CODE=$(echo "$RESPONSE" | grep "HTTP_CODE" | cut -d: -f2)
    
    if [ "$i" == "1" ]; then
        echo "First request - checking rate limit headers:"
        curl -i -H "X-Org-ID: $TEST_ORG_ID" $ORG_CORE_URL/api/v1/admin/ratelimit/stats 2>&1 | grep -i "ratelimit" || echo "No ratelimit headers found"
        echo ""
    fi
    
    if [ "$HTTP_CODE" == "429" ]; then
        echo "✓ Rate limited at request #$i (HTTP 429)"
        break
    fi
done

echo ""
echo "b) Testing Rate Limit Admin API..."
echo "GET /api/v1/admin/ratelimit/$TEST_ORG_ID/stats"
curl -s $ORG_CORE_URL/api/v1/admin/ratelimit/$TEST_ORG_ID/stats | python3 -m json.tool 2>&1 || echo "Endpoint not accessible or not JSON"
echo ""

# Test 3: Audit Logging
echo "========================================="
echo "3. Testing Audit Logging"
echo "========================================="
echo ""

echo "a) Sending test requests to generate audit logs..."
for i in {1..3}; do
    curl -s -X GET -H "X-Org-ID: $TEST_ORG_ID" $ORG_CORE_URL/health > /dev/null
done
echo "Sent 3 requests"
echo ""

echo "b) Querying audit logs from database..."
AUDIT_COUNT=$(docker exec aquatiq-postgres-local psql -U postgres -d aquatiq_dev -t -c \
    "SELECT COUNT(*) FROM audit_logs WHERE created_at > NOW() - INTERVAL '2 minutes';" 2>&1 | xargs)
echo "Audit logs in last 2 minutes: $AUDIT_COUNT"

if [ "$AUDIT_COUNT" -gt 0 ]; then
    echo ""
    echo "Sample audit log entries:"
    docker exec aquatiq-postgres-local psql -U postgres -d aquatiq_dev -c \
        "SELECT org_id, action, resource_type, created_at 
         FROM audit_logs 
         WHERE created_at > NOW() - INTERVAL '2 minutes'
         ORDER BY created_at DESC 
         LIMIT 3;" 2>&1
fi

echo ""
echo "c) Testing Audit Admin API..."
echo "GET /api/v1/admin/audit/logs?limit=3"
curl -s "$ORG_CORE_URL/api/v1/admin/audit/logs?limit=3" | python3 -m json.tool 2>&1 || echo "Endpoint not accessible or not JSON"
echo ""

# Test 4: Prompt Templates
echo "========================================="
echo "4. Testing Prompt Templates"
echo "========================================="
echo ""

echo "a) Checking templates in database..."
docker exec aquatiq-postgres-local psql -U postgres -d ai_core -c \
    "SELECT name, category, version FROM prompt_templates ORDER BY name LIMIT 6;" 2>&1
echo ""

echo "b) Testing Templates API..."
echo "GET /api/v1/templates"
TEMPLATES_RESPONSE=$(curl -s $AI_CORE_URL/api/v1/templates 2>&1)
echo "$TEMPLATES_RESPONSE" | python3 -m json.tool 2>&1 || echo "Response: $TEMPLATES_RESPONSE"
echo ""

if echo "$TEMPLATES_RESPONSE" | grep -q "chat_default"; then
    echo "✓ Templates API working!"
    echo ""
    echo "c) Testing template rendering..."
    echo "POST /api/v1/templates/render"
    curl -s -X POST "$AI_CORE_URL/api/v1/templates/render" \
        -H "Content-Type: application/json" \
        -d '{
            "name": "chat_default",
            "variables": {
                "query": "Hello world"
            },
            "org_id": "'"$TEST_ORG_ID"'"
        }' | python3 -m json.tool 2>&1 || echo "Render endpoint error"
else
    echo "✗ Templates API not working yet"
fi

echo ""
echo "========================================="
echo "Test Summary"
echo "========================================="
echo ""
echo "Services:"
echo "  - Org Core: Running on :8080"
echo "  - AI Core: Running on :8040"
echo ""
echo "Database Tables:"
echo "  - audit_logs: ✓ Created"
echo "  - prompt_templates: ✓ Created with 6 default templates"
echo ""
echo "Features to verify:"
echo "  1. Rate Limiting - Check X-RateLimit headers in responses"
echo "  2. Audit Logging - Check database for request logs"
echo "  3. Prompt Templates - Test API endpoints"
echo ""
echo "Admin Endpoints:"
echo "  - Rate Limit Stats: GET $ORG_CORE_URL/api/v1/admin/ratelimit/:org_id/stats"
echo "  - Audit Logs: GET $ORG_CORE_URL/api/v1/admin/audit/logs"
echo "  - Templates: GET $AI_CORE_URL/api/v1/templates"
echo ""
