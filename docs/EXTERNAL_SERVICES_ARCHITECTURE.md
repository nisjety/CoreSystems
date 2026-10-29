# Architecture & Integration Reference
## External Services in CoreSystem

```
┌─────────────────────────────────────────────────────────────────┐
│                        Frontend (Verevon)                         │
│                       :3000 (Next.js)                            │
└──────────────┬──────────────────────────────────────────────────┘
               │ HTTP requests
               ▼
┌─────────────────────────────────────────────────────────────────┐
│                   API Gateway / Proxy Layer                      │
│              (org-core, user-service, auth-service)             │
└───────┬────────────────────────┬───────────────────┬────────────┘
        │                        │                   │
        │ (gRPC/HTTP)            │ (HTTP)            │ (HTTP)
        │                        │                   │
        ▼                        ▼                   ▼
    ┌─────────┐            ┌──────────┐        ┌──────────────────┐
    │ Model   │            │ Data &   │        │ EXTERNAL SERVICES│
    │ Plane   │            │ Ingestion│        │ (New)             │
    │ (AI)    │            │ Plane    │        │                  │
    └─────────┘            └──────────┘        │ ┌──────────────┐ │
                                               │ │   Zammad     │ │
                                               │ │   (3012)     │ │
                                               │ │ Ticketing    │ │
                                               │ └──────────────┘ │
                                               │                  │
                                               │ ┌──────────────┐ │
                                               │ │   Nango      │ │
                                               │ │   (3013)     │ │
                                               │ │ Connectors   │ │
                                               │ └──────────────┘ │
                                               │                  │
                                               │ ┌──────────────┐ │
                                               │ │   Nohu       │ │
                                               │ │   (3014)     │ │
                                               │ │ Workflows    │ │
                                               │ └──────────────┘ │
                                               │                  │
                                               │ Network:         │
                                               │ external-        │
                                               │ services-net     │
                                               └──────────────────┘
```

## Data Flow Examples

### 1. Customer Support Workflow
```
User Creates Issue
    ↓
Frontend → Auth Service (3011) → Org Core
    ↓
Create Ticket
    ↓
Zammad API (3012)
    ├─ Store in PostgreSQL
    └─ Notify via Nohu
        ↓
    Nohu Workflow (3014)
        ├─ Trigger notification
        ├─ Call Nango to sync CRM
        │   ├─ Nango (3013)
        │   └─ External CRM (Salesforce/HubSpot)
        └─ Enrich Zammad ticket with CRM data
```

### 2. Multi-Service Integration
```
Event: New User Signup
    ↓
Frontend → User Service
    ↓
User Created Event
    ↓
Nohu Workflow Engine (3014) Triggered
    ├─ Step 1: Create support ticket
    │   └─ HTTP → Zammad API (3012)
    ├─ Step 2: Sync to CRM
    │   └─ HTTP → Nango API (3013)
    │       └─ OAuth → Salesforce/HubSpot
    ├─ Step 3: Send welcome email
    │   └─ Internal email service
    └─ Step 4: Initialize onboarding
        └─ HTTP → Document Service
```

### 3. OAuth Integration via Nango
```
External App (GitHub, Google, etc.)
    ↓
Frontend requests OAuth
    ↓
Frontend → Nango (3013)
    │
Nango handles:
├─ OAuth flow
├─ Token storage (encrypted)
├─ Token refresh
└─ Connection management
    ↓
Returns: Auth token to Frontend
    ↓
Frontend now authorized to access external API
```

## Service Dependencies

```
External Services Network: external-services-net

Zammad
├─ zammad-postgres (database)
├─ zammad-redis (cache)
└─ Port: 3012

Nango
├─ nango-postgres (database)
└─ Port: 3013

Nohu
├─ nohu-postgres (database)
├─ nohu-redis (job queue)
└─ Port: 3014
```

## API Integration Points

### From Frontend
```javascript
// Fetch tickets from Zammad
fetch('http://localhost:3012/api/v1/tickets', {
  headers: {
    'Authorization': `Bearer ${ZAMMAD_API_TOKEN}`
  }
})

// Setup OAuth with Nango
fetch('http://localhost:3013/oauth/authorize', {
  method: 'POST',
  headers: {
    'X-API-Key': NANGO_API_KEY,
    'Content-Type': 'application/json'
  },
  body: JSON.stringify({
    integration: 'salesforce',
    redirectTo: 'http://localhost:3000/auth/callback'
  })
})

// Trigger workflow in Nohu
fetch('http://localhost:3014/workflows/process_ticket/execute', {
  method: 'POST',
  headers: {
    'X-API-Key': NOHU_API_KEY,
    'Content-Type': 'application/json'
  },
  body: JSON.stringify({
    ticket_id: 123,
    customer_email: 'user@company.com'
  })
})
```

### Inter-Service Communication (Docker Network)
```bash
# From Nohu calling Nango (internal network)
curl http://nango-api:3013/sync \
  -H "X-API-Key: ${NANGO_API_KEY}"

# From Nohu creating Zammad ticket (internal network)
curl http://zammad-api:3012/api/v1/tickets \
  -H "Authorization: Bearer ${ZAMMAD_API_TOKEN}"
```

## Configuration

### Environment Variables
All credentials managed in `.env.external-services.local`:

```bash
# Zammad
ZAMMAD_DB_USER=zammad
ZAMMAD_DB_PASSWORD=***
ZAMMAD_REDIS_PASSWORD=***
ZAMMAD_API_TOKEN=***

# Nango
NANGO_DB_USER=nango
NANGO_DB_PASSWORD=***
NANGO_API_KEY=***
NANGO_SECRET_KEY=***
NANGO_ENCRYPTOR_KEY=***

# Nohu
NOHU_DB_USER=nohu
NOHU_DB_PASSWORD=***
NOHU_REDIS_PASSWORD=***
NOHU_API_KEY=***
NOHU_SECRET_KEY=***
```

## Deployment

### Local Development
```bash
# Start services
./scripts/start_external_services.sh

# Check health
./scripts/check_external_services_health.sh

# View logs
docker-compose -f docker-compose.external-services.yml logs -f
```

### Production Considerations
- [ ] Move databases to managed services (AWS RDS, Azure Database)
- [ ] Enable TLS/SSL for all APIs
- [ ] Use Kubernetes instead of Docker Compose
- [ ] Set up monitoring (Prometheus, Datadog)
- [ ] Enable audit logging
- [ ] Configure automated backups
- [ ] Use secret management (AWS Secrets Manager, Azure Key Vault)
- [ ] Set up API rate limiting
- [ ] Enable CORS policies

## Troubleshooting

### Services won't start
```bash
# Check logs
docker-compose -f docker-compose.external-services.yml logs

# Verify environment
cat .env.external-services.local | grep -v "^#"

# Check Docker network
docker network inspect external-services-net
```

### Database connection errors
```bash
# Verify database is running
docker exec zammad-postgres psql -U zammad -c "SELECT 1"

# Check database URL format
# postgres://user:password@host:port/dbname?sslmode=disable
```

### API authentication fails
```bash
# Verify API keys exist
echo $ZAMMAD_API_TOKEN
echo $NANGO_API_KEY
echo $NOHU_API_KEY

# Test endpoint
curl -I http://localhost:3012
```

## Next Steps

1. **Start services:** `./scripts/start_external_services.sh`
2. **Generate secure credentials:** Already done in `generate_external_services_credentials.sh`
3. **Integrate with Verevon Frontend:** Update environment variables
4. **Set up webhooks:** Configure event listeners
5. **Monitor and log:** Add observability stack

See [EXTERNAL_SERVICES_SETUP.md](docs/EXTERNAL_SERVICES_SETUP.md) for detailed integration guide.
