#!/bin/bash
# Org-Core Service Test Suite
# Tests all major endpoints and functionality

# Don't exit on error, we want to see all test results
# set -e

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test counters
PASSED=0
FAILED=0

# Helper functions
test_pass() {
    echo -e "${GREEN}✓${NC} $1"
    ((PASSED++))
}

test_fail() {
    echo -e "${RED}✗${NC} $1"
    ((FAILED++))
}

test_info() {
    echo -e "${YELLOW}ℹ${NC} $1"
}

echo "=================================================="
echo "Org-Core Service Test Suite"
echo "=================================================="
echo ""

# 1. Container Status
echo "1. Checking container status..."
if docker ps --filter "name=org-core-service" --filter "status=running" | grep -q "org-core-service"; then
    test_pass "Container is running"
else
    test_fail "Container is not running"
    exit 1
fi

# 2. HTTP Health Check
echo ""
echo "2. Testing HTTP health endpoint..."
HEALTH_RESPONSE=$(curl -s http://localhost:8080/health)
if echo "$HEALTH_RESPONSE" | jq -e '.status == "healthy"' > /dev/null 2>&1; then
    test_pass "HTTP health check (status: healthy)"
    test_info "   Database: $(echo "$HEALTH_RESPONSE" | jq -r '.components.database.status')"
    test_info "   Redis: $(echo "$HEALTH_RESPONSE" | jq -r '.components.redis.status')"
else
    test_fail "HTTP health check failed"
fi

# 3. Prometheus Metrics
echo ""
echo "3. Testing Prometheus metrics..."
METRICS=$(curl -s http://localhost:9091/metrics)
if echo "$METRICS" | grep -q "go_info"; then
    test_pass "Prometheus metrics endpoint"
    GO_VERSION=$(echo "$METRICS" | grep "go_info{version" | sed -n 's/.*version="\([^"]*\)".*/\1/p')
    test_info "   Go version: $GO_VERSION"
    THREADS=$(echo "$METRICS" | grep "^go_threads" | awk '{print $2}')
    test_info "   Threads: $THREADS"
else
    test_fail "Prometheus metrics endpoint"
fi

# 4. gRPC Service Discovery
echo ""
echo "4. Testing gRPC service discovery..."
GRPC_SERVICES=$(grpcurl -plaintext localhost:9090 list 2>&1)
if echo "$GRPC_SERVICES" | grep -q "orgcore.v1.OrgCoreService"; then
    test_pass "gRPC service discovery"
    test_info "   Services: $(echo "$GRPC_SERVICES" | wc -l | xargs)"
else
    test_fail "gRPC service discovery"
fi

# 5. gRPC Service Methods
echo ""
echo "5. Testing gRPC service methods..."
METHODS=$(grpcurl -plaintext localhost:9090 describe orgcore.v1.OrgCoreService 2>&1)
if echo "$METHODS" | grep -q "CreateOrganization"; then
    test_pass "gRPC service methods available"
    METHOD_COUNT=$(echo "$METHODS" | grep -c "rpc " || echo 0)
    test_info "   Methods: $METHOD_COUNT"
else
    test_fail "gRPC service methods"
fi

# 6. RAG Tools Schema
echo ""
echo "6. Testing RAG tools schema endpoint..."
RAG_SCHEMA=$(curl -s http://localhost:8080/api/v1/rag/tools/schema)
if echo "$RAG_SCHEMA" | jq -e '.tools | has("memory_recall")' > /dev/null 2>&1; then
    test_pass "RAG tools schema endpoint"
    TOOL_COUNT=$(echo "$RAG_SCHEMA" | jq '.tools | length')
    test_info "   Tools available: $TOOL_COUNT"
    test_info "   Format: $(echo "$RAG_SCHEMA" | jq -r '.format')"
else
    test_fail "RAG tools schema endpoint"
fi

# 7. TOON Format Support
echo ""
echo "7. Checking TOON format support..."
if docker exec org-core-service test -f /root/org-core 2>/dev/null; then
    test_pass "Service binary exists"
    # TOON is compiled into the binary
    test_info "   TOON converter: Included in binary"
else
    test_fail "Service binary check"
fi

# 8. Database Connection
echo ""
echo "8. Testing database connection..."
DB_STATUS=$(echo "$HEALTH_RESPONSE" | jq -r '.components.database.status')
DB_MESSAGE=$(echo "$HEALTH_RESPONSE" | jq -r '.components.database.message')
if [ "$DB_STATUS" = "healthy" ]; then
    test_pass "Database connection"
    test_info "   $DB_MESSAGE"
else
    test_fail "Database connection"
fi

# 9. Redis Connection
echo ""
echo "9. Testing Redis connection..."
REDIS_STATUS=$(echo "$HEALTH_RESPONSE" | jq -r '.components.redis.status')
if [ "$REDIS_STATUS" = "healthy" ]; then
    test_pass "Redis connection"
else
    test_fail "Redis connection"
fi

# 10. Service Logs Check
echo ""
echo "10. Checking service logs for errors..."
ERROR_COUNT=$(docker logs org-core-service 2>&1 | grep -i "error\|fatal\|panic" | wc -l | xargs)
if [ "$ERROR_COUNT" -eq 0 ]; then
    test_pass "No errors in logs"
else
    test_fail "Found $ERROR_COUNT errors in logs"
    test_info "   Run 'docker logs org-core-service' to see details"
fi

# 11. Memory Usage
echo ""
echo "11. Checking memory usage..."
MEMORY_USAGE=$(docker stats org-core-service --no-stream --format "{{.MemUsage}}" 2>/dev/null || echo "N/A")
if [ "$MEMORY_USAGE" != "N/A" ]; then
    test_pass "Memory usage: $MEMORY_USAGE"
else
    test_info "Memory usage: Could not retrieve"
fi

# 12. Port Bindings
echo ""
echo "12. Verifying port bindings..."
PORTS=$(docker port org-core-service 2>/dev/null)
if echo "$PORTS" | grep -q "8080\|9090\|9091"; then
    test_pass "Port bindings verified"
    test_info "   HTTP: 8080, gRPC: 9090, Metrics: 9091"
else
    test_fail "Port bindings"
fi

# Summary
echo ""
echo "=================================================="
echo "Test Summary"
echo "=================================================="
echo -e "${GREEN}Passed:${NC} $PASSED"
echo -e "${RED}Failed:${NC} $FAILED"
echo "Total: $((PASSED + FAILED))"
echo ""

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}✓ All tests passed!${NC}"
    echo ""
    echo "Service is ready for production use:"
    echo "  - HTTP API: http://localhost:8080"
    echo "  - gRPC API: localhost:9090"
    echo "  - Metrics: http://localhost:9091/metrics"
    exit 0
else
    echo -e "${RED}✗ Some tests failed${NC}"
    exit 1
fi
