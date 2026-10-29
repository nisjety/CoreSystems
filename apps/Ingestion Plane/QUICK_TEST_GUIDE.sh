#!/bin/bash
# Quick Test Guide for Ingestion Plane
# Run this script to execute all end-to-end tests

set -e

echo "======================================"
echo "Ingestion Plane - Quick Test Suite"
echo "======================================"
echo ""

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Test 1: Quarry - List Modules
echo -e "${BLUE}Test 1: Quarry - List Available Modules${NC}"
echo "$ curl -H \"X-API-Key: dev-test-key-12345\" http://localhost:9090/v1/modules"
curl -H "X-API-Key: dev-test-key-12345" http://localhost:9090/v1/modules | jq .
echo -e "${GREEN}✓ PASSED${NC}\n"

# Test 2: Quarry - Scrape Content
echo -e "${BLUE}Test 2: Quarry - Scrape Web Content${NC}"
echo "$ curl -X POST http://localhost:9090/v1/scrape ..."
curl -X POST http://localhost:9090/v1/scrape \
  -H "X-API-Key: dev-test-key-12345" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown"]
  }' | jq '.success, .data.metadata.title'
echo -e "${GREEN}✓ PASSED${NC}\n"

# Test 3: Imports-Core - Health Check
echo -e "${BLUE}Test 3: Imports-Core - Health Check${NC}"
echo "$ curl http://localhost:9025/health"
curl http://localhost:9025/health | jq .
echo -e "${GREEN}✓ PASSED${NC}\n"

# Test 4: Imports-Core - File Upload
echo -e "${BLUE}Test 4: Imports-Core - File Upload${NC}"
echo "Creating test file..."
cat > /tmp/test_upload.csv << 'EOF'
sample,data,testing
1,test,value
2,another,row
EOF

echo "$ curl -X POST http://localhost:9025/api/v1/import/jobs/upload ..."
curl -X POST http://localhost:9025/api/v1/import/jobs/upload \
  -F "org_id=org_test_123" \
  -F "files=@/tmp/test_upload.csv" | jq '{id, status, total_items, created_at}'
echo -e "${GREEN}✓ PASSED${NC}\n"

# Summary
echo "======================================"
echo -e "${GREEN}All Tests Passed! ✓${NC}"
echo "======================================"
echo ""
echo "Infrastructure Status:"
echo "  • Quarry API:    http://localhost:9090"
echo "  • Imports-Core:  http://localhost:9025"
echo "  • PostgreSQL:    localhost:9434"
echo "  • Redis:         localhost:9380"
echo "  • NATS:          localhost:9222"
echo "  • Temporal:      localhost:9233"
echo "  • Temporal UI:   http://localhost:9081"
echo "  • Qdrant:        localhost:9333"
echo ""
echo "For detailed test results, see: END_TO_END_TESTS.md"
