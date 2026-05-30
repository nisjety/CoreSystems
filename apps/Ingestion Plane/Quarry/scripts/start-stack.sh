#!/bin/bash
# Comprehensive startup script for Quarry stack
# This script ensures all dependencies are running before starting Quarry

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QUARRY_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
# Compose files to use (workspace-relative)
INGESTION_COMPOSE="/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion Plane/docker-compose.yml"
REASONING_COMPOSE="/Volumes/Lagring/Triodelab/CoreSystem/apps/Reasoning Plane/docker-compose.yml"

# AI core directory is managed via the Reasoning Plane compose file
AI_CORE_DIR="$(dirname "$REASONING_COMPOSE")"

echo "🚀 Starting Quarry Stack with all dependencies..."
echo ""

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

# Function to check if a service is running
check_service() {
    local service_name=$1
    local port=$2
    
    if nc -z localhost $port 2>/dev/null; then
        echo -e "${GREEN}✓${NC} $service_name is running on port $port"
        return 0
    else
        echo -e "${RED}✗${NC} $service_name is NOT running on port $port"
        return 1
    fi
}

# Function to wait for a service
wait_for_service() {
    local service_name=$1
    local port=$2
    local max_attempts=30
    local attempt=1
    
    echo -e "${YELLOW}⏳${NC} Waiting for $service_name on port $port..."
    
    while [ $attempt -le $max_attempts ]; do
        if nc -z localhost $port 2>/dev/null; then
            echo -e "${GREEN}✓${NC} $service_name is ready!"
            return 0
        fi
        sleep 2
        attempt=$((attempt + 1))
    done
    
    echo -e "${RED}✗${NC} $service_name did not start within expected time"
    return 1
}

echo "📦 Step 1: Checking if ai-core is running (via ${REASONING_COMPOSE})..."
if ! check_service "ai-core gRPC (host)" 50061; then
    echo -e "${YELLOW}Starting ai-core via Reasoning Plane compose...${NC}"

    if [ ! -f "$REASONING_COMPOSE" ]; then
        echo -e "${RED}ERROR: Reasoning Plane compose not found at $REASONING_COMPOSE${NC}"
        exit 1
    fi

    # Start ai-core using the reasoning-plane compose file
    docker compose -f "$REASONING_COMPOSE" up -d ai-core || true

    # Wait for ai-core to be ready (host ports mapped by compose)
    wait_for_service "ai-core gRPC (host)" 50061
    wait_for_service "ai-core HTTP (host)" 8100
else
    echo -e "${GREEN}ai-core (host) is already running${NC}"
fi

echo ""
echo "📦 Step 2: Starting Quarry docker stack (via ${INGESTION_COMPOSE})..."
cd "$QUARRY_DIR"

# Check if .env exists
if [ ! -f ".env" ]; then
    echo -e "${YELLOW}⚠️  No .env file found${NC}"
    echo "Creating .env from .env.example..."
    cp .env.example .env
    echo -e "${GREEN}✓ Created .env file${NC}"
fi

# Stop any existing containers to ensure clean start (use ingestion compose)
echo "Stopping existing Quarry containers (if any)..."
docker compose -f "$INGESTION_COMPOSE" down 2>/dev/null || true

# Build and start the ingestion-plane stack
echo "Building and starting Ingestion Plane stack..."
docker compose -f "$INGESTION_COMPOSE" build
docker compose -f "$INGESTION_COMPOSE" up -d

echo ""
echo "📦 Step 3: Waiting for services to be ready..."

# Wait for services (host ports per ingestion-compose)
wait_for_service "PostgreSQL (ingestion)" 9434

# Wait for Redis (host)
wait_for_service "Redis (ingestion)" 9380

# Wait for Temporal (host)
wait_for_service "Temporal (ingestion)" 9233

# Wait for Temporal UI (host)
wait_for_service "Temporal UI (ingestion)" 9081

# Wait for Quarry API (host)
wait_for_service "Quarry API (host)" 9090

echo ""
echo "🎉 All services are running!"
echo ""
echo "📊 Service Status:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Service              Port    Status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
check_service "AI Core gRPC (host)" 50061 && echo "  ✓" || echo "  ✗"
check_service "AI Core HTTP (host)" 8100 && echo "  ✓" || echo "  ✗"
check_service "Quarry API (host)" 9090 && echo "  ✓" || echo "  ✗"
check_service "Temporal (host)" 9233 && echo "  ✓" || echo "  ✗"
check_service "Temporal UI (host)" 9081 && echo "  ✓" || echo "  ✗"
check_service "PostgreSQL (ingestion)" 9434 && echo "  ✓" || echo "  ✗"
check_service "Redis (ingestion)" 9380 && echo "  ✓" || echo "  ✗"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

echo "🔗 Service URLs:"
echo "  • Quarry API:      http://localhost:8090"
echo "  • Temporal UI:     http://localhost:8089"
echo "  • AI Core:         http://localhost:8040"
echo "  • Health Check:    http://localhost:8090/health"
echo "  • Metrics:         http://localhost:8090/metrics"
echo ""

echo "📝 Next steps:"
echo "  • Run endpoint tests:    ./scripts/test-endpoints.sh"
echo "  • Run performance tests: ./scripts/test-performance.sh"
echo "  • View logs:            docker compose logs -f quarry-api"
echo "  • Stop services:        docker compose down"
echo ""

echo "✅ Quarry stack is ready for testing!"
