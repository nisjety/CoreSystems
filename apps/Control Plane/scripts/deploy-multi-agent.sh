#!/bin/bash
# Multi-Agent AI System Deployment Script
# Sets up LangGraph, Letta, and LangChain integration

set -e

echo "🚀 AI-Core Multi-Agent System Deployment"
echo "=========================================="
echo ""

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Check if running from correct directory
if [ ! -f "docker-compose.yml" ]; then
    echo -e "${RED}❌ Error: Must run from backend/ directory${NC}"
    exit 1
fi

echo -e "${YELLOW}Step 1: Installing Python dependencies...${NC}"
cd ai-core
pip install -r requirements.txt
cd ..
echo -e "${GREEN}✅ Dependencies installed${NC}"
echo ""

echo -e "${YELLOW}Step 2: Checking database connections...${NC}"
# Check if Aquatiq PostgreSQL is running
if docker exec aquatiq-postgres psql -U aquatiq -c "SELECT 1" > /dev/null 2>&1; then
    echo -e "${GREEN}✅ Aquatiq PostgreSQL connected${NC}"
else
    echo -e "${RED}❌ Error: Aquatiq PostgreSQL not accessible${NC}"
    echo "   Start Aquatiq Root Container first"
    exit 1
fi

# Create Letta database
echo -e "${YELLOW}   Creating letta_db database...${NC}"
docker exec aquatiq-postgres psql -U aquatiq -c "CREATE DATABASE letta_db;" 2>/dev/null || echo "   Database already exists"
echo -e "${GREEN}✅ letta_db ready${NC}"
echo ""

echo -e "${YELLOW}Step 3: Starting Letta Server...${NC}"
docker compose up -d letta-server
echo "   Waiting for Letta server to be ready..."
sleep 5

# Check Letta health
LETTA_HEALTH=$(curl -s http://localhost:8283/health || echo "failed")
if [[ "$LETTA_HEALTH" == *"ok"* ]] || [[ "$LETTA_HEALTH" == *"healthy"* ]]; then
    echo -e "${GREEN}✅ Letta server running at http://localhost:8283${NC}"
else
    echo -e "${YELLOW}⚠️  Letta server starting... (may take 30-60 seconds)${NC}"
fi
echo ""

echo -e "${YELLOW}Step 4: Rebuilding AI-Core with new dependencies...${NC}"
docker compose build ai-core
echo -e "${GREEN}✅ AI-Core rebuilt${NC}"
echo ""

echo -e "${YELLOW}Step 5: Starting AI-Core...${NC}"
docker compose up -d ai-core
echo "   Waiting for AI-Core to be ready..."
sleep 10

# Check AI-Core health
AI_HEALTH=$(curl -s http://localhost:8000/health || echo "failed")
if [[ "$AI_HEALTH" == *"healthy"* ]] || [[ "$AI_HEALTH" == *"ok"* ]]; then
    echo -e "${GREEN}✅ AI-Core running at http://localhost:8000${NC}"
else
    echo -e "${YELLOW}⚠️  AI-Core starting... check logs: docker compose logs ai-core${NC}"
fi
echo ""

echo -e "${YELLOW}Step 6: Verifying services...${NC}"
echo "   Checking service status..."
docker compose ps letta-server ai-core
echo ""

echo -e "${GREEN}=========================================="
echo "✅ Multi-Agent AI System Deployed!"
echo "==========================================${NC}"
echo ""
echo "📊 Service Status:"
echo "   - Letta Server:  http://localhost:8283"
echo "   - AI-Core API:   http://localhost:8000"
echo "   - API Docs:      http://localhost:8000/docs"
echo ""
echo "📚 Next Steps:"
echo "   1. Set ANTHROPIC_API_KEY in .env for Claude Sonnet 4.5"
echo "   2. Test Intent Engine: see docs/MULTI_AGENT_INTEGRATION.md"
echo "   3. Test Tool Calling: see examples in integration guide"
echo "   4. Monitor logs: docker compose logs -f ai-core letta-server"
echo ""
echo "📖 Documentation:"
echo "   - Integration Guide: ai-core/docs/MULTI_AGENT_INTEGRATION.md"
echo "   - Layer Architecture: ai-core/docs/LAYER_ARCHITECTURE.md"
echo ""
echo -e "${YELLOW}⚠️  Important: Add ANTHROPIC_API_KEY to .env for Claude Sonnet 4.5${NC}"
echo ""
