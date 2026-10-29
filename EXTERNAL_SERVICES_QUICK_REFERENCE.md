# External Services - Quick Reference

Three background services (API-only, no web UIs) deployed as Docker containers.

## 📋 Services

| Service | Purpose | Port | API Docs |
|---------|---------|------|----------|
| **Zammad** | Customer support ticketing | 3012 | [Zammad API](https://docs.zammad.org/en/latest/api/) |
| **Nango** | API connector platform (500+ integrations) | 3013 | [Nango Docs](https://docs.nango.dev/) |
| **Nohu** | Workflow orchestration engine | 3014 | [Nohu Docs](https://docs.nohu.io/) |

Each service has:
- Dedicated PostgreSQL database
- Redis cache (Zammad & Nohu)
- API-only mode (no web UI)
- Health checks enabled

## 🚀 Quick Start

### 1️⃣ Generate Credentials
```bash
cd /Volumes/Lagring/Triodelab/CoreSystem
./scripts/generate_external_services_credentials.sh
mv .env.external-services.generated .env.external-services.local
```

### 2️⃣ Start Services
```bash
./scripts/start_external_services.sh
```

### 3️⃣ Check Health
```bash
./scripts/check_external_services_health.sh
```

## 📁 Files Created

```
CoreSystem/
├── docker-compose.external-services.yml   # Service definitions
├── .env.external-services                 # Template with defaults
├── .env.external-services.local          # ← GENERATED (keep secure!)
├── scripts/
│   ├── start_external_services.sh         # One-command startup
│   ├── generate_external_services_credentials.sh
│   └── check_external_services_health.sh
└── docs/
    └── EXTERNAL_SERVICES_SETUP.md         # Detailed guide
```

## 🔑 Credentials Stored In

**`.env.external-services.local`** (KEEP SECURE - add to `.gitignore`)

```bash
# After generation, contains:
ZAMMAD_API_TOKEN=...
NANGO_API_KEY=...
NOHU_API_KEY=...
# Plus database passwords and Redis passwords
```

## 🧪 Test Connectivity

```bash
# Load credentials
set -a && source .env.external-services.local && set +a

# Test each service
curl -s http://localhost:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN"

curl -s http://localhost:3013/health \
  -H "X-API-Key: $NANGO_API_KEY"

curl -s http://localhost:3014/health \
  -H "X-API-Key: $NOHU_API_KEY"
```

## 🔄 Service Communication

All services are on the same Docker network (`external-services-net`), so they can communicate internally:

```bash
# Within Nohu workflow, call Nango:
curl http://nango-api:3013/sync \
  -H "X-API-Key: $NANGO_API_KEY"

# Within workflow, create Zammad ticket:
curl http://zammad-api:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN"
```

## 🛑 Stop Services

```bash
docker-compose -f docker-compose.external-services.yml down
```

## 📊 View Logs

```bash
# All services
docker-compose -f docker-compose.external-services.yml logs -f

# Specific service
docker-compose -f docker-compose.external-services.yml logs -f zammad
docker-compose -f docker-compose.external-services.yml logs -f nango
docker-compose -f docker-compose.external-services.yml logs -f nohu
```

## 🔒 Security

- [ ] Generate credentials with: `./scripts/generate_external_services_credentials.sh`
- [ ] Store in `.env.external-services.local` (NOT committed)
- [ ] Add to `.gitignore`: `echo ".env.external-services.local" >> .gitignore`
- [ ] Rotate API keys every 90 days
- [ ] Use strong database passwords (currently: `*_secure_password` - CHANGE!)

## 🔗 Integration Examples

### Example: Ticket → CRM Sync → Email (Using All 3)

```bash
# 1. Create ticket in Zammad
TICKET=$(curl -s -X POST http://localhost:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"title":"New customer","customer":"user@company.com"}')

TICKET_ID=$(echo $TICKET | jq -r '.id')

# 2. Trigger Nohu workflow that:
#    - Enriches ticket with Nango CRM sync
#    - Sends notification email
#    - Updates Zammad with CRM data
curl -X POST http://localhost:3014/workflows/enrich_ticket/execute \
  -H "X-API-Key: $NOHU_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"ticket_id\":$TICKET_ID,\"customer_email\":\"user@company.com\"}"

# 3. Nohu internally calls Nango to sync CRM
#    (connection made via external-services-net)
```

## 📞 Support

- **Zammad API Issues:** Check `docker-compose logs zammad`
- **Nango Connection Errors:** Verify OAuth provider credentials
- **Nohu Workflow Failures:** Check Redis connection and workflow syntax

See [EXTERNAL_SERVICES_SETUP.md](docs/EXTERNAL_SERVICES_SETUP.md) for detailed guide.
