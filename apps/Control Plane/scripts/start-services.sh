#!/bin/bash
# Setup and start all backend services with Aquatiq Root Container

set -e

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}🚀 Starting CoreSystem Backend Services${NC}"
echo ""

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo -e "${RED}❌ Docker is not running. Please start Docker first.${NC}"
    exit 1
fi

# Check if Aquatiq Root Container is running
echo -e "${YELLOW}📋 Checking Aquatiq Root Container...${NC}"
required_containers=("aquatiq-postgres-local" "aquatiq-redis-local" "aquatiq-nats-local")
all_running=true

for container in "${required_containers[@]}"; do
    if ! docker ps --format '{{.Names}}' | grep -q "^${container}$"; then
        echo -e "${RED}❌ ${container} is not running${NC}"
        all_running=false
    fi
done

if [ "$all_running" = false ]; then
    echo -e "${RED}❌ Aquatiq Root Container is not running${NC}"
    echo -e "${YELLOW}Starting Aquatiq Root Container...${NC}"
    
    if [ -d "/Volumes/Lagring/Aquatiq/aquatiq-root-container" ]; then
        cd /Volumes/Lagring/Aquatiq/aquatiq-root-container
        ./start-local.sh
        cd - > /dev/null
    else
        echo -e "${RED}❌ Aquatiq Root Container not found at /Volumes/Lagring/Aquatiq/aquatiq-root-container${NC}"
        echo "Please start it manually first."
        exit 1
    fi
fi

echo -e "${GREEN}✅ Aquatiq Root Container is running${NC}"
echo ""

# Check if aquatiq-local network exists
if ! docker network ls --format '{{.Name}}' | grep -q "^aquatiq-local$"; then
    echo -e "${RED}❌ Network aquatiq-local does not exist${NC}"
    exit 1
fi

# Create databases if they don't exist
echo -e "${YELLOW}📋 Setting up databases...${NC}"
databases=("aquatiq_dev" "user_service" "admin_service" "auth_service" "ai_core")

for db in "${databases[@]}"; do
    if docker exec aquatiq-postgres-local psql -U aquatiq -lqt | cut -d \| -f 1 | grep -qw "$db"; then
        echo -e "${GREEN}✅ Database $db exists${NC}"
    else
        echo -e "${YELLOW}Creating database $db...${NC}"
        docker exec aquatiq-postgres-local psql -U aquatiq -c "CREATE DATABASE $db;"
        echo -e "${GREEN}✅ Database $db created${NC}"
    fi
done

echo ""

# Build and start services
echo -e "${YELLOW}🔨 Building services...${NC}"
docker compose build

echo ""
echo -e "${YELLOW}🚀 Starting services...${NC}"
docker compose up -d

echo ""
echo -e "${YELLOW}⏳ Waiting for services to be healthy...${NC}"
sleep 10

# Check service health
echo ""
echo -e "${BLUE}📊 Service Status:${NC}"
docker compose ps

echo ""
echo -e "${GREEN}✅ All services started!${NC}"
echo ""
echo -e "${BLUE}Service URLs:${NC}"
echo "  Org Core:     http://localhost:8080 (HTTP) | localhost:9090 (gRPC)"
echo "  User Service: http://localhost:3012 (HTTP) | localhost:50012 (gRPC)"
echo "  Billing Core: http://localhost:3014 (HTTP) | localhost:50013 (gRPC)"
echo "  Auth Service: http://localhost:3000 (HTTP)"
echo "  AI Core:      http://localhost:8000 (HTTP) | localhost:50014 (gRPC)"
echo ""
echo -e "${BLUE}Shared Infrastructure:${NC}"
echo "  PostgreSQL:   localhost:5432 (aquatiq/postgres)"
echo "  Redis:        localhost:6379 (password: redis)"
echo "  NATS:         localhost:4222 (token: nats)"
echo "  pgAdmin:      http://localhost:5050"
echo "  RedisInsight: http://localhost:5540"
echo ""
echo -e "${BLUE}Useful Commands:${NC}"
echo "  View logs:        docker compose logs -f [service]"
echo "  Stop services:    docker compose stop"
echo "  Restart service:  docker compose restart [service]"
echo "  Monitor NATS:     nats sub \">\" --server=nats://localhost:4222 --token=nats"
echo ""
