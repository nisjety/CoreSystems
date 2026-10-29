# Service Boundary Charter

**Version:** 1.0  
**Status:** ACTIVE  
**Last Updated:** February 19, 2026

This charter defines the official ownership boundaries, API contracts, event responsibilities, and hard constraints for all backend services in CoreSystem.

---

## Charter Purpose

- **Define** clear ownership and responsibility boundaries per service
- **Prevent** accidental cross-plane coupling and boundary violations
- **Enable** safe refactoring with explicit compatibility windows
- **Enforce** "never do" rules to maintain architectural integrity

---

## Service Ownership Matrix

### auth-service (NestJS)

**Primary Responsibility:** Identity and access control plane

**Owns:**
- User authentication (sessions, JWT, JWKS)
- OAuth provider integration
- Organization membership management
- Role and permission assignment
- Auth lifecycle events

**Public API Surface:**
- `POST /api/auth/*` (Better Auth routes)
- `POST /api/v2/auth/oauth/initiate`
- `GET /api/v2/auth/oauth/callback`
- `GET /.well-known/jwks.json`

**Published Events (Target):**
- `user.created`
- `user.updated`
- `user.deleted`
- `organization.created`
- `organization.updated`
- `organization.member_added`
- `organization.member_removed`
- `organization.role_changed`

**Published Events (Current - Compatibility Phase):**
- `auth.user.*` (dual-publish with target events)
- `auth.organization.*` (dual-publish with target events)

**Never Do:**
- ❌ Own user profile/preferences storage (user-service owns)
- ❌ Own org plans/quotas/entitlements (org-core owns)
- ❌ Make direct database calls to other service schemas
- ❌ Implement business logic beyond auth/access control

**Allowed Callers:**
- Frontend (all public routes)
- All internal services (via JWKS validation)

---

### user-service (Go)

**Primary Responsibility:** User profile and preference control plane

**Owns:**
- User profile data (name, avatar, bio, etc.)
- User preferences and settings
- API key generation and management
- User-level feature flags
- User activity tracking

**Public API Surface (Target):**
- `GET /api/users/me`
- `PATCH /api/users/me`
- `GET /api/users/:id`
- `POST /api/users/api-keys`
- `GET /api/users/api-keys`
- `DELETE /api/users/api-keys/:id`

**Public API Surface (Current - Compatibility Phase):**
- gRPC: `UserService` (existing internal contract)
- REST: New endpoints being added for frontend parity

**Consumes Events:**
- `user.created` → Initialize user profile
- `user.deleted` → Cleanup user data
- `organization.member_added` → Track user org memberships

**Published Events:**
- `user.profile_updated`
- `user.preferences_changed`
- `user.api_key_created`
- `user.api_key_revoked`

**Never Do:**
- ❌ Own authentication/session state (auth-service owns)
- ❌ Own organization metadata (org-core owns)
- ❌ Make policy/quota decisions (org-core owns)
- ❌ Access document/vector storage directly

**Allowed Callers:**
- Frontend (REST endpoints)
- Internal services (gRPC during migration, then REST)

---

### org-core (Go)

**Primary Responsibility:** Organization control plane and policy enforcement

**Owns:**
- Organization metadata and settings
- Plan and subscription management
- Entitlements and quota enforcement
- Usage tracking and billing events
- Guardrail and policy decisions
- Admin operations

**Public API Surface:**
- `GET /api/organizations`
- `POST /api/organizations`
- `GET /api/organizations/:id`
- `PATCH /api/organizations/:id`
- `GET /api/organizations/:id/members`
- `GET /api/organizations/:id/usage`
- `POST /api/organizations/:id/check-entitlement`

**Internal Transitional Surface (Being Isolated):**
- Retrieval/indexing routes (to be gated by policy layer)
- RAG initialization (to be isolated from main control plane)

**Consumes Events:**
- `organization.created` → Initialize org data
- `organization.updated` → Sync org metadata
- `organization.member_added` → Update access control
- `user.created` → Track user-org relationships

**Published Events:**
- `organization.quota_exceeded`
- `organization.plan_changed`
- `organization.entitlement_updated`
- `organization.usage_reported`

**Never Do:**
- ❌ Own user authentication (auth-service owns)
- ❌ Own user profiles (user-service owns)
- ❌ Expose retrieval endpoints without policy checks
- ❌ Allow direct Qdrant access from external callers
- ❌ Implement AI orchestration logic (ai-core owns)

**Allowed Callers:**
- Frontend (public org endpoints)
- ai-core (for policy/quota checks via internal API)
- Internal services (for entitlement validation)

---

### ai-core (Python/FastAPI)

**Primary Responsibility:** AI orchestration and reasoning plane

**Owns:**
- AI workflow orchestration
- Model inference coordination
- Reranking and synthesis
- Result formatting and validation
- AI metrics and evaluation

**Public API Surface:**
- `POST /api/ai/chat`
- `POST /api/ai/summarize`
- `POST /api/ai/analyze`
- `GET /api/ai/status/:job_id`

**Internal Surface (Being Cleaned):**
- Document pipeline coordination (NOT document storage)
- Embedding generation requests (NOT vector storage ownership)

**Consumes Events:**
- `organization.quota_exceeded` → Throttle AI requests
- `organization.entitlement_updated` → Update AI feature access

**Published Events:**
- `ai.job_started`
- `ai.job_completed`
- `ai.job_failed`
- `ai.token_usage_reported`

**Never Do:**
- ❌ Own document source-of-truth storage
- ❌ Own vector database directly (must go through policy layer)
- ❌ Make policy/quota decisions (must call org-core)
- ❌ Bypass entitlement checks for AI operations
- ❌ Store user/org data independently

**Allowed Callers:**
- Frontend (public AI endpoints)
- Temporal workflows (orchestrated tasks)

---

## Cross-Cutting Concerns

### Temporal

**Owns:** Workflow orchestration only

**Never Do:**
- ❌ Own business logic (workers implement, Temporal coordinates)
- ❌ Store business data (only workflow state)

---

### NATS JetStream

**Owns:** Event bus infrastructure

**Never Do:**
- ❌ Transform or validate business events (services own event schemas)

---

## Event Migration Strategy

### Current State (Compatibility Phase)
auth-service publishes:
- `auth.user.created` AND `user.created` (dual-publish)
- `auth.user.updated` AND `user.updated` (dual-publish)
- `auth.organization.created` AND `organization.created` (dual-publish)
- etc.

### Target State (After Migration Window)
Only simplified event names:
- `user.*`
- `organization.*`

### Migration Timeline
- **Dual-publish period:** 2 sprints minimum
- **Consumer migration:** All consumers must migrate to new event names
- **Deprecation:** Old event names removed after validation

---

## API Compatibility Matrix

| Service | Current API | Target API | Migration Status |
|---------|-------------|------------|------------------|
| auth-service | `/api/auth/*` | `/api/auth/*` | ✅ Stable |
| user-service | gRPC only | gRPC + REST `/api/users/*` | 🔄 Adding REST |
| org-core | Mixed control+data routes | Control plane only | 🔄 Isolating data plane |
| ai-core | Mixed orchestration+storage | Orchestration only | 🔄 Removing storage patterns |

---

## Policy Enforcement Rules

### Retrieval Firewall (Mode A - Current)

**Flow:** `AI-core → Org-core policy check → Retrieval → Qdrant`

**Rules:**
1. AI-core MUST call org-core entitlement check before any retrieval
2. Org-core MUST validate quota/permissions before allowing retrieval
3. Qdrant access ONLY through org-core internal retrieval layer
4. No direct Qdrant client initialization outside org-core

**Validation:**
- Code review: No new Qdrant clients in ai-core or other services
- Runtime audit: Log all retrieval requests with entitlement check results

---

## Configuration Standards

### Service Communication

**Internal (Docker Compose):**
- Use service names: `http://auth-service:3011`, `http://org-core:8082`

**External (Frontend/Public):**
- Use localhost for local dev: `http://localhost:3011`
- Use proper load balancer URLs for staging/prod

**Current Issues Being Fixed:**
- Mixed localhost/service name usage
- Hardcoded fallback values
- Inconsistent port configuration

---

## Code Review Checklist

Before merging any backend changes, verify:

- [ ] Change respects service ownership boundaries
- [ ] No new direct database access to other service schemas
- [ ] No new Qdrant clients outside approved paths
- [ ] Events use target naming (or dual-publish if in compatibility window)
- [ ] API changes documented in compatibility matrix
- [ ] Trace/correlation IDs propagated across service calls
- [ ] Security: service-to-service auth validated
- [ ] Tests verify boundary enforcement

---

## Escalation and Exceptions

If a change violates this charter:

1. **Document the exception** with business justification
2. **Get architect approval** before implementation
3. **Add to technical debt backlog** with remediation plan
4. **Set expiration date** for exceptional path

No permanent exceptions allowed.

---

## Charter Maintenance

- **Review cadence:** Quarterly or when new services added
- **Owner:** Backend architecture team
- **Change process:** PR + team review required

---

## Quick Reference

**Golden Rule:** If a service owns the data, it owns the policy. If it owns the policy, it owns the public API.

**Emergency Contact:** Check current ownership in this document before making cross-service changes.
