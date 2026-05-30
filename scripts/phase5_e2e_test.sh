#!/bin/bash

################################################################################
# Phase 5 E2E Event Publishing Test
# Tests event publishing across Control Plane services
################################################################################

set -e

echo "═══════════════════════════════════════════════════════════════════════════"
echo "  Phase 5: E2E Event Publishing Test"
echo "═══════════════════════════════════════════════════════════════════════════"
echo ""

# Configuration
CONTROL_PLANE_DIR="/Volumes/Lagring/Triodelab/CoreSystem/apps/Control Plane"
AUTH_SERVICE_URL="http://localhost:3011"
USER_SERVICE_URL="http://localhost:3012"
ORG_SERVICE_URL="http://localhost:6061"
BILLING_SERVICE_URL="http://localhost:3014"
COMPOSE_DIR="$CONTROL_PLANE_DIR"

cd "$COMPOSE_DIR"

################################################################################
# Test 1: Verify Service Health
################################################################################
echo "📋 Test 1: Verify Service Health"
echo "─────────────────────────────────────────────────────────────────────────"

services=("auth-service" "user-service" "org-core-service" "billing-core-service")
for service in "${services[@]}"; do
    health=$(docker inspect "$service" --format='{{.State.Health.Status}}' 2>/dev/null || echo "unknown")
    status=$(docker inspect "$service" --format='{{.State.Running}}' 2>/dev/null || echo "false")
    if [ "$status" = "true" ]; then
        echo "  ✅ $service: Running"
    else
        echo "  ❌ $service: Not running"
    fi
done
echo ""

################################################################################
# Test 2: Verify NATS Connection
################################################################################
echo "📋 Test 2: Verify NATS Connections in Service Logs"
echo "─────────────────────────────────────────────────────────────────────────"

for service in auth-service user-service org-core-service billing-core-service; do
    count=$(docker logs "$service" 2>&1 | grep -iE "connected to shared NATS|connected to NATS" | wc -l)
    if [ "$count" -gt 0 ]; then
        echo "  ✅ $service: NATS connected ($count messages)"
    else
        echo "  ⚠️  $service: No NATS connection logs found"
    fi
done
echo ""

################################################################################
# Test 3: Health Check Endpoints
################################################################################
echo "📋 Test 3: Health Check Endpoints"
echo "─────────────────────────────────────────────────────────────────────────"

services_and_ports=(
  "user-service|3012"
  "org-core-service|6061"
  "billing-core-service|3014"
)

for entry in "${services_and_ports[@]}"; do
    IFS='|' read -r service port <<< "$entry"
    response=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost:$port/health" 2>/dev/null || echo "000")
    if [ "$response" = "200" ]; then
        echo "  ✅ localhost:$port/health: $response"
    else
        echo "  ⚠️  localhost:$port/health: $response"
    fi
done
echo ""

################################################################################
# Test 4: Create Organization (org-core)
################################################################################
echo "📋 Test 4: Create Organization via org-core"
echo "─────────────────────────────────────────────────────────────────────────"

ORG_NAME="TestOrg-$(date +%s)"
ORG_OWNER_ID="test-user-$(date +%s)"

echo "  📝 Attempting to create organization: $ORG_NAME"

# Note: org-core API requires proper authentication
# This is a test to see the endpoint behavior
org_response=$(curl -s -X POST "$ORG_SERVICE_URL:8080/orgs" \
  -H "Content-Type: application/json" \
  -d "{
    \"name\": \"$ORG_NAME\",
    \"ownerId\": \"$ORG_OWNER_ID\"
  }" 2>/dev/null || echo "")

if [ -n "$org_response" ]; then
    echo "  ✅ Organization creation response received"
    # Try to extract org ID
    org_id=$(echo "$org_response" | grep -o '"id":"[^"]*' | head -1 | cut -d'"' -f4 || echo "")
    if [ -n "$org_id" ]; then
        echo "     Organization ID: $org_id"
    fi
else
    echo "  ⚠️  No response from organization endpoint"
fi
echo ""

################################################################################
# Test 5: Check Service Logs for Event Publishing
################################################################################
echo "📋 Test 5: Check Service Logs for Event Publishing"
echo "─────────────────────────────────────────────────────────────────────────"

echo "  📊 Event publishing status:"
echo ""

# Check user-service for user events
echo "  user-service:"
user_events=$(docker logs user-service 2>&1 | grep -iE "user\.registered|user\.updated|user\.deleted|user\.provider_linked|publishUser" | wc -l)
echo "    Found $user_events user-related event logs"

# Check org-core for org events
echo "  org-core-service:"
org_events=$(docker logs org-core-service 2>&1 | grep -iE "org\.created|org\.updated|org\.deleted|org\.plan_changed|publishOrg" | wc -l)
echo "    Found $org_events organization-related event logs"

# Check billing-core for billing events
echo "  billing-core-service:"
billing_events=$(docker logs billing-core-service 2>&1 | grep -iE "billing\.account_updated|billing\.quota_exceeded|billing\.invoice|billing\.plan_changed|publishAccount|publishQuota" | wc -l)
echo "    Found $billing_events billing-related event logs"

# Check auth-service for user events
echo "  auth-service:"
auth_events=$(docker logs auth-service 2>&1 | grep -iE "publishUserRegistration|user\.registered" | wc -l)
echo "    Found $auth_events authentication-related event logs"

echo ""

################################################################################
# Test 6: NATS Stream Status
################################################################################
echo "📋 Test 6: NATS JetStream Configuration"
echo "─────────────────────────────────────────────────────────────────────────"

echo "  Shared NATS Configuration:"
echo "    Broker: nats://velion-nats:4222"
echo "    Token: aqencia-shared-nats-token-2026"
echo "    Stream: AQENCIA_CONTROLPLANE"
echo "    Subject: aqencia.controlplane.>"
echo ""

# Try to check NATS connectivity via DNS
nats_host_status=$(getent hosts velion-nats 2>/dev/null && echo "✅ hostname resolves" || echo "⚠️  hostname resolution unavailable")
echo "  $nats_host_status"
echo ""

################################################################################
# Test 7: Event Publishing Infrastructure Check
################################################################################
echo "📋 Test 7: Event Publishing Infrastructure"
echo "─────────────────────────────────────────────────────────────────────────"

echo "  Checking SharedPublisher initialization in service logs:"
echo ""

services_check=(
  "user-service|user-core"
  "org-core-service|org-core"
  "billing-core-service|billing-core"
  "auth-service|auth-core"
)

for entry in "${services_check[@]}"; do
    IFS='|' read -r container service_name <<< "$entry"
    
    # Check for SharedPublisher initialization
    init_msg=$(docker logs "$container" 2>&1 | grep -iE "SharedPublisher initialized|connected to shared NATS" | head -1 || echo "")
    
    if [ -n "$init_msg" ]; then
        echo "  ✅ $service_name:"
        echo "     $init_msg" | sed 's/^/     /'
    else
        echo "  ⚠️  $service_name: No initialization message found"
    fi
done
echo ""

################################################################################
# Test 8: Sample Event Subjects
################################################################################
echo "📋 Test 8: Control Plane Event Subjects"
echo "─────────────────────────────────────────────────────────────────────────"

echo "  User Domain Events:"
echo "    • aqencia.controlplane.user.registered"
echo "    • aqencia.controlplane.user.updated"
echo "    • aqencia.controlplane.user.deleted"
echo "    • aqencia.controlplane.user.provider_linked"
echo ""

echo "  Organization Domain Events:"
echo "    • aqencia.controlplane.org.created"
echo "    • aqencia.controlplane.org.updated"
echo "    • aqencia.controlplane.org.deleted"
echo "    • aqencia.controlplane.org.plan_changed"
echo "    • aqencia.controlplane.org.member_added"
echo "    • aqencia.controlplane.org.member_removed"
echo ""

echo "  Billing Domain Events:"
echo "    • aqencia.controlplane.billing.account_updated"
echo "    • aqencia.controlplane.billing.quota_exceeded"
echo "    • aqencia.controlplane.billing.invoice_created"
echo "    • aqencia.controlplane.billing.plan_changed"
echo ""

################################################################################
# Test Summary
################################################################################
echo "═══════════════════════════════════════════════════════════════════════════"
echo "  Test Summary"
echo "═══════════════════════════════════════════════════════════════════════════"
echo ""
echo "✅ All infrastructure tests completed"
echo ""
echo "Next steps:"
echo "1. Trigger actual user/org creation flows to generate events"
echo "2. Monitor JetStream stream for published events"
echo "3. Verify cross-plane event consumption"
echo ""
echo "To manually test:"
echo "  • Register user via: curl -X POST http://localhost:3011/api/v2/auth/register"
echo "  • Create org via: curl -X POST http://localhost:6061:8080/orgs"
echo "  • Link provider via: curl -X POST http://localhost:3012/api/v1/providers"
echo ""
echo "═══════════════════════════════════════════════════════════════════════════"
