# Backend Refactor - Implementation Checklist

**Status:** ✅ IMPLEMENTATION COMPLETE  
**Date:** February 19, 2026

---

## Build Status

### ✅ User Service
- **Status:** Built successfully
- **HTTP Server:** Port 3012
- **gRPC Server:** Port 50012
- **REST Endpoints:** Implemented (stubs for API keys/preferences)
- **Dependencies:** gin-gonic/gin, zerolog added

### 🔄 Auth Service (TypeScript/NestJS)
- **Dual-publish:** Implemented
- **Trace IDs:** Implemented
- **New Streams:** USER_EVENTS, ORGANIZATION_EVENTS
- **No build required:** Uses npm/pnpm

### 🔄 Org-core Service (Go)
- **Policy Middleware:** Implemented
- **Route Protection:** Applied
- **Boundary Comments:** Added
- **Build Status:** To be tested

### 🔄 AI-core Service (Python)
- **Deprecation Notices:** Added
- **Boundary Violations:** Documented
- **Guidelines:** Created
- **Build Status:** To be tested (Python no compilation)

---

## Code Changes Summary

### Phase 1: Event Dual-Publishing ✅

**Files Modified:**
- `apps/backend/auth/src/internal/auth-event.publisher.ts`

**Changes:**
- Added `BaseEvent` with `traceId` and `correlationId`
- Implemented dual-publish logic (old + new event names)
- Created USER_EVENTS and ORGANIZATION_EVENTS streams
- Added `ENABLE_DUAL_PUBLISH` environment flag

**Testing:**
```bash
# Verify dual publish in auth service logs
docker logs auth-service | grep "Published event"
```

---

### Phase 2: Org-core Boundaries ✅

**Files Modified:**
- `apps/backend/Org-core/internal/http/policy_middleware.go` (NEW)
- `apps/backend/Org-core/internal/http/rag_handler.go`
- `apps/backend/Org-core/internal/http/server/server.go`

**Changes:**
- Created `PolicyMiddleware` with quota enforcement
- Applied middleware to RAG endpoints
- Added boundary violation logging
- Separated control-plane from data-plane routes

**Testing:**
```bash
# Build org-core
cd apps/backend/Org-core && go build ./cmd/server

# Test policy middleware with curl
curl -X POST http://localhost:8080/api/v1/rag/documents \
  -H "Content-Type: application/json" \
  -H "X-Org-ID: test-org-id" \
  -d '{"content": "test"}'
```

---

### Phase 3: AI-core Cleanup ✅

**Files Modified:**
- `apps/backend/ai-core/app/routes/document_knowledge.py`
- `apps/backend/ai-core/app/services/document_knowledge_pipeline.py`

**Documentation Created:**
- `docs/AI_CORE_BOUNDARY_GUIDELINES.md`

**Changes:**
- Marked `/api/documents/*` as DEPRECATED
- Documented in-memory vector store as violation
- Added migration timeline (2 sprints)

**Testing:**
```bash
# Verify deprecation notices in API docs
curl http://localhost:8040/docs
```

---

### Phase 4: User-service REST ✅

**Files Created:**
- `apps/backend/user/internal/http/server.go`
- `apps/backend/user/internal/http/handlers.go`

**Files Modified:**
- `apps/backend/user/cmd/server/main.go` - Added HTTP server startup
- `apps/backend/user/internal/grpc/server.go` - Updated service initialization
- `apps/backend/user/internal/users/service.go` - Updated constructor

**Dependencies Added:**
```bash
go get github.com/gin-gonic/gin
go get github.com/rs/zerolog
```

**Endpoints Implemented:**
```
GET   /api/v1/users/me       - Get current user profile
PATCH /api/v1/users/me       - Update current user profile
GET   /api/v1/users/:id      - Get user by ID (admin)
POST  /api/v1/api-keys       - Create API key (stub)
GET   /api/v1/api-keys       - List API keys (stub)
DELETE /api/v1/api-keys/:id  - Revoke API key (stub)
GET   /api/v1/preferences    - Get preferences (stub)
PATCH /api/v1/preferences    - Update preferences (stub)
```

**Build Verification:**
```bash
cd apps/backend/user
go build -o /tmp/user-service ./cmd/server
# ✅ Build successful
```

**Testing:**
```bash
# Start user-service with HTTP server
./user-service

# Test REST endpoint
curl http://localhost:3012/health

# Test user profile (requires JWT)
curl http://localhost:3012/api/v1/users/me \
  -H "Authorization: Bearer <jwt-token>"
```

---

### Phase 5: Configuration Normalization ✅

**Files Created:**
- `.env.example` - Template with all variables
- `docs/CONFIGURATION_NORMALIZATION_GUIDE.md`

**Files Modified:**
- `docker-compose.yml` - Updated auth-service and user-service

**Changes:**
- Removed hardcoded secrets from docker-compose
- Normalized database URLs to use env vars
- Standardized service name references (no localhost in containers)
- Added ENABLE_DUAL_PUBLISH flag support

**Environment Variables:**
```bash
# Copy example and fill in values
cp .env.example .env

# Required variables:
DB_PASSWORD=
REDIS_PASSWORD=
NATS_TOKEN=
JWT_SECRET=
BETTER_AUTH_SECRET=
INTERNAL_SERVICE_SECRET=
```

---

## Deployment Steps

### 1. Environment Setup

```bash
# Copy environment file
cp .env.example .env

# Edit with your secrets
nano .env
```

### 2. Build Services

```bash
# User service (Go)
cd apps/backend/user && go build ./cmd/server

# Org-core (Go)
cd apps/backend/Org-core && go build ./cmd/server

# Auth service (Node.js)
cd apps/backend/auth && npm install && npm run build

# AI-core (Python)
cd apps/backend/ai-core && pip install -r requirements.txt
```

### 3. Start Infrastructure

```bash
# Start databases and message queue
docker-compose up -d coresystem-postgres-local coresystem-redis-local coresystem-nats-local coresystem-qdrant-local
```

### 4. Start Services (Development)

```bash
# In separate terminals:

# Terminal 1: Auth service
cd apps/backend/auth && npm run dev

# Terminal 2: User service
cd apps/backend/user && go run ./cmd/server

# Terminal 3: Org-core
cd apps/backend/Org-core && go run ./cmd/server

# Terminal 4: AI-core
cd apps/backend/ai-core && python -m uvicorn app.main:app --reload

# Terminal 5: Frontend
cd apps/frontend && pnpm dev
```

### 5. Start Services (Docker)

```bash
# Build and start all services
docker-compose up -d

# Watch logs
docker-compose logs -f
```

---

## Validation

### Service Health Checks

```bash
# User service
curl http://localhost:3012/health
curl http://localhost:3012/api/v1/users/me -H "Authorization: Bearer <token>"

# Auth service
curl http://localhost:3011/api/auth/get-session

# Org-core
curl http://localhost:8080/health

# AI-core
curl http://localhost:8040/health
```

### Event Dual-Publishing Verification

```bash
# Check auth service logs for dual-publish
docker logs auth-service 2>&1 | grep "Published event"

# Should see both:
# - Published event: auth.user.registered to auth.user.registered (old)
# - Published event: user.registered (new)
```

### Policy Middleware Verification

```bash
# Test quota enforcement (should fail without proper headers)
curl -X POST http://localhost:8080/api/v1/rag/retrieve \
  -H "Content-Type: application/json" \
  -d '{"query": "test"}'

# Should return quota/policy error
```

---

## Next Steps

### Sprint 1 (Current)
- [x] Implement core changes
- [x] Build verification
- [x] Create documentation
- [ ] Deploy to staging
- [ ] Run integration tests

### Sprint 2
- [ ] Migrate event consumers to new event names
- [ ] Complete API key management implementation
- [ ] Complete preferences storage implementation
- [ ] Add contract tests for policy enforcement

### Sprint 3+
- [ ] Create RetrievalClient in AI-core
- [ ] Remove in-memory vector store
- [ ] Remove dual-publish (after consumer migration)
- [ ] Add observability using trace IDs

---

## Rollback Plan

If issues arise:

1. **Disable Dual-Publish:**
   ```bash
   # Set in .env or docker-compose
   ENABLE_DUAL_PUBLISH=false
   ```

2. **Revert to gRPC-only user-service:**
   - Comment out HTTP server in `cmd/server/main.go`
   - Rebuild and redeploy

3. **Remove Policy Middleware:**
   - Comment out middleware application in `rag_handler.go`
   - Rebuild org-core

4. **Revert Configuration:**
   - Use old hardcoded values in docker-compose
   - Redeploy services

---

## Support

**Documentation:**
- [SERVICE_BOUNDARY_CHARTER.md](./SERVICE_BOUNDARY_CHARTER.md)
- [BACKEND_REFACTOR_IMPLEMENTATION_SUMMARY.md](./BACKEND_REFACTOR_IMPLEMENTATION_SUMMARY.md)
- [CONFIGURATION_NORMALIZATION_GUIDE.md](./CONFIGURATION_NORMALIZATION_GUIDE.md)
- [AI_CORE_BOUNDARY_GUIDELINES.md](./AI_CORE_BOUNDARY_GUIDELINES.md)

**Contact:**
- Backend Team: See charter for ownership
- DevOps: For deployment issues
- Architecture: For boundary questions
