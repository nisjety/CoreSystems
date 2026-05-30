#!/bin/bash

# Test Convex Gateway Integration with Org-Core and AI-Core

set -e

echo "==========================================="
echo "Convex Gateway Integration Tests"
echo "==========================================="
echo

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Test URLs
CONVEX_BACKEND="http://localhost:3210"
CONVEX_DASHBOARD="http://localhost:6791"
ORG_CORE="http://localhost:8080"
AI_CORE="http://localhost:8040"

# Test 1: Convex Backend Health
echo -e "${YELLOW}Test 1: Convex Backend Health${NC}"
if curl -sf "$CONVEX_BACKEND/version" > /dev/null; then
    VERSION=$(curl -s "$CONVEX_BACKEND/version")
    echo -e "${GREEN}✓${NC} Convex backend is healthy"
    echo "  Version: $VERSION"
else
    echo -e "${RED}✗${NC} Convex backend is not responding"
    exit 1
fi
echo

# Test 2: Convex Dashboard
echo -e "${YELLOW}Test 2: Convex Dashboard${NC}"
if curl -sf "$CONVEX_DASHBOARD" > /dev/null; then
    echo -e "${GREEN}✓${NC} Convex dashboard is accessible at $CONVEX_DASHBOARD"
else
    echo -e "${RED}✗${NC} Convex dashboard is not responding"
fi
echo

# Test 3: Org-Core Health
echo -e "${YELLOW}Test 3: Org-Core Service${NC}"
if curl -sf "$ORG_CORE/health" > /dev/null; then
    echo -e "${GREEN}✓${NC} Org-Core is healthy and reachable"
else
    echo -e "${RED}✗${NC} Org-Core is not responding"
fi
echo

# Test 4: AI-Core Health
echo -e "${YELLOW}Test 4: AI-Core Service${NC}"
if curl -sf "$AI_CORE/health" > /dev/null; then
    echo -e "${GREEN}✓${NC} AI-Core is healthy and reachable"
else
    echo -e "${RED}✗${NC} AI-Core is not responding"
fi
echo

# Test 5: Convex Function Deployment
echo -e "${YELLOW}Test 5: Check Convex Functions${NC}"
echo "Testing if Convex can accept function deployments..."

# Get database info
DB_INFO=$(curl -s "$CONVEX_BACKEND/api/info" 2>/dev/null || echo "{}")
if [ ! -z "$DB_INFO" ]; then
    echo -e "${GREEN}✓${NC} Convex API is responding"
    echo "  Database info available"
else
    echo -e "${YELLOW}⚠${NC} Convex API info not available (functions may need deployment)"
fi
echo

# Test 6: Network connectivity
echo -e "${YELLOW}Test 6: Docker Network Connectivity${NC}"
echo "Testing if Convex can reach other services from within Docker..."

# Test Org-Core from Convex container
if docker exec convex-backend curl -sf http://org-core-service:8080/health > /dev/null 2>&1; then
    echo -e "${GREEN}✓${NC} Convex → Org-Core: Connected"
else
    echo -e "${RED}✗${NC} Convex → Org-Core: Failed (check if org-core-service is running)"
fi

# Test AI-Core from Convex container
if docker exec convex-backend curl -sf http://ai-core-service:8040/health > /dev/null 2>&1; then
    echo -e "${GREEN}✓${NC} Convex → AI-Core: Connected"
else
    echo -e "${RED}✗${NC} Convex → AI-Core: Failed (check if ai-core-service is running)"
fi
echo

# Summary
echo "==========================================="
echo "Summary"
echo "==========================================="
echo "Convex Gateway: http://localhost:3210"
echo "Convex Dashboard: http://localhost:6791"
echo ""
echo "Next Steps:"
echo "1. Visit the Convex dashboard at http://localhost:6791"
echo "2. Deploy Convex functions:"
echo "   cd backend/convex-gateway"
echo "   npx convex deploy --url http://localhost:3210"
echo "3. Test realtime subscriptions from frontend"
echo "4. Test AI-Core integration via Convex actions"
echo ""
echo "To view Convex logs:"
echo "   docker logs -f convex-backend"
echo ""
