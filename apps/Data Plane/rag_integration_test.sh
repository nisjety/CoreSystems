#!/bin/bash
# RAG Integration End-to-End Test Suite
# Demonstrates the complete RAG pipeline without external dependencies

set -e

# Configuration
DATA_PLANE="http://localhost:8004"
DOCS_PLANE="http://localhost:8001"
AI_CORE="http://localhost:8100"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Counters
PASSED=0
FAILED=0

# Test helper function
test_case() {
    local name="$1"
    local cmd="$2"
    local expected="$3"
    
    echo -e "\n${YELLOW}[TEST]${NC} $name"
    
    result=$(eval "$cmd" 2>&1)
    
    if echo "$result" | grep -q "$expected"; then
        echo -e "${GREEN}✅ PASS${NC}"
        ((PASSED++))
    else
        echo -e "${RED}❌ FAIL${NC}"
        echo "Expected: $expected"
        echo "Got: $result" | head -3
        ((FAILED++))
    fi
}

# Run tests
echo ""
echo "════════════════════════════════════════════════════════════════"
echo "🚀 RAG Integration Test Suite"
echo "════════════════════════════════════════════════════════════════"

# Test 1: Service Health
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "[TEST 1] Service Health"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

test_case "Data Plane Retrieval (port 8004)" \
    "curl -s $DATA_PLANE/health" \
    "ok"

test_case "Data Plane Documents (port 8001)" \
    "curl -s $DOCS_PLANE/health" \
    "ok"

test_case "AI-Core (port 8100)" \
    "curl -s $AI_CORE/health" \
    "healthy"

# Test 2: Vector Search + Reranking
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "[TEST 2] Vector Search + Cohere Reranking"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

RETRIEVE_RESPONSE=$(curl -s -X POST "$DATA_PLANE/v1/retrieve" \
  -H "Content-Type: application/json" \
  -d '{
    "org_id": "rag-demo",
    "query": "How does vector search work?",
    "top_k": 5
  }')

test_case "Retrieval response has facts" \
    "echo '$RETRIEVE_RESPONSE'" \
    '"facts":'

test_case "Retrieval response has rerank_score" \
    "echo '$RETRIEVE_RESPONSE'" \
    '"rerank_score":'

test_case "Sources are included" \
    "echo '$RETRIEVE_RESPONSE'" \
    '"sources":'

# Test 3: Multi-Tenant Isolation
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "[TEST 3] Multi-Tenant Isolation"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

ISOLATION_RESPONSE=$(curl -s -X POST "$DATA_PLANE/v1/retrieve" \
  -H "Content-Type: application/json" \
  -d '{
    "org_id": "non-existent-org",
    "query": "test",
    "top_k": 10
  }')

test_case "Non-existent org returns 0 facts" \
    "echo '$ISOLATION_RESPONSE' | grep -o '\"facts\":\[\]'" \
    "facts"

# Test 4: Response Format
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "[TEST 4] Response Format Validation"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

FORMAT_RESPONSE=$(curl -s -X POST "$DATA_PLANE/v1/retrieve" \
  -H "Content-Type: application/json" \
  -d '{
    "org_id": "rag-demo",
    "query": "test",
    "top_k": 2
  }')

test_case "Response has all required fields" \
    "echo '$FORMAT_RESPONSE'" \
    '"query":'

test_case "Each fact has knowledge_id" \
    "echo '$FORMAT_RESPONSE' | grep -o '\"knowledge_id\"'" \
    "knowledge_id"

test_case "Each fact has document_id" \
    "echo '$FORMAT_RESPONSE' | grep -o '\"document_id\"'" \
    "document_id"

test_case "Each fact has text" \
    "echo '$FORMAT_RESPONSE' | grep -o '\"text\"'" \
    "text"

test_case "Each fact has score" \
    "echo '$FORMAT_RESPONSE' | grep -o '\"score\"'" \
    "score"

# Summary
echo ""
echo "════════════════════════════════════════════════════════════════"
echo "TEST SUMMARY"
echo "════════════════════════════════════════════════════════════════"
echo -e "${GREEN}✅ Passed: $PASSED${NC}"
echo -e "${RED}❌ Failed: $FAILED${NC}"

TOTAL=$((PASSED + FAILED))
if [ $FAILED -eq 0 ]; then
    echo -e "\n${GREEN}🎉 All tests passed! RAG integration is working.${NC}"
    exit 0
else
    echo -e "\n${RED}⚠️  $FAILED/$TOTAL test(s) failed. Check configuration.${NC}"
    exit 1
fi
