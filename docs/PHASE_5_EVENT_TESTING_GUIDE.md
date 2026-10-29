# Phase 5 Event Publishing Verification Guide

**Quick Reference for Testing Event Publishing**

---

## Infrastructure Status ✅

All Control Plane services are connected to shared NATS and ready to publish events:

```
✅ auth-core (port 3011) - Publishing user.registered events
✅ user-core (port 3012) - Publishing user.* domain events
✅ org-core (port 6061/8080) - Publishing org.* domain events
✅ billing-core (port 3014) - Publishing billing.* domain events
```

---

## Test Flows Available

### 1. User Registration (Creates user.registered Event)

**Via Auth Service:**
```bash
curl -X POST http://localhost:3011/api/v2/auth/register \
  -H "Content-Type: application/json" \
  -d '{
    "email": "testuser@example.com",
    "password": "SecurePassword123!",
    "name": "Test User"
  }'
```

**Expected Event:**
- Subject: `aqencia.controlplane.user.registered`
- Payload: `{user_id, email, name, provider: "local"}`
- Published by: auth-core service

---

### 2. Create Organization (Creates org.created Event)

**Via Org-Core:**
```bash
curl -X POST http://localhost:8080/orgs \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Test Organization",
    "ownerId": "user-id-here"
  }'
```

**Expected Event:**
- Subject: `aqencia.controlplane.org.created`
- Payload: `{org_id, name, owner_id}`
- Published by: org-core service

---

### 3. Link OAuth Provider (Creates user.provider_linked Event)

**Via User-Core:**
```bash
curl -X POST http://localhost:3012/api/v1/providers \
  -H "Content-Type: application/json" \
  -d '{
    "provider": "microsoft",
    "tenantId": "tenant-id",
    "accessToken": "token-here"
  }'
```

**Expected Event:**
- Subject: `aqencia.controlplane.user.provider_linked`
- Payload: `{user_id, email, provider: "microsoft", tenant_id}`
- Published by: user-core service

---

### 4. Update User (Creates user.updated Event)

**Via User-Core:**
```bash
curl -X PATCH http://localhost:3012/api/v1/users/:userId \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Updated Name",
    "email": "newemail@example.com"
  }'
```

**Expected Event:**
- Subject: `aqencia.controlplane.user.updated`
- Payload: `{user_id, email, changes: {name, email}}`
- Published by: user-core service

---

### 5. Update Organization Plan (Creates org.plan_changed Event)

**Via Org-Core or Billing-Core:**
```bash
curl -X PATCH http://localhost:6061:8080/orgs/:orgId \
  -H "Content-Type: application/json" \
  -d '{
    "plan": "professional",
    "tier": "paid"
  }'
```

**Expected Event:**
- Subject: `aqencia.controlplane.org.plan_changed`
- Payload: `{org_id, plan, tier}`
- Published by: org-core or billing-core

---

### 6. Record Billing Usage (Potential quota_exceeded Event)

**Via Billing-Core:**
```bash
curl -X POST http://localhost:3014/api/v1/billing/orgs/:orgId/usage \
  -H "Content-Type: application/json" \
  -d '{
    "metric": "api_calls",
    "amount": 1000
  }'
```

**Expected Event (if quota exceeded):**
- Subject: `aqencia.controlplane.billing.quota_exceeded`
- Payload: `{org_id, metric, limit, current}`
- Published by: billing-core

---

## Monitoring Events

### Option 1: Docker Logs (Simple)

```bash
# Watch auth-core for publishing
docker logs auth-service -f | grep -iE "publish|event|nats"

# Watch user-core for publishing
docker logs user-service -f | grep -iE "publish|event|nats"

# Watch org-core for publishing
docker logs org-core-service -f | grep -iE "publish|event|nats"

# Watch billing-core for publishing
docker logs billing-core-service -f | grep -iE "publish|event|nats"
```

### Option 2: NATS JetStream Monitoring (Direct)

```bash
# View stream info
docker exec controlplane-nats nats stream info AQENCIA_CONTROLPLANE

# View recent messages
docker exec controlplane-nats nats stream view AQENCIA_CONTROLPLANE

# Subscribe to specific subject
docker exec controlplane-nats nats sub 'aqencia.controlplane.user.*'

# Subscribe to all events
docker exec controlplane-nats nats sub 'aqencia.controlplane.>'
```

### Option 3: Custom Monitoring Script

```bash
#!/bin/bash

echo "Monitoring Control Plane Events on NATS..."
echo "Subject Pattern: aqencia.controlplane.>"
echo ""
echo "Watch for events in real-time:"

# Monitor each domain
echo "🔍 Monitoring user events..."
docker exec controlplane-nats nats sub 'aqencia.controlplane.user.>' &

echo "🔍 Monitoring org events..."
docker exec controlplane-nats nats sub 'aqencia.controlplane.org.>' &

echo "🔍 Monitoring billing events..."
docker exec controlplane-nats nats sub 'aqencia.controlplane.billing.>' &

wait
```

---

## Event Flow Diagram

```
┌─────────────────────────────────────────────────────────┐
│  Test Flow: User Registration → Org Creation → Link Provider
└─────────────────────────────────────────────────────────┘

1. Register User
   ↓
   curl -X POST http://localhost:3011/api/v2/auth/register
   ↓
   auth-core creates user in database
   ↓
   publishUserRegistered() called
   ↓
   Event: aqencia.controlplane.user.registered
   ↓
   Shared NATS publishes to AQENCIA_CONTROLPLANE stream
   ↓
   ✓ Event visible in JetStream

2. Create Organization
   ↓
   curl -X POST http://localhost:8080/orgs
   ↓
   org-core creates organization in database
   ↓
   publishOrgCreated() called
   ↓
   Event: aqencia.controlplane.org.created
   ↓
   Shared NATS publishes to AQENCIA_CONTROLPLANE stream
   ↓
   ✓ Event visible in JetStream

3. Link OAuth Provider
   ↓
   curl -X POST http://localhost:3012/api/v1/providers
   ↓
   user-core links provider to user
   ↓
   publishProviderLinked() called
   ↓
   Event: aqencia.controlplane.user.provider_linked
   ↓
   Shared NATS publishes to AQENCIA_CONTROLPLANE stream
   ↓
   ✓ Event visible in JetStream
   ↓
   ✓ Ingestion Plane can subscribe and setup M365 integration
```

---

## Troubleshooting

### Event Not Published?

1. **Check service is running:**
   ```bash
   docker-compose ps | grep -E "user-service|org-core|billing-core|auth-service"
   ```

2. **Check NATS connection in logs:**
   ```bash
   docker logs user-service | grep -i "connected to shared NATS"
   ```

3. **Check no errors in logs:**
   ```bash
   docker logs user-service | grep -iE "error|fail|panic"
   ```

4. **Verify SharedPublisher is set:**
   ```bash
   docker logs user-service | grep "connected to shared NATS for event publishing"
   ```

### NATS Stream Not Receiving?

1. **Check stream exists:**
   ```bash
   docker exec controlplane-nats nats stream list
   ```

2. **Check stream has subjects:**
   ```bash
   docker exec controlplane-nats nats stream info AQENCIA_CONTROLPLANE
   ```

3. **Check NATS broker is healthy:**
   ```bash
   docker-compose ps | grep controlplane-nats
   ```

---

## Key Event Subjects Reference

### User Events
- `aqencia.controlplane.user.registered` - New user created
- `aqencia.controlplane.user.updated` - User profile updated
- `aqencia.controlplane.user.deleted` - User deleted
- `aqencia.controlplane.user.provider_linked` - OAuth provider linked

### Organization Events
- `aqencia.controlplane.org.created` - New organization
- `aqencia.controlplane.org.updated` - Organization updated
- `aqencia.controlplane.org.deleted` - Organization deleted
- `aqencia.controlplane.org.plan_changed` - Plan changed
- `aqencia.controlplane.org.member_added` - Member invited
- `aqencia.controlplane.org.member_removed` - Member removed

### Billing Events
- `aqencia.controlplane.billing.account_updated` - Account settings changed
- `aqencia.controlplane.billing.quota_exceeded` - Quota threshold exceeded
- `aqencia.controlplane.billing.invoice_created` - Invoice generated
- `aqencia.controlplane.billing.plan_changed` - Plan changed

---

## Next Steps (Phase 6)

### Build Event Subscribers in Other Planes

1. **Ingestion Plane**
   - Subscribe to: `aqencia.controlplane.user.provider_linked`
   - Action: Setup M365 Graph API integration

2. **Data Plane**
   - Subscribe to: `aqencia.controlplane.org.plan_changed`
   - Action: Enforce usage quotas by plan

3. **Reasoning Plane**
   - Subscribe to: `aqencia.controlplane.billing.quota_exceeded`
   - Action: Restrict operations based on quota

### Create Cross-Plane E2E Tests

```bash
# Example: Full flow E2E test
1. Register user → Check user.registered event
2. Create org → Check org.created event
3. Link provider → Check user.provider_linked event
4. Verify Ingestion Plane received event and setup M365
5. Verify Data Plane enforces quotas
6. Verify Reasoning Plane respects limits
```

---

**Last Updated:** February 28, 2026  
**Phase Status:** Phase 5 Complete ✅ | Phase 6 Ready
