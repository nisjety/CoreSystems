# Ingestion Plane Quick Start

**Project:** Quarry + Imports-Core - Complete Ingestion Stack  
**Status:** Production Ready  
**Last Updated:** February 19, 2026

---

## Prerequisites

```bash
# System requirements
- Docker Desktop 4.25+
- Docker Compose 2.25+
- Make (optional, for convenience)
- curl (for testing endpoints)

# Recommended resources
- 8GB RAM (4GB minimum)
- 20GB disk space
- 2+ CPU cores
```

---

## Quick Start (Docker Compose)

### 1. Start All Services

```bash
# Navigate to Ingestion Plane directory
cd "Ingestion Plane"

# Start complete stack (Quarry + Imports + Infrastructure)
docker-compose -f docker-compose.full.yml up -d

# Watch startup logs
docker-compose -f docker-compose.full.yml logs -f

# Check service status
docker-compose -f docker-compose.full.yml ps
```

### 2. Verify Services

```bash
# Quarry API (web scraping)
curl http://localhost:8090/health
# Expected: {"status":"ok"}

# Imports API (file import)
curl http://localhost:3025/health
# Expected: {"status":"ok","service":"import-service"}

# Temporal UI (workflow dashboard)
open http://localhost:8089
# Dashboard shows running workflows
```

### 3. Test File Upload

```bash
# Create a test CSV file
echo -e "name,email\nJohn Doe,john@example.com\nJane Smith,jane@example.com" > test.csv

# Upload file
curl -X POST http://localhost:3025/api/v1/import/jobs/upload \
  -F "org_id=test-org-001" \
  -F "user_id=test-user-001" \
  -F "files=@test.csv"

# Expected response:
# {
#   "id": "uuid-...",
#   "org_id": "test-org-001",
#   "source_type": "upload",
#   "status": "queued",
#   "total_items": 2,
#   "processed_items": 0
# }
```

### 4. Monitor Import Job

```bash
# Get job ID from upload response
JOB_ID="uuid-..."

# Check status
curl http://localhost:3025/api/v1/import/jobs/$JOB_ID

# Stream events (SSE)
curl http://localhost:3025/api/v1/import/jobs/$JOB_ID/events
```

---

## Development Setup

### Local Python Development (Imports-Core)

```bash
# Navigate to imports-core
cd imports-core

# Create virtual environment
python -m venv venv
source venv/bin/activate  # Windows: venv\Scripts\activate

# Install dependencies
pip install -r requirements.txt

# Configure environment
cp .env.example .env
# Edit .env with local settings

# Run database migrations
psql $DATABASE_URL < migrations/001_init.sql

# Start service (with auto-reload)
uvicorn app.main:app --reload --port 3025
```

### Local Go Development (Quarry)

```bash
# Navigate to Quarry
cd Quarry

# Install dependencies
go mod download

# Build binaries
make build

# Start infrastructure only (without Quarry services)
docker-compose up -d postgres redis temporal nats qdrant
docker-compose exec postgres psql -U root < scripts/init-quarry-db.sql

# Run API server
./bin/quarry-api

# In another terminal, run worker
./bin/quarry-worker
```

---

## Common Tasks

### View Logs

```bash
# All services
docker-compose -f docker-compose.full.yml logs -f

# Specific service
docker-compose -f docker-compose.full.yml logs -f imports-api
docker-compose -f docker-compose.full.yml logs -f quarry-api

# Follow new logs only
docker-compose -f docker-compose.full.yml logs -f --tail 50
```

### Database Access

```bash
# Connect to PostgreSQL
docker-compose -f docker-compose.full.yml exec postgres psql -U root

# Inside psql shell:
\c quarry                    # Switch to quarry database
\dt                          # List tables
SELECT * FROM jobs LIMIT 5;  # Query jobs

\c imports                   # Switch to imports database
SELECT * FROM import_jobs;   # View import jobs
```

### Redis CLI

```bash
# Access Redis
docker-compose -f docker-compose.full.yml exec redis redis-cli

# Inside redis-cli:
KEYS *                       # List all keys
GET quarry:cache:abc         # Get specific key
FLUSHDB                      # Clear current database (careful!)
```

### NATS Monitoring

```bash
# View NATS stats
curl http://localhost:8223/varz | jq .

# View published messages
curl http://localhost:8223/subscription/detail | jq .
```

---

## Testing

### Test Web Scraping (Quarry)

```bash
# Simple scrape
curl -X POST http://localhost:8090/v1/scrape \
  -H "X-API-Key: dev-test-key-12345" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "formats": ["markdown"]
  }'

# Async crawl
curl -X POST http://localhost:8090/v1/crawl \
  -H "X-API-Key: dev-test-key-12345" \
  -H "Content-Type: application/json" \
  -d '{
    "url": "https://example.com",
    "maxDepth": 2,
    "limit": 10
  }'
```

### Test File Import (Imports-Core)

```bash
# Create test file
cat > data.json << 'EOF'
[
  {"id": 1, "name": "Item 1", "value": 100},
  {"id": 2, "name": "Item 2", "value": 200}
]
EOF

# Upload
curl -X POST http://localhost:3025/api/v1/import/jobs/upload \
  -F "org_id=test-org" \
  -F "files=@data.json"

# Test Notion integration
curl -X POST http://localhost:3025/api/v1/import/jobs/source \
  -H "Content-Type: application/json" \
  -d '{
    "org_id": "test-org",
    "source_type": "notion",
    "connection": {
      "token": "secret_abc123..."
    },
    "options": {
      "filter": {"property": "Type", "select": {"equals": "Product"}}
    }
  }'
```

---

## Troubleshooting

### Service Won't Start

```bash
# Check logs
docker-compose -f docker-compose.full.yml logs imports-api

# Common issues:
# - Port already in use: lsof -i :3025
# - Database not ready: docker-compose logs postgres
# - Network issues: docker network inspect ingestion-net
```

### Database Connection Error

```bash
# Verify database is running
docker-compose -f docker-compose.full.yml exec postgres pg_isready

# Check credentials in .env
cat imports-core/.env | grep DATABASE_URL

# Test connection manually
psql postgresql://imports:imports@localhost:5434/imports
```

### File Upload Fails

```bash
# Check file size
ls -lh test.csv

# Check allowed file types
curl http://localhost:3025/api/v1/import/jobs/upload \
  -F "org_id=test" \
  -F "files=@test.xlsx"  # Might fail if xlsx not in ALLOWED_FILE_TYPES

# Check logs
docker-compose -f docker-compose.full.yml logs imports-api | tail -20
```

### Memory Issues

```bash
# Check resource usage
docker stats

# Reduce allocations
docker-compose -f docker-compose.full.yml down
# Edit docker-compose.full.yml to reduce memory limits
docker-compose -f docker-compose.full.yml up -d

# Or increase Docker Desktop memory allocation
# Docker Desktop Settings > Resources > Memory
```

---

## Cleanup

### Stop All Services

```bash
docker-compose -f docker-compose.full.yml down
```

### Remove Everything (including data)

```bash
docker-compose -f docker-compose.full.yml down -v
```

### Clean Unused Resources

```bash
docker system prune -a --volumes
```

---

## Production Deployment

### Build Custom Images

```bash
# Build Quarry image
docker build -t quarry:1.0.0 ./Quarry

# Build Imports image
docker build -t imports-core:1.0.0 ./imports-core

# Tag for registry
docker tag quarry:1.0.0 registry.example.com/quarry:1.0.0
docker tag imports-core:1.0.0 registry.example.com/imports-core:1.0.0

# Push to registry
docker push registry.example.com/quarry:1.0.0
docker push registry.example.com/imports-core:1.0.0
```

### Deploy to Kubernetes

```bash
# Create namespace
kubectl create namespace ingestion-plane

# Create secrets
kubectl create secret generic ingestion-secrets \
  --from-literal=database-url='postgresql://user:pass@host/db' \
  --from-literal=nats-token='secret-token' \
  -n ingestion-plane

# Deploy services
kubectl apply -f kubernetes/quarry-deployment.yaml -n ingestion-plane
kubectl apply -f kubernetes/imports-deployment.yaml -n ingestion-plane
kubectl apply -f kubernetes/postgres-statefulset.yaml -n ingestion-plane
kubectl apply -f kubernetes/redis-statefulset.yaml -n ingestion-plane

# Check status
kubectl get deployments -n ingestion-plane
kubectl get pods -n ingestion-plane
```

---

## Performance Monitoring

### Real-time Metrics

```bash
# Quarry metrics
curl http://localhost:8090/metrics | grep -E "quarry_|go_"

# CPU and memory
docker stats quarry-api imports-api

# Network I/O
docker stats --no-stream
```

### Load Testing

```bash
# Using Apache Bench
ab -n 1000 -c 10 http://localhost:3025/health

# Using wrk (if installed)
wrk -t4 -c100 -d30s http://localhost:3025/health

# Results show:
# - Requests/sec (throughput)
# - Avg latency
# - 99th percentile latency
```

---

## Environment Configuration

### Development

```bash
IMPORT_SERVICE_PORT=3025
LOG_LEVEL=DEBUG
DATABASE_URL=postgresql+asyncpg://root:root@localhost:5434/imports
TEMPORAL_ENABLED=false
NATS_URL=nats://localhost:4223
```

### Production

```bash
IMPORT_SERVICE_PORT=3025
LOG_LEVEL=WARNING
DATABASE_URL=postgresql+asyncpg://user:secure-password@postgres-prod:5432/imports
TEMPORAL_ENABLED=true
TEMPORAL_HOST_PORT=temporal-prod:7233
NATS_URL=nats://nats-prod:4222
NATS_TOKEN=secure-token
```

---

## Documentation

- **[Quarry Documentation](Quarry/docs/API.md)** - Web scraping API reference
- **[Imports-Core README](imports-core/README.md)** - File import service guide
- **[Architecture Overview](INGESTION_PLANE_ARCHITECTURE.md)** - System design
- **[Deployment Guide](Quarry/docs/DEPLOYMENT.md)** - Production setup

---

## Support

- **Issues:** Create GitHub issue with logs
- **Slack:** #ingestion-plane-dev
- **On-Call:** Check PagerDuty rotation

---

**Last Updated:** February 19, 2026
