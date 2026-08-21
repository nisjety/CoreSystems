#!/bin/bash

# Convex Gateway Setup Script
# This script sets up and starts the Convex self-hosted backend

set -e

echo "==================================="
echo "Convex Gateway Setup"
echo "==================================="
echo

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "❌ Docker is not running. Please start Docker and try again."
    exit 1
fi

# Check if coresystem-local network exists
if ! docker network inspect coresystem-local > /dev/null 2>&1; then
    echo "📡 Creating coresystem-local Docker network..."
    docker network create coresystem-local
else
    echo "✅ Docker network coresystem-local already exists"
fi

# Check if .env.local exists
if [ ! -f .env.local ]; then
    echo "⚠️  .env.local not found. Creating from template..."
    cp .env.local .env.local
    echo "✅ Created .env.local - please review and update values"
fi

echo
echo "📦 Starting Convex services..."
docker compose up -d

echo
echo "⏳ Waiting for services to be healthy..."
sleep 10

# Check if backend is ready
if ! docker compose exec backend curl -f http://localhost:3210/version > /dev/null 2>&1; then
    echo "⚠️  Backend not ready yet, waiting longer..."
    sleep 20
fi

echo
echo "🔑 Generating admin key..."
# generate_admin_key.sh echoes the bare key ("<instance>|<secret>") and nothing
# else -- it never prints an "Admin key:" label -- and the service is named
# convex-backend, not backend. The old parse therefore always yielded an empty
# string and fell through to the warning below, which is why .env.local was
# never written and convex-gateway crash-looped on an empty admin key.
# Select the key by its '|' separator so any future banner lines are ignored.
ADMIN_KEY=$(docker compose exec -T convex-backend ./generate_admin_key.sh 2>/dev/null | tr -d '\r' | grep -E '\|' | tail -n 1 | tr -d '[:space:]')

if [ -z "$ADMIN_KEY" ]; then
    echo "⚠️  Could not auto-generate admin key. Run manually:"
    echo "   docker compose exec convex-backend ./generate_admin_key.sh"
else
    echo "✅ Admin key generated: $ADMIN_KEY"
    
    # Update .env.local with admin key
    if grep -q "CONVEX_ADMIN_KEY=" .env.local; then
        sed -i.bak "s|CONVEX_ADMIN_KEY=.*|CONVEX_ADMIN_KEY=$ADMIN_KEY|" .env.local
        rm .env.local.bak
    else
        echo "CONVEX_ADMIN_KEY=$ADMIN_KEY" >> .env.local
    fi
    
    echo "✅ Updated .env.local with admin key"
fi

echo
echo "📦 Installing Node dependencies..."
if [ -f package.json ]; then
    npm install
else
    echo "⚠️  package.json not found, skipping npm install"
fi

echo
echo "==================================="
echo "✅ Convex Gateway Setup Complete!"
echo "==================================="
echo
echo "Services running:"
echo "  • Backend:   http://localhost:3210"
echo "  • Dashboard: http://localhost:6791"
echo "  • HTTP Actions: http://localhost:3211"
echo
echo "Next steps:"
echo "  1. Visit dashboard at http://localhost:6791"
echo "  2. Run 'npm run dev' to start Convex development server"
echo "  3. Deploy functions with 'npm run deploy'"
echo
echo "Useful commands:"
echo "  • View logs:     docker compose logs -f"
echo "  • Stop services: docker compose down"
echo "  • Restart:       docker compose restart"
echo
