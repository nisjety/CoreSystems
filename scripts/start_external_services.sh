#!/bin/bash
# Quick startup script for external services

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

echo "=== Starting External Services (Zammad, Nango, Nohu) ==="
echo ""

# Check for credentials file
if [ ! -f "$ROOT_DIR/.env.external-services.local" ]; then
  echo "⚠️  Credentials file not found: .env.external-services.local"
  echo ""
  echo "Generating credentials..."
  bash "$SCRIPT_DIR/generate_external_services_credentials.sh"
  echo ""
  echo "Now copy the generated credentials:"
  echo "  mv .env.external-services.generated .env.external-services.local"
  echo ""
  echo "Then run this script again."
  exit 1
fi

# Load credentials
set -a
source "$ROOT_DIR/.env.external-services.local"
set +a

echo "Starting services..."
echo ""

# Start services
docker-compose -f "$ROOT_DIR/docker-compose.external-services.yml" up -d

echo ""
echo "✓ Services starting..."
echo ""
echo "Waiting for services to be ready (60 seconds)..."
echo ""

# Wait for services
for i in {1..12}; do
  echo -n "."
  sleep 5
done
echo ""
echo ""

# Run health check
bash "$SCRIPT_DIR/check_external_services_health.sh"

echo ""
echo "=== Setup Complete ==="
echo ""
echo "API Endpoints:"
echo "  • Zammad:  http://localhost:3012/api/v1"
echo "  • Nango:   http://localhost:3013"
echo "  • Nohu:    http://localhost:3014"
echo ""
echo "API Keys (from .env.external-services.local):"
echo "  • ZAMMAD_API_TOKEN: $ZAMMAD_API_TOKEN"
echo "  • NANGO_API_KEY: $NANGO_API_KEY"
echo "  • NOHU_API_KEY: $NOHU_API_KEY"
echo ""
echo "View logs:"
echo "  docker-compose -f docker-compose.external-services.yml logs -f [service-name]"
echo ""
echo "Stop services:"
echo "  docker-compose -f docker-compose.external-services.yml down"
echo ""
