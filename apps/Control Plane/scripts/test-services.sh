#!/bin/bash
# Test all backend services

set -e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🧪 Testing CoreSystem Backend Services${NC}"
echo ""

# Check if services are running
if ! docker compose ps | grep -q "running"; then
    echo -e "${RED}❌ Services are not running. Start them first with ./start-services.sh${NC}"
    exit 1
fi

# Test counters
passed=0
failed=0

# Helper function to test HTTP endpoint
test_http() {
    local service=$1
    local url=$2
    local expected_status=${3:-200}
    
    echo -e "${YELLOW}Testing $service: $url${NC}"
    
    if response=$(curl -s -o /dev/null -w "%{http_code}" --max-time 10 "$url" 2>/dev/null); then
        if [ "$response" = "$expected_status" ]; then
            echo -e "${GREEN}✅ $service responded with $response${NC}"
            ((passed++))
        else
            echo -e "${RED}❌ $service responded with $response (expected $expected_status)${NC}"
            ((failed++))
        fi
    else
        echo -e "${RED}❌ $service did not respond${NC}"
        ((failed++))
    fi
}

# Helper function to test gRPC endpoint
test_grpc() {
    local service=$1
    local address=$2
    local method=$3
    
    echo -e "${YELLOW}Testing $service gRPC: $address${NC}"
    
    if command -v grpcurl > /dev/null 2>&1; then
        if grpcurl -plaintext -max-time 10 "$address" list > /dev/null 2>&1; then
            echo -e "${GREEN}✅ $service gRPC is accessible${NC}"
            ((passed++))
        else
            echo -e "${RED}❌ $service gRPC is not accessible${NC}"
            ((failed++))
        fi
    else
        echo -e "${YELLOW}⚠️  grpcurl not installed, skipping gRPC test${NC}"
    fi
}

# Test NATS connection
test_nats() {
    echo -e "${YELLOW}Testing NATS connection${NC}"
    
    if command -v nats > /dev/null 2>&1; then
        if nats pub test.connection "test message" --server=nats://localhost:4222 --token=nats > /dev/null 2>&1; then
            echo -e "${GREEN}✅ NATS is accessible${NC}"
            ((passed++))
        else
            echo -e "${RED}❌ NATS is not accessible${NC}"
            ((failed++))
        fi
    else
        echo -e "${YELLOW}⚠️  nats CLI not installed, skipping NATS test${NC}"
    fi
}

echo -e "${BLUE}=== Health Check Tests ===${NC}"
echo ""

# Test Org Core
test_http "Org Core" "http://localhost:8080/health"

# Test User Service (if it has a health endpoint)
test_http "User Service" "http://localhost:3012/health" "200"

# Test Billing Core
test_http "Billing Core" "http://localhost:3014/health" "200"

# Test Auth Service
test_http "Auth Service" "http://localhost:3000/health" "200"

# Test AI Core
test_http "AI Core" "http://localhost:8000/health" "200"

echo ""
echo -e "${BLUE}=== gRPC Tests ===${NC}"
echo ""

# Test gRPC endpoints
test_grpc "Org Core" "localhost:9090"
test_grpc "User Service" "localhost:50012"
test_grpc "Billing Core" "localhost:50013"
test_grpc "AI Core" "localhost:50014"

echo ""
echo -e "${BLUE}=== Infrastructure Tests ===${NC}"
echo ""

# Test PostgreSQL
echo -e "${YELLOW}Testing PostgreSQL${NC}"
if docker exec coresystem-postgres-local psql -U coresystem -c "SELECT 1" > /dev/null 2>&1; then
    echo -e "${GREEN}✅ PostgreSQL is accessible${NC}"
    ((passed++))
else
    echo -e "${RED}❌ PostgreSQL is not accessible${NC}"
    ((failed++))
fi

# Test Redis
echo -e "${YELLOW}Testing Redis${NC}"
if docker exec coresystem-redis-local redis-cli -a redis ping | grep -q PONG; then
    echo -e "${GREEN}✅ Redis is accessible${NC}"
    ((passed++))
else
    echo -e "${RED}❌ Redis is not accessible${NC}"
    ((failed++))
fi

# Test NATS
test_nats

echo ""
echo -e "${BLUE}=== Database Tests ===${NC}"
echo ""

# Check databases exist
databases=("coresystem_dev" "user_service" "admin_service" "auth_service" "ai_core")
for db in "${databases[@]}"; do
    echo -e "${YELLOW}Checking database $db${NC}"
    if docker exec coresystem-postgres-local psql -U coresystem -lqt | cut -d \| -f 1 | grep -qw "$db"; then
        echo -e "${GREEN}✅ Database $db exists${NC}"
        ((passed++))
    else
        echo -e "${RED}❌ Database $db does not exist${NC}"
        ((failed++))
    fi
done

# Check pgvector extension
echo -e "${YELLOW}Checking pgvector extension${NC}"
if docker exec coresystem-postgres-local psql -U coresystem -d coresystem_dev -c "\dx" | grep -q vector; then
    echo -e "${GREEN}✅ pgvector extension is installed${NC}"
    ((passed++))
else
    echo -e "${RED}❌ pgvector extension is not installed${NC}"
    ((failed++))
fi

echo ""
echo -e "${BLUE}=== Test Summary ===${NC}"
echo -e "Passed: ${GREEN}$passed${NC}"
echo -e "Failed: ${RED}$failed${NC}"
echo ""

if [ $failed -eq 0 ]; then
    echo -e "${GREEN}✅ All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}❌ Some tests failed${NC}"
    exit 1
fi
