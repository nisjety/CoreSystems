# External Services Setup - Complete Summary

Generated: April 15, 2026

## ✅ What Was Created

### 1. Docker Compose Configuration
**File:** `docker-compose.external-services.yml`
- **Zammad** (port 3012) - Ticketing system
- **Nango** (port 3013) - API connector platform  
- **Nohu** (port 3014) - Workflow orchestration engine
- All with dedicated PostgreSQL databases
- Redis caches for Zammad & Nohu
- No web UIs (API-only mode, like PostgreSQL)
- Health checks on all services

### 2. Credentials & Secrets
**File:** `.env.external-services` (template)
**File:** `.env.external-services.local` (generated, keep secure!)

Contains:
```
ZAMMAD_DB_PASSWORD
ZAMMAD_API_TOKEN
NANGO_DB_PASSWORD
NANGO_API_KEY
NANGO_SECRET_KEY
NOHU_DB_PASSWORD
NOHU_API_KEY
NOHU_SECRET_KEY
... + Redis passwords
```

### 3. Helper Scripts
- `scripts/generate_external_services_credentials.sh` - Create secure random credentials
- `scripts/start_external_services.sh` - One-command startup with health check
- `scripts/check_external_services_health.sh` - Verify all services are running

### 4. Documentation
- `EXTERNAL_SERVICES_QUICK_REFERENCE.md` - Quick start guide
- `docs/EXTERNAL_SERVICES_SETUP.md` - Complete setup & integration guide
- `docs/EXTERNAL_SERVICES_ARCHITECTURE.md` - Architecture & data flow diagrams

## 🚀 Quick Start

### Step 1: Generate Credentials
```bash
cd /Volumes/Lagring/Triodelab/CoreSystem
./scripts/generate_external_services_credentials.sh
mv .env.external-services.generated .env.external-services.local
```

### Step 2: Add to .gitignore
```bash
echo ".env.external-services.local" >> .gitignore
```

### Step 3: Start Services
```bash
./scripts/start_external_services.sh
```

The script will:
- Verify credentials exist
- Load credentials from `.env.external-services.local`
- Start all three services + databases + Redis
- Wait 60 seconds for initialization
- Run health checks

### Step 4: Verify Health
```bash
./scripts/check_external_services_health.sh
```

Output will show:
- ✓ Service status (running/failed)
- ✓ API endpoint health (3012, 3013, 3014)
- ✓ Database connections
- ✓ Redis connections
- ✓ Docker network status

## 🔑 Accessing Services

### Load Credentials in Terminal
```bash
set -a
source .env.external-services.local
set +a

echo "Zammad token: $ZAMMAD_API_TOKEN"
echo "Nango key: $NANGO_API_KEY"
echo "Nohu key: $NOHU_API_KEY"
```

### Test Zammad (Ticketing)
```bash
curl -s http://localhost:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN" | jq .
```

### Test Nango (Connectors)
```bash
curl -s http://localhost:3013/health \
  -H "X-API-Key: $NANGO_API_KEY" | jq .
```

### Test Nohu (Workflows)
```bash
curl -s http://localhost:3014/health \
  -H "X-API-Key: $NOHU_API_KEY" | jq .
```

## 📊 Service Summary

| Service | Purpose | Port | Database | Cache | Status |
|---------|---------|------|----------|-------|--------|
| Zammad | Ticketing | 3012 | PostgreSQL | Redis | API-only |
| Nango | Connectors | 3013 | PostgreSQL | - | API-only |
| Nohu | Workflows | 3014 | PostgreSQL | Redis | API-only |

## 🔗 Inter-Service Communication

All services on same Docker network (`external-services-net`):

```bash
# From Nohu workflow calling Nango (internal Docker network)
curl http://nango-api:3013/integrations \
  -H "X-API-Key: $NANGO_API_KEY"

# From Frontend to Zammad (external - through port 3012)
curl http://localhost:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN"
```

## 📁 File Structure

```
/Volumes/Lagring/Triodelab/CoreSystem/
├── docker-compose.external-services.yml      ← Service definitions
├── .env.external-services                     ← Template (committed)
├── .env.external-services.local              ← Generated (in .gitignore)
├── EXTERNAL_SERVICES_QUICK_REFERENCE.md      ← Quick start
├── scripts/
│   ├── start_external_services.sh
│   ├── generate_external_services_credentials.sh
│   └── check_external_services_health.sh
└── docs/
    ├── EXTERNAL_SERVICES_SETUP.md             ← Detailed guide
    └── EXTERNAL_SERVICES_ARCHITECTURE.md      ← Architecture & flows
```

## ⚙️ Configuration

### Environment Variables
All credentials in `.env.external-services.local`:

```bash
# For Zammad
ZAMMAD_DB_USER=zammad
ZAMMAD_DB_PASSWORD=<random_hex_32>
ZAMMAD_API_TOKEN=<random_hex_64>

# For Nango
NANGO_DB_USER=nango
NANGO_API_KEY=<random_hex_32>
NANGO_SECRET_KEY=<random_hex_64>

# For Nohu
NOHU_DB_USER=nohu
NOHU_API_KEY=<random_hex_32>
NOHU_REDIS_PASSWORD=<random_hex_32>
```

### Docker Network
All services communicate via: `external-services-net`
This is created automatically in docker-compose file.

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

# Follow Zammad database logs
docker-compose -f docker-compose.external-services.yml logs -f zammad-postgres
```

## 🔒 Security Checklist

- [x] Generated secure random credentials (run: `./scripts/generate_external_services_credentials.sh`)
- [x] Credentials stored in `.env.external-services.local`
- [ ] Added `.env.external-services.local` to `.gitignore` (IMPORTANT!)
- [ ] Review all passwords in `.env.external-services.local`
- [ ] Change default passwords from template
- [ ] Never commit `.env.external-services.local`
- [ ] Rotate credentials every 90 days
- [ ] Enable TLS in production
- [ ] Set up audit logging
- [ ] Monitor database backups

## 🔄 Typical Workflow

```bash
# 1. Generate credentials (one-time)
./scripts/generate_external_services_credentials.sh
mv .env.external-services.generated .env.external-services.local

# 2. Add to gitignore (one-time)
echo ".env.external-services.local" >> .gitignore

# 3. Start services (development)
./scripts/start_external_services.sh

# 4. Load credentials in new terminal
set -a && source .env.external-services.local && set +a

# 5. Use API keys to interact with services
curl -X GET http://localhost:3012/api/v1/tickets \
  -H "Authorization: Bearer $ZAMMAD_API_TOKEN"

# 6. Stop when done
docker-compose -f docker-compose.external-services.yml down
```

## 🚨 Common Issues

### Services won't start
```bash
docker-compose -f docker-compose.external-services.yml logs
# Check for credential errors or port conflicts
```

### API returns 401 Unauthorized
```bash
# Verify API key is loaded
echo $ZAMMAD_API_TOKEN
# Verify it matches the one in the service logs
```

### Database connection refused
```bash
# Check if database container is running
docker ps | grep postgres

# Check database logs
docker-compose -f docker-compose.external-services.yml logs zammad-postgres
```

### Port already in use
```bash
# Change ports in docker-compose.external-services.yml
# Or kill existing processes
lsof -i :3012  # Zammad
lsof -i :3013  # Nango
lsof -i :3014  # Nohu
```

## 📚 Documentation Files

1. **EXTERNAL_SERVICES_QUICK_REFERENCE.md** - Quick start & commands
2. **docs/EXTERNAL_SERVICES_SETUP.md** - Detailed integration guide with API examples
3. **docs/EXTERNAL_SERVICES_ARCHITECTURE.md** - Architecture diagrams & data flows

## ✨ Next Steps

1. **Start the services:** `./scripts/start_external_services.sh`
2. **Test connectivity:** `./scripts/check_external_services_health.sh`
3. **Read the detailed guide:** [EXTERNAL_SERVICES_SETUP.md](docs/EXTERNAL_SERVICES_SETUP.md)
4. **Integrate with Frontend:** Update Velion to call these APIs
5. **Set up monitoring:** Add logging/monitoring for production

---

**Setup Date:** April 15, 2026  
**Services:** Zammad (3012), Nango (3013), Nohu (3014)  
**Network:** external-services-net  
**All services:** API-only, no web UIs, ready to use immediately
