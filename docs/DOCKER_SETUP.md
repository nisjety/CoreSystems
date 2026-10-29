# Docker Compose Setup - Quick Reference

## What Was Fixed

### 1. **Corrected Context Paths**
All service build contexts now correctly point to the `backend/` directory:
- ✅ `backend/Org-core/` (was `./Org-core`)
- ✅ `backend/user/` (was `./user`)
- ✅ `backend/auth/` (was `./auth`)
- ✅ `backend/ai-core/` (was `./ai-core`)
- ✅ `backend/convex-gateway/` (was `./convex-gateway`)

### 2. **Added Frontend Service**
The Next.js frontend is now included in the Docker Compose setup:
- **Container:** `frontend`
- **Port:** `3000`
- **Build:** Uses the production Dockerfile with Next.js standalone output
- **Environment Variables:**
  - `NEXT_PUBLIC_APP_URL=http://localhost:3000`
  - `NEXT_PUBLIC_API_URL=http://localhost:8080`
  - `NEXT_PUBLIC_BACKEND_URL=http://localhost:8080`
  - `NEXT_PUBLIC_INTEGRATIONS_API=http://localhost:3210`
  - `BETTER_AUTH_URL=http://localhost:3011`

### 3. **Service Dependencies**
Frontend now properly depends on:
- `auth-service` (authentication)
- `org-core` (organization management)
- `convex-backend` (realtime data)

## How to Build and Run

### Option 1: Using the Build Script (Recommended)
```bash
chmod +x build-and-run.sh
./build-and-run.sh
```

### Option 2: Manual Docker Compose Commands
```bash
# Create network (if needed)
docker network create coresystem-local

# Build all services
docker-compose build --parallel

# Start services
docker-compose up -d

# View logs
docker-compose logs -f

# View specific service logs
docker-compose logs -f frontend
docker-compose logs -f auth-service
```

## Service Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      Frontend (Next.js)                      │
│                     http://localhost:3000                    │
└─────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
        ▼                     ▼                     ▼
┌──────────────┐    ┌──────────────┐    ┌──────────────┐
│ Auth Service │    │  Org Core    │    │   Convex     │
│   Port 3011  │    │  Port 8080   │    │  Port 3210   │
└──────────────┘    └──────────────┘    └──────────────┘
        │                     │                     │
        │                     ▼                     │
        │            ┌──────────────┐              │
        └───────────▶│ User Service │◀─────────────┘
                     │  Port 3012   │
                     └──────────────┘
                              │
                              ▼
                     ┌──────────────┐
                     │  AI Core     │
                     │  Port 8040   │
                     └──────────────┘
```

## All Service URLs

| Service | URL | Description |
|---------|-----|-------------|
| **Frontend** | http://localhost:3000 | Next.js web application |
| **Auth Service** | http://localhost:3011 | Authentication & authorization |
| **User Service** | http://localhost:3012 | User management |
| **Org Core** | http://localhost:8080 | Organization management |
| **AI Core** | http://localhost:8040 | AI agent core |
| **Convex Backend** | http://localhost:3210 | Realtime backend |
| **Convex Dashboard** | http://localhost:6791 | Convex UI |
| **Letta Server** | http://localhost:8283 | AI memory server |
| **Temporal UI** | http://localhost:8088 | Workflow UI |

## Health Checks

All services include health checks to ensure proper startup order:
- **Frontend:** 60s start period, checks port 3000
- **Auth:** Checks `/api/auth/get-session`
- **User:** Checks gRPC port 50012
- **Org Core:** Checks `/health`
- **AI Core:** Checks `/health`

## Useful Commands

```bash
# View all service status
docker-compose ps

# Stop all services
docker-compose down

# Stop and remove all data
docker-compose down -v

# Rebuild a specific service
docker-compose build frontend

# Restart a specific service
docker-compose restart frontend

# View resource usage
docker stats

# Follow logs from all services
docker-compose logs -f

# Follow logs from specific service
docker-compose logs -f frontend
```

## Troubleshooting

### Services won't start
```bash
# Check network exists
docker network ls | grep coresystem-local

# Create network manually
docker network create coresystem-local

# Check for port conflicts
lsof -i :3000  # Check if port 3000 is in use
```

### Build errors
```bash
# Clean build with no cache
docker-compose build --no-cache

# Remove old images
docker image prune -a
```

### Frontend not connecting to backend
1. Check environment variables in docker-compose.yml
2. Ensure all backend services are healthy
3. Check logs: `docker-compose logs -f frontend auth-service org-core`

### Database connection issues
Check that the database URLs in each service match the network configuration:
- Hostname should be the service name (e.g., `coresystem-postgres-local`)
- Port should be the internal port (e.g., `5432` not `5433`)

## Network Configuration

All services are connected via the `coresystem-local` external network:
- Services communicate using their container names as hostnames
- Internal ports are used for inter-service communication
- External ports (mapped) are for host access

## Next Steps

1. **Environment Variables:** Create `.env` files for each service if needed
2. **SSL/TLS:** Configure SSL certificates for production
3. **Monitoring:** Set up logging aggregation and monitoring
4. **Backup:** Configure database backup strategies
5. **Scaling:** Consider load balancing for production deployment

## Notes

- The frontend is built with Next.js standalone mode for optimal Docker performance
- All services use health checks to ensure proper startup sequence
- Volumes are configured for persistent data (Letta, Temporal, Convex)
- The network is external to allow sharing with other Docker Compose projects
