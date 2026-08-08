# Environment Configuration Structure

> **Verified 2026-07-16**: service-local environment ownership, scoped NATS
> credentials, and the
> auth-core/user-core port numbers below were re-checked against `docker-compose.yml`
> and live containers and are accurate. The "Infrastructure Services" hostnames
> (`coresystem-postgres-local` / `coresystem-redis-local` / `coresystem-nats-local`) were stale —
> those names only survive in historical comments and unused files, not in
> `docker-compose.yml`. The service list and port table include all six active
> cores. The Control Plane root `.env` and `.env.example` are intentionally absent.

## Secrets: service-local credentials plus transient Compose interpolation

Each core owns its ignored `.env` and tracked `.env.example`. Database URLs in
the service-local files are the development source of truth; the local runner
derives the shared Postgres interpolation value from those URLs and writes it
only to a temporary `0600` file. Production resets all development `env_file`
entries and requires external secret-manager/file-backed inputs.

- The supported local entry point is `./scripts/run-control-plane.sh`; direct
  Compose invocation without its generated interpolation inputs is expected to
  fail closed.
- Never copy production credentials into `.env`, `.env.docker`, or source.

## Overview

Each active service has an independent `.env` and `.env.example`; Docker-only
hostname overrides exist where needed:

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

`audit-core`, `billing-core`, and `session-core` follow the same `.env` plus
`.env.example` contract. Billing and Session also have Docker hostname
overrides; Audit's runtime interpolation is defined in Compose.

All six cores now have their own ignored `.env` and tracked `.env.example`.
Local development values are disposable; production values come from the
external secret manager.

## File Purposes

### `.env` - Development (Localhost)
- **Purpose**: Local development without Docker
- **Hostnames**: `localhost:5432`, `localhost:6379`, `localhost:4222`
- **Use Case**: Running services directly on your machine
- **Generated From**: `.env.example`
- **Status**: Not committed to git (in .gitignore)

### `.env.docker` - Docker Compose Local Override
- **Purpose**: Docker Compose local development environment
- **Hostnames**: `controlplane-postgres`, `controlplane-dragonfly`, `controlplane-nats`, and core service names
- **Use Case**: Docker hostname overrides layered after the core `.env`
- **Referenced By**: `docker-compose.yml` (this is what compose uses)
- **Generated From**: `.env.example` + Docker service names
- **Status**: Safe to commit (uses placeholder passwords)

### `.env.example` - Service Template
- **Purpose**: Template showing all required variables
- **Values**: All set to `CHANGE-ME` or `CHANGE-ME-PROD-*`
- **Use Case**: Documentation and as base for creating new `.env` files
- **Status**: Committed to git for reference
- **Command**: create the file inside the service directory; never create a
  Control Plane root `.env`.

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

# Start the full Control Plane stack using all six service-local env files
./scripts/run-control-plane.sh up -d

# Rebuild current images and start
./scripts/run-control-plane.sh up -d --build
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
- Database: `auth_service` (Dragonfly DB 3, addressed via `DRAGONFLY_URL`)

### user-core
- HTTP API: `3012`
- gRPC API: `50012`
- Database: `user_service` (Dragonfly DB 2, addressed via `DRAGONFLY_HOST`/`DRAGONFLY_DB`)

### org-core
- HTTP API (container): `8080` — published on host as `18080` in `docker-compose.yml`
  (host `8080` is reserved for Model Plane's model-gateway)
- gRPC API (container): `9090` — published on host as `19090`
- Metrics (container): `9091` — published on host as `19091`
- Also publishes a 4th port `6061:6061` (see `docker-compose.yml` for current use)
- Database: `org_core`

### billing-core
- HTTP API: `3014` (metrics on `6062`; gRPC `50013`) — has its own `.env`,
  `.env.docker`, and `.env.example`.

### session-core
- HTTP API: `3017` (gRPC `50017`) — has its own `.env`, `.env.docker`, and
  `.env.example`.

### audit-core
- HTTP API: `8187`. Audit owns `.env` and `.env.example`; its Docker-specific
  interpolation (`DATABASE_URL`, NATS buses, and scoped credentials) is set in
  the Compose `environment:` block.

---

## Infrastructure Services (Shared)

Used by all control-plane services when running Docker Compose. These are the
actual Docker network hostnames from `docker-compose.yml` (the older
`coresystem-*-local` names below only survive in `.env.example` comments/unused files
and are not what compose or any live `.env.docker` actually points at):

- **PostgreSQL**: `controlplane-postgres:5432` (container `controlplane-postgres`)
  - User: `coresystem`
  - Password: derived by the local runner from the service-local database URL;
    production uses an external secret-manager value.

- **Dragonfly** (Redis-protocol-compatible; replaced first-party Redis fleet-wide):
  `controlplane-dragonfly:6379` (container `controlplane-dragonfly`)
  - Password: set via `DRAGONFLY_URL`/service-specific vars in each `.env.docker`
    (not the literal `change-me-redis-password`).
  - Services use different DB numbers (2, 3, etc.)

- **NATS**: `controlplane-nats:4222` (container `controlplane-nats`)
  - Each core receives its own scoped NATS user/password; there is no shared
    development token authority.

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
each core owns:
  ./<core>/.env
  ./<core>/.env.example
local Compose is invoked through:
  ./scripts/run-control-plane.sh
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

1. Run `./scripts/run-control-plane.sh config --quiet` to validate the six
   service-local files and Compose interpolation.
2. Verify database hostname matches:
   - Docker: `controlplane-postgres:5432`
   - Local: `localhost:5432`

### "Password mismatch" Errors

1. Ensure the service-local `DATABASE_URL` credentials agree across the cores.
2. Re-run `./scripts/run-control-plane.sh up -d` so the temporary interpolation
   file and container environment are regenerated together.

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

# Start with the service-local environment runner
./scripts/run-control-plane.sh up -d

# Check that the interpolated configuration renders (without printing secrets)
./scripts/run-control-plane.sh config --quiet

# View service logs to verify env loading
docker logs auth-service | head -20

# List all env files
find . -maxdepth 2 -name ".env*"

# Check which .env file a service is using
cat docker-compose.yml | grep -B3 -A1 "env_file"
```
