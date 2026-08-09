# Configuration Normalization Guide (Phase 5)

**Version:** 1.0  
**Status:** ACTIVE - Normalization in Progress  
**Last Updated:** February 19, 2026

## Purpose

Standardize service communication URLs, environment variables, and configuration patterns across all backend services and docker-compose to ensure consistent behavior across local/dev/staging/prod environments.

---

## Normalization Rules

### 1. Service Names (Docker Compose)

**Standard:** Use consistent naming pattern

| Service | Container Name | Internal DNS Name |
|---------|---------------|-------------------|
| Frontend | `frontend` | `frontend` | 
| Auth | `auth-service` | `auth-service` |
| User | `user-service` | `user-service` |
| Org-core | `org-core-service` | `org-core-service` |
| AI-core | `ai-core-service` | `ai-core-service` |
| Temporal (Org) | `org-core-temporal` | `org-core-temporal` |
| PostgreSQL | `coresystem-postgres-local` | `coresystem-postgres-local` |
| Redis | `coresystem-redis-local` | `coresystem-redis-local` |
| NATS | `coresystem-nats-local` | `coresystem-nats-local` |
| Qdrant | `coresystem-qdrant-local` | `coresystem-qdrant-local` |

---

### 2. Internal Service URLs (Container-to-Container)

**Rule:** Always use service names, never `localhost`

**Correct:**
```env
# Auth service calling user service
USER_SERVICE_GRPC_URL=user-service:50012

# Org-core calling AI-core
AI_CORE_GRPC_URL=ai-core-service:50014

# Any service calling auth
AUTH_SERVICE_URL=http://auth-service:3011
```

**Incorrect:**
```env
# WRONG: Using localhost inside containers
USER_SERVICE_GRPC_URL=localhost:50012
```

---

### 3. External/Public URLs (Browser-Facing)

**Rule:** Use `localhost` for local dev, proper domain for prod

**Frontend Environment:**
```env
# Build-time (for server-side rendering)
BETTER_AUTH_URL=http://localhost:3000

# Runtime (browser uses window.location.origin)
NEXT_PUBLIC_APP_URL=http://localhost:3000  # local dev
NEXT_PUBLIC_APP_URL=https://app.example.com  # production
```

**Auth Service:**
```env
# For JWT issuer/audience
BETTER_AUTH_URL=http://localhost:3011  # local dev
BETTER_AUTH_URL=https://auth.example.com  # production
```

---

### 4. Port Standardization

| Service | HTTP Port | gRPC Port | Metrics Port |
|---------|-----------|-----------|--------------|
| Frontend | 3000 | - | - |
| Auth | 3011 | 50011 | - |
| User | 3012 | 50012 | - |
| Org-core | 8080 | 9090 | 9091 |
| AI-core | 8040 | 50014 | - |

---

### 5. Database URLs

**Pattern:** `postgres://<user>:<password>@<host>:<port>/<database>?sslmode=<mode>`

**Correct (Internal):**
```env
# Auth service DB
DATABASE_URL=postgres://coresystem:${DB_PASSWORD}@coresystem-postgres-local:5432/auth_service?sslmode=disable

# User service DB
DATABASE_URL=postgres://coresystem:${DB_PASSWORD}@coresystem-postgres-local:5432/user_service?sslmode=disable

# Org-core DB
DATABASE_URL=postgres://coresystem:${DB_PASSWORD}@coresystem-postgres-local:5432/org_core?sslmode=disable

# AI-core DB
DATABASE_URL=postgresql://postgres:${DB_PASSWORD}@coresystem-postgres-local:5432/ai_core
```

**Note:** Use environment variable for password, not hardcoded value

---

### 6. Dragonfly URLs

**Pattern:** `redis://:<password>@<host>:<port>/<db_num>`

**Correct:**
```env
# Dragonfly password should come from env var
REDIS_PASSWORD=redis  # or use ${REDIS_PASSWORD}

# Service-specific DB numbers
AUTH_REDIS_URL=redis://:${REDIS_PASSWORD}@coresystem-redis-local:6379/3
USER_REDIS_URL=redis://:${REDIS_PASSWORD}@coresystem-redis-local:6379/2
ORG_REDIS_URL=redis://:${REDIS_PASSWORD}@coresystem-redis-local:6379/1
AI_REDIS_URL=redis://:${REDIS_PASSWORD}@coresystem-redis-local:6379/4
```

---

### 7. NATS Configuration

**Pattern:** `nats://<host>:<port>` with optional token

**Correct:**
```env
# All services use same NATS cluster
NATS_URL=nats://coresystem-nats-local:4222
NATS_TOKEN=${NATS_TOKEN:-nats}  # Use env var with fallback
```

---

### 8. Secrets Management

**Rule:** Never hardcode production secrets in docker-compose

**Before (WRONG):**
```yaml
environment:
  - JWT_SECRET=dev-jwt-secret-change-in-production
  - BETTER_AUTH_SECRET=<REDACTED-rotate-and-set-via-.env>
```

**After (CORRECT):**
```yaml
environment:
  - JWT_SECRET=${JWT_SECRET:-dev-jwt-secret-local}
  - BETTER_AUTH_SECRET=${BETTER_AUTH_SECRET}
```

**Create `.env` file at root:**
```env
# .env (NOT committed to git)
JWT_SECRET=your-production-secret
BETTER_AUTH_SECRET=your-production-auth-secret
DB_PASSWORD=your-db-password
REDIS_PASSWORD=your-redis-password
NATS_TOKEN=your-nats-token
```

---

## Migration Checklist

### Phase 5 Immediate Actions

- [x] Document current configuration issues
- [ ] Create root `.env.example` with all required variables
- [ ] Update docker-compose.yml to use environment variables for secrets
- [ ] Normalize all internal service URLs to use service names
- [ ] Normalize all port assignments to standard pattern
- [ ] Update service .env files to reference correct service names
- [ ] Remove hardcoded fallback URLs (like "coresystem-qdrant-local" in code)

### Phase 5 Validation

- [ ] Test service-to-service communication after normalization
- [ ] Verify no localhost references in container logs
- [ ] Confirm secrets are properly loaded from environment
- [ ] Test docker-compose up with fresh environment

---

## Environment File Structure

### Root `.env` (Shared Secrets)
```env
# Database
DB_PASSWORD=secure-password-here
DB_USER=coresystem

# Dragonfly
REDIS_PASSWORD=secure-redis-password

# NATS
NATS_TOKEN=secure-nats-token

# JWT/Auth
JWT_SECRET=secure-jwt-secret
BETTER_AUTH_SECRET=secure-better-auth-secret
INTERNAL_SERVICE_SECRET=secure-internal-service-secret

# OpenAI/Azure (if shared)
OPENAI_API_KEY=your-openai-key
AZURE_OPENAI_API_KEY=your-azure-key
```

### Service-Specific `.env` Files
Keep service-specific configs in their respective `.env` files:
- `apps/backend/auth/.env` - OAuth credentials, auth-specific settings
- `apps/backend/ai-core/.env` - AI model endpoints, embeddings config
- `apps/frontend/.env` - Public/client-side variables only

---

## Success Criteria

Phase 5 is complete when:

1. ✅ All internal service URLs use service names (no localhost)
2. ✅ All secrets loaded from environment variables (no hardcoded)
3. ✅ Port assignments follow standard pattern
4. ✅ Database/Dragonfly URLs use consistent patterns
5. ✅ `.env.example` created with all required variables
6. ✅ docker-compose up works with fresh `.env` file
7. ✅ No config-related errors in service startup logs

---

## Common Pitfalls

### Pitfall #1: Mixing localhost and service names
```yaml
# WRONG: This breaks container-to-container communication
- AUTH_URL=http://localhost:3011
```

### Pitfall #2: Hardcoding passwords
```yaml
# WRONG: Passwords in version control
- DATABASE_URL=postgres://user:hardcoded-password@db:5432/mydb
```

### Pitfall #3: Inconsistent port mapping
```yaml
# WRONG: External port doesn't match internal port (confusing)
ports:
  - "8000:3011"  # External 8000 but service expects 3011
```

### Pitfall #4: Missing fallback for optional variables
```yaml
# WRONG: Service fails if optional var not set
- ENABLE_FEATURE=${ENABLE_FEATURE}

# CORRECT: Use fallback for optional vars
- ENABLE_FEATURE=${ENABLE_FEATURE:-false}
```

---

## Owner

**Team:** Backend Architecture  
**Reviewers:** DevOps, Backend Teams  
**Next Review:** After Phase 5 completion
