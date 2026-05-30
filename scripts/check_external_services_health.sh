#!/bin/bash
# Check health of external services (Zammad, Nango, Nohu)

set -e

echo "=== External Services Health Check ==="
echo ""

# Color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Load env if available
if [ -f .env.external-services.local ]; then
  set -a
  source .env.external-services.local
  set +a
elif [ -f .env.external-services ]; then
  set -a
  source .env.external-services
  set +a
fi

check_service() {
  local name=$1
  local url=$2
  local port=$3
  local method=${4:-"GET"}
  
  echo -n "Checking $name (port $port)... "
  
  if [ "$method" == "GET" ]; then
    if curl -s -f "$url" > /dev/null 2>&1; then
      echo -e "${GREEN}✓ Running${NC}"
      return 0
    fi
  fi
  
  echo -e "${RED}✗ Failed${NC}"
  return 1
}

check_database() {
  local name=$1
  local container=$2
  local user=$3
  local password=$4
  
  echo -n "Checking $name database... "
  
  if docker exec "$container" psql -U "$user" -c "SELECT 1" > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Connected${NC}"
    return 0
  else
    echo -e "${RED}✗ Failed${NC}"
    return 1
  fi
}

check_redis() {
  local name=$1
  local container=$2
  local password=$3
  
  echo -n "Checking $name Redis... "
  
  if docker exec "$container" redis-cli -a "$password" ping > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Connected${NC}"
    return 0
  else
    echo -e "${RED}✗ Failed${NC}"
    return 1
  fi
}

# Check if services are running
echo "=== Service Status ==="
docker-compose -f docker-compose.external-services.yml ps || true
echo ""

# Check API endpoints
echo "=== API Endpoints ==="
check_service "Zammad" "http://localhost:3012/api/v1/tickets" 3012 || true
check_service "Nango" "http://localhost:3013/health" 3013 || true
check_service "Nohu" "http://localhost:3014/health" 3014 || true
echo ""

# Check databases
echo "=== Databases ==="
check_database "Zammad" "zammad-postgres" "${ZAMMAD_DB_USER:-zammad}" "${ZAMMAD_DB_PASSWORD:-zammad_secure_password}" || true
check_database "Nango" "nango-postgres" "${NANGO_DB_USER:-nango}" "${NANGO_DB_PASSWORD:-nango_secure_password}" || true
check_database "Nohu" "nohu-postgres" "${NOHU_DB_USER:-nohu}" "${NOHU_DB_PASSWORD:-nohu_secure_password}" || true
echo ""

# Check Redis instances
echo "=== Redis Caches ==="
check_redis "Zammad" "zammad-redis" "${ZAMMAD_REDIS_PASSWORD:-zammad_redis_password}" || true
check_redis "Nohu" "nohu-redis" "${NOHU_REDIS_PASSWORD:-nohu_redis_password}" || true
echo ""

# Network check
echo "=== Network Status ==="
if docker network inspect external-services-net > /dev/null 2>&1; then
  echo -e "${GREEN}✓ Network 'external-services-net' exists${NC}"
  docker network inspect external-services-net | grep -A 10 "Containers" || true
else
  echo -e "${RED}✗ Network 'external-services-net' not found${NC}"
fi
echo ""

echo "=== Summary ==="
echo "Start all services:"
echo "  docker-compose -f docker-compose.external-services.yml up -d"
echo ""
echo "Stop all services:"
echo "  docker-compose -f docker-compose.external-services.yml down"
echo ""
echo "View logs:"
echo "  docker-compose -f docker-compose.external-services.yml logs -f [zammad|nango|nohu]"
