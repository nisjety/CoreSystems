# Environment Configuration Structure

## Secrets: `DB_PASSWORD` has a single source of truth

`DB_PASSWORD` lives **only** in the root Control Plane `.env` (the live value).
`docker-compose.yml` injects it into every service as `DATABASE_PASSWORD=${DB_PASSWORD}`.

- Per-service `.env.docker` files **must not** define `DATABASE_PASSWORD` — a baked-in
  literal silently drifts from the live value and breaks DB auth when a container is
  recreated.
- Service config defaults for `DATABASE_PASSWORD` are **empty** (e.g. session-core
  `config.go`, user-core) so a missing value fails fast instead of connecting with a
  stale fallback. Never copy the live hex into source or per-service env files.

## Overview

Each service now has **4 environment files** for different deployment contexts:

### File Structure

```
auth-core/
├── .env                  # Development (localhost) - USE THIS FOR LOCAL DEV
├── .env.docker          # Docker Compose local dev - REFERENCED BY docker-compose.yml
├── .env.example         # Template showing all variables
└── .env.production      # Production deployment (environment variable placeholders)

user-core/
├── .env                  # Development (localhost) - USE THIS FOR LOCAL DEV
├── .env.docker          # Docker Compose local dev - REFERENCED BY docker-compose.yml
├── .env.example         # Template showing all variables
└── .env.production      # Production deployment (environment variable placeholders)

org-core/
├── .env                  # Development (localhost) - USE THIS FOR LOCAL DEV
├── .env.docker          # Docker Compose local dev - REFERENCED BY docker-compose.yml
├── .env.example         # Template showing all variables
└── .env.production      # Production deployment (environment variable placeholders)
```

## File Purposes

### `.env` - Development (Localhost)
- **Purpose**: Local development without Docker
- **Hostnames**: `localhost:5432`, `localhost:6379`, `localhost:4222`
- **Use Case**: Running services directly on your machine
- **Generated From**: `.env.example`
- **Status**: Not committed to git (in .gitignore)

### `.env.docker` - Docker Compose Local
- **Purpose**: Docker Compose local development environment
- **Hostnames**: `aquatiq-postgres-local`, `aquatiq-redis-local`, `aquatiq-nats-local`, `auth-core`, `user-core`
- **Use Case**: Running via `docker compose up -d`
- **Referenced By**: `docker-compose.yml` (this is what compose uses)
- **Generated From**: `.env.example` + Docker service names
- **Status**: Safe to commit (uses placeholder passwords)

### `.env.example` - Template
- **Purpose**: Template showing all required variables
- **Values**: All set to `CHANGE-ME` or `CHANGE-ME-PROD-*`
- **Use Case**: Documentation and as base for creating new `.env` files
- **Status**: Committed to git for reference
- **Command**: `cp .env.example .env` (then customize)

### `.env.production` - Production Deployment
- **Purpose**: Production deployment with environment variable placeholders
- **Hostnames**: `your-postgres-host`, `your-redis-host`, `your-nats-host`
- **Values**: All `CHANGE-ME-PROD-*` as placeholders for environment variables
- **Use Case**: Kubernetes/cloud deployments with secret management
- **Status**: Not sensitive (uses placeholders); safe to commit as template
- **Usage**: Reference for setting actual production secrets

---

## How to Use

### For Docker Compose (Recommended for Control Plane Testing)

```bash
cd '/Volumes/Lagring/Triodelab/CoreSystem/apps/Control Plane'

# Start the full control plane stack
docker compose up -d

# The compose file automatically uses .env.docker for all services
# because docker-compose.yml specifies:
#   env_file:
#     - ./auth-core/.env.docker
#     - ./user-core/.env.docker
#     - ./org-core/.env.docker
```

### For Local Development (Without Docker)

```bash
# Create .env from .env.example in each service
cd auth-core
cp .env.example .env
# Edit .env with your local database/redis/nats credentials

# Start service directly
npm start  # or `go run main.go` depending on service
```

### For Production Deployment

1. **Use .env.production as Reference**
   ```bash
   cp .env.production .env.production.local
   ```

2. **Set Actual Secrets via Environment Variables**
   ```bash
   export AUTH_DB_URL="postgres://user:actual-password@prod-host:5432/db"
   export AUTH_REDIS_URL="redis://:actual-password@prod-host:6379/3"
   export AUTH_NATS_URL="nats://prod-nats-host:4222"
   export AUTH_NATS_TOKEN="actual-nats-token"
   ```

3. **Or Use Kubernetes Secrets**
   ```yaml
   apiVersion: v1
   kind: Secret
   metadata:
     name: auth-core-secrets
   type: Opaque
   stringData:
     DATABASE_URL: "postgres://..."
     REDIS_URL: "redis://..."
     NATS_URL: "nats://..."
   ---
   # Then reference in deployment:
   envFrom:
   - configMapRef:
       name: auth-core-config
   - secretRef:
       name: auth-core-secrets
   ```

---

## Service Port Mappings

### auth-core
- HTTP API: `3011`
- gRPC API: `50011`
- Database: `auth_service` (Redis DB 3)

### user-core
- HTTP API: `3012`
- gRPC API: `50012`
- Database: `user_service` (Redis DB 2)

### org-core
- HTTP API: `8080`
- gRPC API: `9090`
- Metrics: `9091`
- Database: `org_core`

---

## Infrastructure Services (Shared)

Used by all control-plane services when running Docker Compose:

- **PostgreSQL**: `aquatiq-postgres-local:5432`
  - User: `aquatiq`
  - Password: `<redacted-rotate-and-set-locally>` (in `.env.docker`)

- **Redis**: `aquatiq-redis-local:6379`
  - Password: `change-me-redis-password` (in `.env.docker`)
  - Services use different DB numbers (2, 3, etc.)

- **NATS**: `aquatiq-nats-local:4222`
  - Token: `nats` (in `.env.docker`)

---

## Key Variables by Service

### Universal (All Services)
```
DATABASE_URL        # PostgreSQL connection
REDIS_URL           # Redis connection
NATS_URL            # NATS streaming broker
ENVIRONMENT         # development/production
LOG_LEVEL           # debug/info/warn/error
LOG_FORMAT          # json or text
```

### auth-core Only
```
BETTER_AUTH_SECRET  # Session encryption key
RESEND_API_KEY      # Email service (Resend)
TWILIO_*            # SMS service credentials
SSO_ENABLED         # Single sign-on support
OIDC_PROVIDER_ENABLED  # OpenID Connect server
```

### user-core Only
```
JWT_SECRET          # Token signing key
SESSION_EXPIRE_SECONDS  # Session timeout
BETTER_AUTH_URL     # Reference to auth-core
```

### org-core Only
```
ENABLE_POLICY_MIDDLEWARE  # Authorization enforcement
ENABLE_QUOTA_ENFORCEMENT  # Rate limiting
AUTH_SERVICE_URL    # Reference to auth-core
USER_SERVICE_URL    # Reference to user-core
```

---

## Migration Guide

### From Old Structure (Root Env) to New Structure (Service Scoped)

**Old (What You Had):**
```
docker-compose.yml uses:
  environment:
    DATABASE_URL: ...
    REDIS_URL: ...
```

**New (What You Have Now):**
```
docker-compose.yml references:
  env_file:
    - ./auth-core/.env.docker
    - ./user-core/.env.docker
    - ./org-core/.env.docker
```

**Benefits:**
✅ Services are fully self-contained
✅ Can deploy services independently
✅ Clear separation of concerns
✅ Different configs for different environments (.docker vs .production)
✅ Better maintainability

---

## Troubleshooting

### Service Can't Connect to Database

1. Check correct `.env.docker` is being used (verify `docker-compose.yml`)
2. Verify database hostname matches:
   - Docker: `aquatiq-postgres-local:5432`
   - Local: `localhost:5432`

### "Password mismatch" Errors

1. Ensure `DB_PASSWORD` in root `.env` matches services' `DATABASE_URL`
2. For Docker, infra password should match what's in service `.env.docker`

### Services Can't Communicate

1. Verify service DNS names in service URLs:
   - Docker: `http://auth-core:3011` ✓
   - Local: `http://localhost:3011` ✓

### Deploying to Production

1. Use `.env.production` as template/reference
2. Never commit actual secrets
3. Use environment variable substitution or secret management tool
4. Verify placeholder values are replaced before deployment

---

## Quick Reference Commands

```bash
# View what env file docker-compose uses
grep -A1 "env_file:" docker-compose.yml

# Start with Docker Compose (uses .env.docker files)
docker compose up -d

# Check what variables are actually loaded
docker compose config | grep -A 200 "auth-core:"

# View service logs to verify env loading
docker logs auth-service | head -20

# List all env files
find . -maxdepth 2 -name ".env*"

# Check which .env file a service is using
cat docker-compose.yml | grep -B3 -A1 "env_file"
```
