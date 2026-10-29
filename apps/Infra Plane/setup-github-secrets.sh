#!/bin/bash
# Setup GitHub Secrets for Aquatiq Production Deployment
# Run this script to add all sensitive credentials to GitHub Secrets
# Prerequisites: gh CLI installed and authenticated (gh auth login)

set -e

echo "🔐 Setting up GitHub Secrets for Aquatiq..."
echo "==========================================="
echo ""

# Check if gh CLI is installed
if ! command -v gh &> /dev/null; then
    echo "❌ ERROR: GitHub CLI (gh) is not installed."
    echo "Install it from: https://cli.github.com/"
    exit 1
fi

# Get current repository
REPO=$(git config --get remote.origin.url | sed 's/.*:\|.git//g')
if [ -z "$REPO" ]; then
    echo "❌ ERROR: Could not determine repository. Make sure you're in a git repository."
    exit 1
fi

echo "📦 Repository: $REPO"
echo ""

# Required Credentials for docker-compose.flexible.yml services
echo "📋 REQUIRED CREDENTIALS (from docker-compose.flexible.yml)"
echo "==========================================================="
echo ""

read -p "Domain (e.g., coresystem.com): " DOMAIN
echo ""

read -sp "PostgreSQL Password: " POSTGRES_PASSWORD
echo ""
read -sp "Redis Password: " REDIS_PASSWORD
echo ""
read -sp "NATS Auth Token: " NATS_AUTH_TOKEN
echo ""
read -sp "N8N Encryption Key: " N8N_ENCRYPTION_KEY
echo ""
read -sp "Integration Gateway API Key: " INTEGRATION_GATEWAY_API_KEY
echo ""
read -p "pgAdmin Email: " PGADMIN_EMAIL
echo ""
read -sp "pgAdmin Password: " PGADMIN_PASSWORD
echo ""
read -sp "Grafana Admin Password: " GRAFANA_PASSWORD
echo ""
read -sp "Traefik Dashboard Auth (htpasswd hash): " TRAEFIK_DASHBOARD_AUTH
echo ""

echo ""
echo "✅ Validating inputs..."

# Validate required fields
REQUIRED_FIELDS=("POSTGRES_PASSWORD" "REDIS_PASSWORD" "NATS_AUTH_TOKEN" "N8N_ENCRYPTION_KEY" "DOMAIN" "INTEGRATION_GATEWAY_API_KEY" "PGADMIN_EMAIL" "PGADMIN_PASSWORD" "GRAFANA_PASSWORD" "TRAEFIK_DASHBOARD_AUTH")
for field in "${REQUIRED_FIELDS[@]}"; do
    if [ -z "${!field}" ]; then
        echo "❌ ERROR: $field is required"
        exit 1
    fi
done

echo "✅ All required fields provided"
echo ""
echo "🚀 Adding secrets to GitHub..."
echo ""

# Add secrets to GitHub
gh secret set POSTGRES_PASSWORD --body "$POSTGRES_PASSWORD" --repo "$REPO"
gh secret set REDIS_PASSWORD --body "$REDIS_PASSWORD" --repo "$REPO"
gh secret set NATS_AUTH_TOKEN --body "$NATS_AUTH_TOKEN" --repo "$REPO"
gh secret set N8N_ENCRYPTION_KEY --body "$N8N_ENCRYPTION_KEY" --repo "$REPO"
gh secret set DOMAIN --body "$DOMAIN" --repo "$REPO"
gh secret set INTEGRATION_GATEWAY_API_KEY --body "$INTEGRATION_GATEWAY_API_KEY" --repo "$REPO"
gh secret set PGADMIN_EMAIL --body "$PGADMIN_EMAIL" --repo "$REPO"
gh secret set PGADMIN_PASSWORD --body "$PGADMIN_PASSWORD" --repo "$REPO"
gh secret set GRAFANA_PASSWORD --body "$GRAFANA_PASSWORD" --repo "$REPO"
gh secret set TRAEFIK_DASHBOARD_AUTH --body "$TRAEFIK_DASHBOARD_AUTH" --repo "$REPO"

echo ""
echo "✅ All secrets added to GitHub!"
echo ""
echo "📋 Secrets created:"
echo "   ✓ POSTGRES_PASSWORD"
echo "   ✓ REDIS_PASSWORD"
echo "   ✓ NATS_AUTH_TOKEN"
echo "   ✓ N8N_ENCRYPTION_KEY"
echo "   ✓ DOMAIN"
echo "   ✓ INTEGRATION_GATEWAY_API_KEY"
echo "   ✓ PGADMIN_EMAIL"
echo "   ✓ PGADMIN_PASSWORD"
echo "   ✓ GRAFANA_PASSWORD"
echo "   ✓ TRAEFIK_DASHBOARD_AUTH"
echo ""
echo "Next: Update your GitHub Actions workflow to use these secrets!"
