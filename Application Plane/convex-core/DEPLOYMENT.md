# Convex Gateway Deployment Guide

## Quick Start (Local Development)

```bash
cd backend/convex-gateway

# 1. Setup and start services
chmod +x setup.sh
./setup.sh

# 2. Start Convex dev server
npm run dev

# 3. Open dashboard
open http://localhost:6791
```

## Architecture Overview

Convex Gateway replaces the need for a custom Go WebSocket gateway by providing:

- **Reactive Database** - TypeScript queries with automatic subscriptions
- **Real-time Subscriptions** - Clients receive live updates automatically
- **Actions** - Server functions that call AI Core / Org Core
- **HTTP Actions** - Inbound webhooks from external services
- **File Storage** - Built-in file uploads
- **Auth Integration** - JWT verification

## Service URLs

| Service | URL | Purpose |
|---------|-----|---------|
| Backend API | http://localhost:3210 | Main Convex API |
| HTTP Actions | http://localhost:3211 | Inbound webhooks |
| Dashboard | http://localhost:6791 | Management UI |

## Data Flow Example: Chat Message

```
1. User types in Next.js frontend
   ↓
2. Frontend: convex.mutation("conversations:sendMessage", {...})
   ↓
3. Convex stores message in DB (transactional)
   ↓
4. Convex triggers action("ai:generateResponse")
   ↓
5. Action calls AI Core: POST /stream/chat
   ↓
6. AI Core calls Org Core gRPC: GetContext(orgID, query)
   ↓
7. Org Core retrieves from Postgres + Qdrant
   ↓
8. AI Core generates answer, streams chunks
   ↓
9. Convex receives chunks, stores them
   ↓
10. All subscribed clients receive updates (automatic)
    ↓
11. Frontend shows streaming response in real-time
```

## Configuration

### Environment Variables

Key variables in `.env.local`:

```bash
# Backend URLs
CONVEX_SELF_HOSTED_URL=http://localhost:3210
CONVEX_ADMIN_KEY=<generated-key>

# Service Integration
AI_CORE_URL=http://ai-core:8000
ORG_CORE_URL=http://org-core-service:8080
AUTH_SERVER_URL=http://auth-service:3001

# Auth
JWT_SECRET=your-jwt-secret
```

### Storage Options

#### Development: SQLite
```bash
CONVEX_STORAGE_TYPE=sqlite
CONVEX_SQLITE_PATH=/data/convex.db
```

#### Production: PostgreSQL
```bash
CONVEX_STORAGE_TYPE=postgres
DATABASE_URL=postgresql://user:pass@host:5432/convex
```

## Production Deployment

### Option 1: Fly.io (Recommended)

```bash
# Install flyctl
curl -L https://fly.io/install.sh | sh

# Login
fly auth login

# Create app
fly launch --name coresystem-convex

# Set secrets
fly secrets set \
  CONVEX_INSTANCE_SECRET=<generate-strong-secret> \
  JWT_SECRET=<your-jwt-secret> \
  AI_CORE_API_KEY=<api-key> \
  ORG_CORE_API_KEY=<api-key>

# Deploy
fly deploy

# Scale up
fly scale vm shared-cpu-2x --memory 2048
```

### Option 2: Railway

1. Go to https://railway.app
2. Click "New Project"
3. Select "Deploy from GitHub repo"
4. Connect your repo
5. Railway auto-detects Docker Compose
6. Add environment variables
7. Deploy

### Option 3: AWS ECS / GCP Cloud Run

See [deployment/cloud](./deployment/cloud/) for Terraform configs.

## Database Setup (Production)

### Neon PostgreSQL (Serverless)

```bash
# Create database
neonctl databases create convex-prod

# Get connection string
neonctl connection-string convex-prod

# Set in environment
export DATABASE_URL="postgresql://..."
```

### AWS RDS PostgreSQL

```bash
# Create via Terraform
cd deployment/terraform
terraform init
terraform apply

# Get connection string
terraform output database_url
```

## Monitoring

### Convex Dashboard

- Function logs in real-time
- Query performance metrics
- Database size and growth
- Active subscriptions

### External Monitoring

```bash
# Export metrics to Prometheus
docker compose --profile monitoring up -d

# View in Grafana
open http://localhost:3000
```

## Backup & Recovery

### SQLite (Development)

```bash
# Backup
docker compose exec convex-backend cp /data/convex.db /data/backup-$(date +%Y%m%d).db

# Restore
docker compose exec convex-backend cp /data/backup-YYYYMMDD.db /data/convex.db
docker compose restart convex-backend
```

### PostgreSQL (Production)

```bash
# Automated backups with Neon/RDS
# Point-in-time recovery available

# Manual backup
pg_dump $DATABASE_URL > backup.sql

# Restore
psql $DATABASE_URL < backup.sql
```

## Scaling

### Horizontal Scaling

Convex backend can run multiple instances:

```bash
# Scale to 3 instances
fly scale count 3

# Or with docker-compose
docker compose up --scale convex-backend=3
```

### Vertical Scaling

```bash
# Increase memory/CPU
fly scale vm dedicated-cpu-2x --memory 4096
```

## Troubleshooting

### Backend Won't Start

```bash
# Check logs
docker compose logs convex-backend

# Common issues:
# 1. Database connection failed
# 2. Port already in use
# 3. Invalid instance secret
```

### Functions Not Deploying

```bash
# Clear generated files
rm -rf convex/_generated

# Regenerate
npm run generate

# Redeploy
npm run deploy
```

### Slow Queries

```bash
# Check function performance in dashboard
open http://localhost:6791

# Add indexes to schema
# See convex/schema.ts for .index() usage
```

## Security Checklist

- [ ] Change `CONVEX_INSTANCE_SECRET` from default
- [ ] Generate unique `JWT_SECRET`
- [ ] Use HTTPS in production
- [ ] Set up API key rotation
- [ ] Enable webhook signature verification
- [ ] Configure CORS properly
- [ ] Use environment-specific credentials
- [ ] Enable audit logging

## Migration from WebSocket Hub

### Phase 1: Parallel Run
- Deploy Convex alongside existing WebSocket hub
- Implement new chat features in Convex
- Frontend uses Convex for new conversations

### Phase 2: Feature Parity
- Migrate job status to Convex
- Migrate session management
- Test both systems in parallel

### Phase 3: Full Cutover
- Switch all frontend traffic to Convex
- Monitor for 2 weeks
- Deprecate old WebSocket hub

## Cost Estimation

### Self-Hosted
- Compute: $50-100/month (Fly.io/Railway)
- Database: $20-50/month (Neon/RDS)
- Storage: $10-20/month
- **Total: $80-170/month**

### Cloud-Hosted (Alternative)
- Free tier: 500K calls/month
- Pro: $25/month (5M calls)
- **For most apps: $0-25/month**

## Support

- Discord: https://discord.gg/convex (#self-hosted channel)
- GitHub: https://github.com/get-convex/convex-backend/issues
- Docs: https://docs.convex.dev/

---

**Last Updated**: February 1, 2026  
**Status**: Production Ready ✅  
**Replaces**: Custom Go Gateway Service  
