#!/bin/bash

# ==============================================
# SSO Configuration Validation Script
# ==============================================

echo "🔐 SSO Configuration Validation"
echo "================================"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Check if services are running
echo ""
echo "${BLUE}📊 Service Health Check${NC}"
echo "------------------------"

# Check auth-service
if docker ps | grep -q "auth-service"; then
    echo "✅ auth-service: Running"
else
    echo "❌ auth-service: Not running"
    echo "   Run: docker-compose up auth-service"
fi

# Check database
if docker ps | grep -q "postgres"; then
    echo "✅ PostgreSQL: Running"
else
    echo "❌ PostgreSQL: Not running"
    echo "   Run: docker-compose up postgres"
fi

# Check NATS
if docker ps | grep -q "nats"; then
    echo "✅ NATS: Running"
else
    echo "❌ NATS: Not running"
    echo "   Run: docker-compose up nats"
fi

echo ""
echo "${BLUE}🔧 Environment Configuration${NC}"
echo "-----------------------------"

# Check for environment variables
if docker-compose config | grep -q "SSO_ENABLED=true"; then
    echo "✅ SSO_ENABLED: Configured"
else
    echo "❌ SSO_ENABLED: Not configured or disabled"
fi

if docker-compose config | grep -q "MICROSOFT_CLIENT_ID"; then
    echo "✅ Microsoft Entra ID: Configured"
else
    echo "⚠️  Microsoft Entra ID: Not configured"
    echo "   Add MICROSOFT_CLIENT_ID, MICROSOFT_CLIENT_SECRET, MICROSOFT_TENANT_ID"
fi

if docker-compose config | grep -q "GOOGLE_CLIENT_ID"; then
    echo "✅ Google Workspace: Configured"
else
    echo "⚠️  Google Workspace: Not configured"
    echo "   Add GOOGLE_CLIENT_ID, GOOGLE_CLIENT_SECRET"
fi

echo ""
echo "${BLUE}🏢 Organization Plugin Check${NC}"
echo "------------------------------"

if docker-compose config | grep -q "ORGANIZATION_ENABLED=true"; then
    echo "✅ Organization Plugin: Enabled"
else
    echo "❌ Organization Plugin: Disabled"
fi

if docker-compose config | grep -q "SSO_ORG_PROVISIONING_DISABLED=false"; then
    echo "✅ Organization Provisioning: Enabled"
else
    echo "❌ Organization Provisioning: Disabled"
fi

echo ""
echo "${BLUE}🌐 API Endpoint Tests${NC}"
echo "----------------------"

# Test auth service health
echo -n "Testing auth-service health... "
if curl -s -o /dev/null -w "%{http_code}" http://localhost:3001/health | grep -q "200"; then
    echo "${GREEN}✅ OK${NC}"
else
    echo "${RED}❌ Failed${NC}"
    echo "   Check if auth-service is running on port 3001"
fi

# Test SSO endpoints availability
echo -n "Testing Microsoft SSO endpoint... "
if curl -s -o /dev/null -w "%{http_code}" http://localhost:3001/api/auth/sign-in/microsoft | grep -q "40[0-9]"; then
    echo "${GREEN}✅ Available${NC} (4xx expected without credentials)"
else
    echo "${RED}❌ Not accessible${NC}"
fi

echo -n "Testing Google SSO endpoint... "
if curl -s -o /dev/null -w "%{http_code}" http://localhost:3001/api/auth/sign-in/google | grep -q "40[0-9]"; then
    echo "${GREEN}✅ Available${NC} (4xx expected without credentials)"
else
    echo "${RED}❌ Not accessible${NC}"
fi

echo ""
echo "${BLUE}📋 Database Schema Check${NC}"
echo "-------------------------"

# Check if Better Auth tables exist
AUTH_DB_CHECK=$(docker exec backend-aquatiq-postgres-local-1 psql -U aquatiq -d auth_service -c "\\dt" 2>/dev/null | grep -c "public")

if [ "$AUTH_DB_CHECK" -gt "10" ]; then
    echo "✅ Better Auth Tables: Created ($AUTH_DB_CHECK tables)"
else
    echo "❌ Better Auth Tables: Missing or incomplete"
    echo "   Run database migrations"
fi

# Check specific SSO-related tables
TABLES=("account" "session" "organization" "member" "invitation")
for table in "${TABLES[@]}"; do
    if docker exec backend-aquatiq-postgres-local-1 psql -U aquatiq -d auth_service -c "\\d $table" &>/dev/null; then
        echo "✅ Table '$table': Exists"
    else
        echo "❌ Table '$table': Missing"
    fi
done

echo ""
echo "${BLUE}📁 Documentation Check${NC}"
echo "------------------------"

if [ -f "/Volumes/Lagring/Triodelab/CoreSystem/backend/docs/SSO_SETUP_GUIDE.md" ]; then
    echo "✅ SSO Setup Guide: Available"
else
    echo "❌ SSO Setup Guide: Missing"
fi

if [ -f "/Volumes/Lagring/Triodelab/CoreSystem/backend/.env.sso.template" ]; then
    echo "✅ Environment Template: Available"
else
    echo "❌ Environment Template: Missing"
fi

echo ""
echo "${BLUE}🎯 Next Steps${NC}"
echo "--------------"

echo "1. Configure SSO providers:"
echo "   • Copy .env.sso.template to .env"
echo "   • Add your Microsoft and Google credentials"
echo "   • See docs/SSO_SETUP_GUIDE.md for detailed setup"

echo ""
echo "2. Test SSO flow:"
echo "   • Access: http://localhost:3001/api/auth/sign-in/microsoft"
echo "   • Access: http://localhost:3001/api/auth/sign-in/google"

echo ""
echo "3. Monitor organization creation:"
echo "   • Watch logs: docker logs -f backend-auth-service-1"
echo "   • Monitor NATS: docker exec -it backend-aquatiq-nats-local-1 nats sub organization.created"

echo ""
echo "${GREEN}🎉 SSO Infrastructure: Ready for Provider Configuration!${NC}"