# CoreSystem Control Plane

Complete identity and organization management infrastructure for the CoreSystem platform.

## Infrastructure Services

The Control Plane has **dedicated infrastructure** with unique ports to avoid conflicts:

| Service        | External Port | Internal Port | Description                |
|----------------|---------------|---------------|----------------------------|
| PostgreSQL     | 5433          | 5432          | Dedicated database         |
| Redis          | 6380          | 6379          | Session & cache store      |
| NATS           | 4223          | 4222          | Event streaming (client)   |
| NATS Monitor   | 8223          | 8222          | NATS HTTP monitoring       |

## Services

| Service        | Port (HTTP) | Port (gRPC) | Port (Metrics) | Description                        |
|----------------|-------------|-------------|----------------|-----------------------------------|
| Frontend       | 3000        | -           | -              | Next.js web application           |
| Auth Core      | 3011        | 50011       | -              | Authentication & authorization    |
| User Core      | 3012        | 50012       | -              | User management & profiles        |
| Org Core       | 8080        | 9090        | 9091           | Organization metadata & policy    |
| Billing Core   | 3014        | 50013       | -              | Billing control plane & usage     |

## Quick Start

### Prerequisites

1. **Docker** and **Docker Compose** installed
2. Minimum 8GB RAM, 20GB disk space

### 1. Start All Services

```bash
# Start the full stack (frontend + backend services)
docker-compose up --build

# Or run in detached mode
docker-compose up -d --build
```

This will start:
- ✅ Infrastructure services (PostgreSQL, Redis, NATS)
- ✅ Auth Core (:3011) - Authentication & sessions
- ✅ User Core (:3012) - User profiles
- ✅ Org Core (:8080) - Organization management
- ✅ Billing Core (:3014) - Plans, quotas, usage, entitlements, invoices
- ✅ Frontend (:3000) - Web application

Access the application at **http://localhost:3000**

### 2. Test All Services

```bash
./test-services.sh
```

This runs comprehensive tests:
- Health checks for all HTTP endpoints
- gRPC connectivity tests
- Database connectivity
- NATS messaging
- Redis cache

### 3. Monitor Services

```bash
# View all Control service logs
./scripts/run-control-plane.sh logs -f

# View specific service logs
./scripts/run-control-plane.sh logs -f auth-core
./scripts/run-control-plane.sh logs -f user-core
./scripts/run-control-plane.sh logs -f org-core
./scripts/run-control-plane.sh logs -f billing-core

# Check service status
./scripts/run-control-plane.sh ps

# Monitor NATS events
nats sub ">" --server=nats://localhost:4222 --token=nats
```

## Architecture

```
┌──────────────┐
│   Frontend   │  Next.js Application
│    :3000     │  (Onboarding + Admin UI)
└──────┬───────┘
       │ HTTP API
       │
┌──────┴──────────────────────────────────────────────────────┐
│           Control Plane Infrastructure                       │
│  ┌──────────┐  ┌──────┐  ┌──────┐                          │
│  │PostgreSQL│  │ Redis│  │ NATS │                          │
│  │  :5433   │  │ :6380│  │ :4223│  Dedicated Ports         │
│  └────┬─────┘  └───┬──┘  └───┬──┘                          │
└───────┼────────────┼─────────┼──────────────────────────────┘
        │            │         │
        └────────────┴─────────┴──────────────
                              │
                   controlplane-network
                              │
        ┌─────────────────────┼─────────────────────┐
        │                     │                     │
   ┌────┴─────┐        ┌──────┴──────┐      ┌──────┴──────┐
   │ Org Core │        │ User Core   │      │ Auth Core   │
   │  :8080   │        │   :3012     │      │    :3011    │
   │  :9090   │        │  :50012     │      │   :50011    │
   │  :9091   │        └──────┬──────┘      └──────┬──────┘
   └────┬─────┘               │                     │
        │                     │                     │
        └─────────NATS────────┴──────Events─────────┘
```

## Service Details

### Frontend
- **Language**: TypeScript (Next.js 15)
- **Purpose**: Web application with onboarding flow and admin dashboard
- **Features**: 
  - User onboarding (profile → organization → team → complete)
  - Admin dashboard (users, organizations, billing)
  - Authentication via auth-core
- **Environment**: Connects to auth-core, user-core, org-core

### Auth Core
- **Language**: TypeScript (NestJS)
- **Purpose**: Authentication, authorization, session management
- **Events**: `auth.*` (login, logout, session.created, etc.)
- **Database**: `auth_service` (via PostgreSQL)
- **Redis DB**: 1

### User Core
- **Language**: Go
- **Purpose**: User profiles, authentication integration
- **Events**: `user.*` (registered, updated, deleted)
- **Database**: `user_service`
- **Redis DB**: 2

### Org Core
- **Language**: Go
- **Purpose**: Organization lifecycle management, quotas, billing, compliance
- **Events**: `org.*` (created, updated, deleted, status.changed, quota.*)
- **Database**: `org_core`
- **Redis DB**: 0
- **Metrics**: Prometheus metrics on port 9091

### Billing Core
- **Language**: Go
- **Purpose**: Financial control plane orchestration and provider abstraction
- **Events**: `billing.*` (account.updated, usage.recorded, invoice.created)
- **Database**: Shared PostgreSQL (`billing_*` tables)
- **Integrations**: Stripe adapter (payments), Lago adapter (usage/invoice)

## NATS Event System

All services communicate asynchronously via NATS (port 4223):

```bash
# Monitor all events
nats sub ">" --server=nats://localhost:4223

# Monitor org events
nats sub "org.*" --server=nats://localhost:4223

# Monitor admin events
nats sub "admin.*" --server=nats://localhost:4222 --token=nats

# Test event publishing
nats pub test.event "Hello from NATS" --server=nats://localhost:4222 --token=nats
```

## Database Schema

Each service has its own database in the shared PostgreSQL instance:

```sql
-- List all databases
docker exec -it controlplane-postgres psql -U coresystem -c "\l"

-- Connect to a specific database
docker exec -it controlplane-postgres psql -U coresystem -d controlplane

-- Check pgvector extension
docker exec -it controlplane-postgres psql -U coresystem -d controlplane -c "\dx"
```

## Environment Variables

Each service has its own `.env.local` file. Common variables:

```env
# Database (each service uses its own database)
POSTGRES_DSN=postgres://coresystem:postgres@postgres:5432/{service_db}?sslmode=disable

# Redis (each service uses its own DB number)
REDIS_URL=redis://:redis@redis:6379/{db_number}

# NATS (shared)
NATS_URL=nats://nats:4222
NATS_TOKEN=nats

# Security (development only)
JWT_SECRET=dev-jwt-secret-change-in-production
```

## Development Workflow

### Start Individual Service

```bash
# Start just Org Core (service-local env runner)
./scripts/run-control-plane.sh up -d org-core

# Start just Billing Core
./scripts/run-control-plane.sh up -d billing-core

# View logs
./scripts/run-control-plane.sh logs -f org-core
```

### Rebuild After Code Changes

```bash
# Rebuild specific service
./scripts/run-control-plane.sh build org-core
./scripts/run-control-plane.sh up -d org-core

./scripts/run-control-plane.sh build billing-core
./scripts/run-control-plane.sh up -d billing-core

# Rebuild all services
./scripts/run-control-plane.sh build
./scripts/run-control-plane.sh up -d
```

### Run Locally (without Docker)

```bash
# Set environment variables for localhost connections
export POSTGRES_DSN="postgres://coresystem:postgres@localhost:5432/coresystem_dev?sslmode=disable"
export REDIS_URL="redis://:redis@localhost:6379/0"
export NATS_URL="nats://localhost:4222"
export NATS_TOKEN="nats"

# Build and run
cd Org-core
make build
./bin/org-core
```

## Troubleshooting

### Services won't start

```bash
# Check Aquatiq Root Container
cd Org-core
./check-coresystem.sh

# Check Docker
docker ps
./scripts/run-control-plane.sh ps

# View logs
./scripts/run-control-plane.sh logs
```

### Database connection errors

```bash
# Test PostgreSQL
docker exec -it controlplane-postgres psql -U coresystem -c "SELECT 1"

# List databases
docker exec -it controlplane-postgres psql -U coresystem -c "\l"

# Create missing database
docker exec -it controlplane-postgres psql -U coresystem -c "CREATE DATABASE {db_name};"
```

### NATS not working

```bash
# Check NATS is running
docker ps --filter "name=controlplane-nats"

# Test NATS
nats sub test --server=nats://localhost:4223
```

### Stop the local stack safely

```bash
# Stop containers without deleting volumes or tenant data
./scripts/run-control-plane.sh down --remove-orphans

# Restart using the service-local runner
./scripts/run-control-plane.sh up -d
```

## API Documentation

### Org Core
- Health: `GET http://localhost:8080/health`
- Organizations: `GET http://localhost:8080/api/v1/organizations`
- gRPC: `grpcurl -plaintext localhost:9090 list`

### User Service
- Health: `GET http://localhost:3012/health`
- Users: `GET http://localhost:3012/api/v1/users`
- gRPC: `grpcurl -plaintext localhost:50012 list`

### Billing Core
- Health: `GET http://localhost:3014/health`
- gRPC: `grpcurl -plaintext localhost:50013 grpc.health.v1.Health/Check`

### Auth Service
- Health: `GET http://localhost:3000/health`
- Login: `POST http://localhost:3000/api/auth/login`

### AI Core
- Health: `GET http://localhost:8000/health`
- gRPC: `grpcurl -plaintext localhost:50014 list`

## Management Tools

Access shared infrastructure tools:

- **pgAdmin**: http://localhost:5050 (admin@coresystem.com / admin)
- **RedisInsight**: http://localhost:5540
- **NATS Monitor**: http://localhost:8222
- **MinIO Console**: http://localhost:9011 (admin / coresystem-minio-2024)

## Production Deployment

For production:

1. Use environment-specific `.env` files
2. Enable SSL/TLS for all connections
3. Use strong random credentials
4. Set up proper monitoring and alerting
5. Configure backup and disaster recovery
6. Use Docker secrets for sensitive data

See individual service documentation for details.

## Contributing

1. Make changes to service code
2. Rebuild: `./scripts/run-control-plane.sh build {service}`
3. Test: `./test-services.sh`
4. Commit and push

## Support

- [Aquatiq Root Container Docs](https://github.com/Aquatiq/coresystem-root-container)
- [Org Core Integration](./Org-core/AQUATIQ_INTEGRATION.md)
- [Admin Service Integration](./admin/AQUATIQ_INTEGRATION.md)
