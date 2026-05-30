# External Services Setup Guide
## Zammad, Nango, and Nohu (API-Only Mode)

This guide covers setting up three external integration services as background API services with no web UIs.

---

## Quick Start

### 1. Load Credentials
```bash
# Copy and customize the credentials file
cp .env.external-services .env.external-services.local

# Edit with your secure passwords
nano .env.external-services.local
```

### 2. Start Services
```bash
# Start all three services with their databases and Redis caches
docker-compose -f docker-compose.external-services.yml up -d

# Verify all services are running
docker-compose -f docker-compose.external-services.yml ps

# Check health of all services
./scripts/check_external_services_health.sh
```

### 3. Verify Connectivity
```bash
# Test Zammad API
curl -X GET http://localhost:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN"

# Test Nango API
curl -X GET http://localhost:3013/health \
  -H "X-API-Key: $NANGO_API_KEY"

# Test Nohu API
curl -X GET http://localhost:3014/health \
  -H "X-API-Key: $NOHU_API_KEY"
```

---

## Service Details

### Zammad (Ticketing System)
**Purpose:** Customer support ticketing and issue tracking  
**API Port:** 3012  
**Database:** PostgreSQL (zammad-postgres)  
**Cache:** Redis (zammad-redis)

#### API Examples
```bash
# Get all tickets
curl -X GET http://localhost:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN"

# Create a ticket
curl -X POST http://localhost:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "Issue title",
    "body": "Issue description",
    "customer": "customer@example.com",
    "priority_id": 2,
    "state_id": 1
  }'

# Get ticket by ID
curl -X GET http://localhost:3012/api/v1/tickets/1 \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN"
```

#### Credentials
- **API Key:** `$ZAMMAD_API_TOKEN`
- **DB User:** `$ZAMMAD_DB_USER`
- **DB Password:** `$ZAMMAD_DB_PASSWORD`
- **Redis Password:** `$ZAMMAD_REDIS_PASSWORD`

---

### Nango (API Connector Platform)
**Purpose:** Universal API integration layer and OAuth connector  
**API Port:** 3013  
**Database:** PostgreSQL (nango-postgres)

#### API Examples
```bash
# List integrations
curl -X GET http://localhost:3013/integrations \
  -H "X-API-Key: $NANGO_API_KEY"

# Create integration
curl -X POST http://localhost:3013/integrations \
  -H "X-API-Key: $NANGO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hubspot",
    "auth_type": "oauth2",
    "config": {
      "client_id": "YOUR_CLIENT_ID",
      "client_secret": "YOUR_CLIENT_SECRET"
    }
  }'

# OAuth callback (after user authorizes)
curl -X POST http://localhost:3013/oauth/callback \
  -H "X-API-Key: $NANGO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "integration": "hubspot",
    "code": "AUTH_CODE_FROM_PROVIDER",
    "state": "STATE_PARAM"
  }'

# Sync data from connected service
curl -X POST http://localhost:3013/sync \
  -H "X-API-Key: $NANGO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "integration": "hubspot",
    "connection_id": "user_123",
    "model": "Contact"
  }'
```

#### Credentials
- **API Key:** `$NANGO_API_KEY`
- **Secret Key:** `$NANGO_SECRET_KEY`
- **Encryptor Key:** `$NANGO_ENCRYPTOR_KEY`
- **DB User:** `$NANGO_DB_USER`
- **DB Password:** `$NANGO_DB_PASSWORD`

#### Supported Integrations
Nango supports 500+ APIs out-of-the-box:
- **CRM:** Salesforce, HubSpot, Pipedrive, Zoho
- **Communication:** Gmail, Slack, Microsoft Teams, Discord
- **Project Mgmt:** Asana, Monday.com, Jira, Linear
- **Accounting:** QuickBooks, FreshBooks, Xero
- Custom API connectors via webhook

---

### Nohu (Workflow Engine)
**Purpose:** Orchestrate multi-step workflows and automation  
**API Port:** 3014  
**Database:** PostgreSQL (nohu-postgres)  
**Job Queue:** Redis (nohu-redis)

#### API Examples
```bash
# Create workflow
curl -X POST http://localhost:3014/workflows \
  -H "X-API-Key: $NOHU_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "customer_onboarding",
    "steps": [
      {
        "id": "step1",
        "action": "create_ticket",
        "service": "zammad",
        "params": {"title": "New customer"}
      },
      {
        "id": "step2",
        "action": "send_email",
        "service": "email",
        "params": {"template": "welcome"}
      },
      {
        "id": "step3",
        "action": "sync_contact",
        "service": "nango:hubspot",
        "params": {"model": "Contact"}
      }
    ]
  }'

# Trigger workflow execution
curl -X POST http://localhost:3014/workflows/customer_onboarding/execute \
  -H "X-API-Key: $NOHU_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "customer_id": "cust_123",
    "customer_email": "user@example.com"
  }'

# Get workflow execution status
curl -X GET http://localhost:3014/executions/exec_123 \
  -H "X-API-Key: $NOHU_API_KEY"

# List all workflows
curl -X GET http://localhost:3014/workflows \
  -H "X-API-Key: $NOHU_API_KEY"
```

#### Credentials
- **API Key:** `$NOHU_API_KEY`
- **Secret Key:** `$NOHU_SECRET_KEY`
- **DB User:** `$NOHU_DB_USER`
- **DB Password:** `$NOHU_DB_PASSWORD`
- **Redis Password:** `$NOHU_REDIS_PASSWORD`

---

## Integration Examples

### Example 1: Automated Ticket → Nango Sync → Nohu Workflow

**Flow:**
1. Customer submits issue → Zammad creates ticket
2. Nohu workflow triggers
3. Nohu calls Nango to sync customer data from CRM
4. Nango fetches customer context from Salesforce/HubSpot
5. Zammad ticket is enriched with customer context
6. Workflow completes

**curl example:**
```bash
# 1. Create Zammad ticket
TICKET_ID=$(curl -s -X POST http://localhost:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "title": "Feature request: API rate limits",
    "customer": "user@company.com"
  }' | jq -r '.id')

# 2. Trigger Nohu workflow to enrich ticket
curl -X POST http://localhost:3014/workflows/enrich_ticket/execute \
  -H "X-API-Key: $NOHU_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{
    \"ticket_id\": $TICKET_ID,
    \"customer_email\": \"user@company.com\"
  }"

# 3. Within the workflow, Nango syncs from CRM
# (Nohu calls Nango API internally)
curl -X POST http://localhost:3013/sync \
  -H "X-API-Key: $NANGO_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "integration": "salesforce",
    "connection_id": "user@company.com",
    "model": "Account"
  }'
```

### Example 2: New User → Multi-Service Setup

**Flow:**
1. User signs up in Velion
2. Nohu workflow triggers
3. Workflow steps:
   - Create support ticket in Zammad
   - Sync contact to CRM via Nango
   - Send welcome email
   - Initialize onboarding

**Workflow definition:**
```json
{
  "name": "new_user_onboarding",
  "trigger": "user.created",
  "steps": [
    {
      "id": "create_ticket",
      "service": "zammad",
      "action": "create_ticket",
      "input": {
        "title": "New customer onboarded: ${user.name}",
        "customer": "${user.email}",
        "group_id": "sales"
      }
    },
    {
      "id": "sync_crm",
      "service": "nango",
      "action": "sync",
      "input": {
        "integration": "hubspot",
        "connection_id": "${user.org_id}",
        "model": "Contact",
        "data": {
          "firstname": "${user.first_name}",
          "lastname": "${user.last_name}",
          "email": "${user.email}",
          "company": "${user.company}"
        }
      }
    },
    {
      "id": "send_email",
      "service": "email",
      "action": "send",
      "input": {
        "to": "${user.email}",
        "template": "welcome_new_user",
        "variables": {
          "name": "${user.first_name}"
        }
      }
    }
  ]
}
```

---

## Troubleshooting

### Service won't start
```bash
# Check logs
docker-compose -f docker-compose.external-services.yml logs zammad
docker-compose -f docker-compose.external-services.yml logs nango
docker-compose -f docker-compose.external-services.yml logs nohu

# Verify databases are ready
docker-compose -f docker-compose.external-services.yml logs zammad-postgres
```

### Database migration errors
```bash
# Reset and reinit (⚠️ destroys data)
docker-compose -f docker-compose.external-services.yml down -v
docker-compose -f docker-compose.external-services.yml up -d
```

### API authentication fails
```bash
# Verify API keys are set
echo $ZAMMAD_API_TOKEN
echo $NANGO_API_KEY
echo $NOHU_API_KEY

# Check if services are responding
curl -I http://localhost:3012
curl -I http://localhost:3013
curl -I http://localhost:3014
```

### Inter-service communication issues
```bash
# Verify network connectivity
docker network ls
docker network inspect external-services-net

# Test from container
docker exec zammad curl -v http://nango-api:3013/health
docker exec nango curl -v http://nohu-api:3014/health
```

---

## Security Checklist

- [ ] Change all default passwords in `.env.external-services.local`
- [ ] Rotate API keys regularly
- [ ] Enable API rate limiting (check service docs)
- [ ] Use TLS in production (configure reverse proxy)
- [ ] Enable audit logging for sensitive operations
- [ ] Restrict network access to services (firewall rules)
- [ ] Monitor database backups
- [ ] Rotate encryption keys periodically

---

## Next Steps

1. **Generate secure credentials:**
   ```bash
   ./scripts/generate_external_services_credentials.sh
   ```

2. **Set up monitoring/logging:**
   ```bash
   # Add to docker-compose.yml or separate monitoring stack
   # Consider: Prometheus, ELK Stack, Datadog
   ```

3. **Integration with Velion:**
   - Update frontend environment variables with API endpoints
   - Create client SDKs or HTTP wrappers for services
   - Add webhook handlers for events

4. **Production deployment:**
   - Move to managed services (AWS RDS, Azure Database, etc.)
   - Enable SSL/TLS for all endpoints
   - Set up automated backups
   - Configure load balancing and failover

---

## References

- **Zammad:** https://docs.zammad.org/en/latest/api/
- **Nango:** https://docs.nango.dev/
- **Nohu:** https://docs.nohu.io/ (or check Nohu docs for actual URL)

---

## Port Summary

| Service | Port | Purpose |
|---------|------|---------|
| Zammad API | 3012 | Ticketing system |
| Nango API | 3013 | API connectors |
| Nohu API | 3014 | Workflow engine |

**All services are API-only with no web UIs.**
