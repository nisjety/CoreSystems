#!/bin/bash
# Generate secure credentials for external services

set -e

echo "=== Generating Secure Credentials for External Services ==="
echo ""

# Function to generate secure random string
generate_secret() {
  openssl rand -hex "$1" | tr -d '\n'
}

# Output file
OUTPUT_FILE=".env.external-services.generated"

echo "Generating credentials..."
echo ""

# Zammad
ZAMMAD_DB_PASSWORD=$(generate_secret 16)
ZAMMAD_REDIS_PASSWORD=$(generate_secret 16)
ZAMMAD_API_TOKEN=$(generate_secret 32)

# Nango
NANGO_DB_PASSWORD=$(generate_secret 16)
NANGO_API_KEY=$(generate_secret 16)
NANGO_SECRET_KEY=$(generate_secret 32)
NANGO_ENCRYPTOR_KEY=$(generate_secret 32)

# Nohu
NOHU_DB_PASSWORD=$(generate_secret 16)
NOHU_REDIS_PASSWORD=$(generate_secret 16)
NOHU_API_KEY=$(generate_secret 16)
NOHU_SECRET_KEY=$(generate_secret 32)

cat > "$OUTPUT_FILE" << EOF
# External Services Credentials - GENERATED $(date)
# ⚠️  KEEP THIS FILE SECURE - Add to .gitignore
# Never commit to version control

# ==============================================
# ZAMMAD (Ticketing System)
# ==============================================
ZAMMAD_DB_USER=zammad
ZAMMAD_DB_PASSWORD=$ZAMMAD_DB_PASSWORD
ZAMMAD_REDIS_PASSWORD=$ZAMMAD_REDIS_PASSWORD
ZAMMAD_API_TOKEN=$ZAMMAD_API_TOKEN

# Zammad API Details
ZAMMAD_API_URL=http://localhost:3012/api/v1
ZAMMAD_API_USER=zammad@aquatiq.io
ZAMMAD_API_PASSWORD=$(generate_secret 16)

# ==============================================
# NANGO (API Connector Platform)
# ==============================================
NANGO_DB_USER=nango
NANGO_DB_PASSWORD=$NANGO_DB_PASSWORD
NANGO_API_KEY=$NANGO_API_KEY
NANGO_SECRET_KEY=$NANGO_SECRET_KEY
NANGO_ENCRYPTOR_KEY=$NANGO_ENCRYPTOR_KEY

# Nango API Details
NANGO_API_URL=http://localhost:3013
NANGO_WEBHOOK_URL=http://localhost:3013/webhooks

# ==============================================
# NOHU (Workflow Engine)
# ==============================================
NOHU_DB_USER=nohu
NOHU_DB_PASSWORD=$NOHU_DB_PASSWORD
NOHU_REDIS_PASSWORD=$NOHU_REDIS_PASSWORD
NOHU_API_KEY=$NOHU_API_KEY
NOHU_SECRET_KEY=$NOHU_SECRET_KEY

# Nohu API Details
NOHU_API_URL=http://localhost:3014
NOHU_WEBHOOK_URL=http://localhost:3014/webhooks

# ==============================================
# INTEGRATION SETTINGS
# ==============================================
EXTERNAL_SERVICES_NETWORK=external-services-net
DEBUG_EXTERNAL_SERVICES=false
EOF

echo -e "\033[0;32m✓ Credentials generated:\033[0m $OUTPUT_FILE"
echo ""
echo "Contents:"
echo "=========================================="
cat "$OUTPUT_FILE"
echo "=========================================="
echo ""
echo "⚠️  Next steps:"
echo "  1. Review the generated credentials"
echo "  2. Move to .env.external-services.local:"
echo "     mv $OUTPUT_FILE .env.external-services.local"
echo "  3. Add to .gitignore to prevent accidental commits"
echo "  4. Load when starting services:"
echo "     set -a && source .env.external-services.local && set +a"
echo "     docker-compose -f docker-compose.external-services.yml up -d"
echo ""
