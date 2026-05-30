#!/bin/bash
# Stop all Quarry and AI-Core services

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QUARRY_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
INGESTION_COMPOSE="/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion Plane/docker-compose.yml"
REASONING_COMPOSE="/Volumes/Lagring/Triodelab/CoreSystem/apps/Reasoning Plane/docker-compose.yml"
AI_CORE_DIR="$(dirname "$REASONING_COMPOSE")"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "🛑 Stopping Quarry Stack..."

cd "$QUARRY_DIR"
docker compose -f "$INGESTION_COMPOSE" down || true

echo -e "${GREEN}✓${NC} Quarry stack stopped"

echo ""
read -p "Stop ai-core as well? (y/N) " -n 1 -r
echo

if [[ $REPLY =~ ^[Yy]$ ]]; then
    if [ -f "$REASONING_COMPOSE" ]; then
        docker compose -f "$REASONING_COMPOSE" down || true
        echo -e "${GREEN}✓${NC} AI-Core stack stopped"
    else
        echo -e "${YELLOW}⚠️${NC}  Reasoning Plane compose not found at $REASONING_COMPOSE"
    fi
fi

echo ""
echo "✅ Services stopped"
