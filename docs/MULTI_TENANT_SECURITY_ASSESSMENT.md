# Multi-Tenant Security Assessment — CoreSystem

> **Phase B, Step 6** · Date: 2025-07-08
> **Scope:** org-core (L1 Control Plane) → Data Plane (L2) multi-tenant isolation
> **Classification:** Security-Critical · GDPR-Relevant

---

## 1. Architecture Validation

### 1.1 org-core → Data Plane Is the Correct Pattern

The relationship between **org-core** (L1 Control Plane, Go, `:8080`) and the **Data Plane** (L2) is **NOT** an architecture violation. It is the **intentional multi-tenant security pattern**:

```
┌─────────────────────────────────────────────────────────┐
│  L1 Control Plane                                       │
│  ┌──────────┐  Defines tenant   ┌──────────────┐       │
│  │ org-core  │  boundaries  ──► │ auth-core     │       │
│  │ (Go:8080) │                  │ (NestJS:3011) │       │
│  └──────────┘                   └──────────────┘        │
│       │ gRPC CheckOrgAccess            │ ValidateToken  │
│       │ (org_id → role, plan,          │ (JWT → user_id,│
│       │  permissions, entitlements)     │  org_id)       │
└───────┼────────────────────────────────┼────────────────┘
        ▼                                ▼
┌─────────────────────────────────────────────────────────┐
│  L2 Data Plane                                          │
│  ┌────────────────┐    ┌───────────────────┐            │
│  │ documents-svc   │    │ retrieval-svc     │            │
│  │ (Py:8001/50051) │    │ (Py:8004/50052)   │            │
│  └────────────────┘    └───────────────────┘            │
│                                                         │
│  Auth Chain per request:                                │
│  Bearer → auth_middleware → gRPC ValidateToken          │
│  → AuthContext{user_id, org_id} → authz.py              │
│  → gRPC CheckOrgAccess to org-core                      │
│  → Permission + entitlement gate → Handler              │
└─────────────────────────────────────────────────────────┘
```

**Authority flows downward** (L1 → L2), which is correct per the 6-layer pyramid model. org-core is the **single source of truth** for:
- Which organization a user belongs to
- What role a user holds within that organization
- What plan (and therefore what entitlements/quotas) apply
- Whether access should be granted or denied

### 1.2 Critical Security Property: Fail-Closed

If org-core is **unreachable**, Data Plane services **DENY ALL ACCESS**. This is enforced in `authz.py` and is the correct fail-closed security posture:

```python
# Data Plane authz.py — fail-closed on error
try:
    access = await grpc_check_org_access(user_id, org_id)
except Exception:
    raise PermissionDenied("Unable to verify organization access")
```

---

## 2. Current Security Posture

### 2.1 Summary Matrix

| Layer | Mechanism | Status | Notes |
|-------|-----------|--------|-------|
| org-core SQL | `WHERE org_id = $1` on every query | ✅ Secure | repository.go (~420 lines) |
| org-core Redis | Keys namespaced: `org:{id}`, `org:slug:{slug}`, `org:members:{org_id}` | ✅ Secure | Cache-aside pattern |
| Data Plane auth | `AuthContext.org_id` from validated JWT per request | ✅ Secure | auth_middleware.py |
| Data Plane authz | `check_org_access()` via gRPC to org-core | ✅ Secure | Fail-closed on error |
| Data Plane quotas | `org_quotas` table, plan-based limits | ✅ Secure | Per-org enforcement |
| GDPR deletion | `gdpr_hard_delete_organization` PL/pgSQL function | ⚠️ Partial | See Section 4 |
| NATS events | Dual-publish: controlplane-nats + velion-nats (JetStream) | ✅ Functional | 6 event types |
| Cache invalidation | NATS-driven on org lifecycle events | ⚠️ Review | See Section 3.3 |

### 2.2 org-core SQL Isolation (Verified)

**File:** `apps/Control Plane/org-core/internal/org/repository.go` (~420 lines)

Every data-access query includes explicit org_id scoping:

```sql
-- All reads scoped
SELECT ... FROM organizations WHERE id = $1
SELECT ... FROM org_members WHERE org_id = $1 AND user_id = $2
SELECT ... FROM org_quotas WHERE org_id = $1

-- All writes scoped
INSERT INTO org_members (org_id, user_id, role) VALUES ($1, $2, $3)
UPDATE organizations SET ... WHERE id = $1
DELETE FROM org_members WHERE org_id = $1 AND user_id = $2
```

**Assessment:** No unscoped queries found. Every database operation requires an explicit `org_id` parameter.

### 2.3 Redis Cache Isolation (Verified)

Cache keys follow a strict namespacing convention:

| Key Pattern | Purpose | TTL |
|------------|---------|-----|
| `org:{uuid}` | Organization metadata | Cache-aside |
| `org:slug:{slug}` | Slug → UUID lookup | Cache-aside |
| `org:members:{org_id}` | Membership list | Cache-aside |

**Assessment:** Keys are tenant-namespaced. No shared/global keys that could leak cross-tenant data.

### 2.4 Dual-NATS Event Topology (Verified)

**Internal (controlplane-nats:4222):**
- Subjects: `organization.>`, `user.>`, `session.>`, `billing.>`, `usage.>`, `auth.>`
- Consumer: org-core BridgeSubscriber listens on `auth.>`

**Cross-Plane (velion-nats, JetStream):**
- Stream: `AQENCIA_CONTROLPLANE`
- Subjects: `aqencia.controlplane.org.*`
- Limits: 100K messages, 14-day retention, 60s dedup window
- Data Plane consumers: queue groups `data-plane-quota` (plan changes), `data-plane-quota-alert` (quota exceeded)

**Side-channels:**
- `notifications.billing.plan_changed` → notification-core
- `notifications.org.member_removed` → notification-core

**Six event types dual-published on every mutation:**
1. `org.created`
2. `org.updated`
3. `org.deleted`
4. `org.plan_changed`
5. `org.member_added`
6. `org.member_removed`

**Graceful Degradation:** If velion-nats is unavailable at startup, Data Plane continues serving without live quota updates — security is maintained because the gRPC auth chain is independent of NATS.

### 2.5 Data Plane Auth Chain (Verified)

```
Request with Bearer token
  → auth_middleware.py: extract token
  → gRPC ValidateToken → auth-core (NestJS :3011)
  → Returns: AuthContext { user_id, org_id, email, ... }
  → Stored in request.state.auth
  → authz.py: check_org_access()
  → gRPC CheckOrgAccess → org-core (Go :8080)
  → Returns: OrgAccess { role, plan, permissions[], entitlements[] }
  → Route-level permission + entitlement check
  → Handler receives fully-validated, org-scoped context
```

**Proto definition** (`proto/org_access.proto`):
```protobuf
service OrgAccessService {
  rpc CheckOrgAccess(OrgAccessRequest) returns (OrgAccessResponse);
}
message OrgAccessRequest { string user_id = 1; string org_id = 2; }
message OrgAccessResponse {
  string role = 1;
  string plan = 2;
  repeated string permissions = 3;
  repeated string entitlements = 4;
}
```

---

## 3. Security Gaps & Hardening Recommendations

### 3.1 CRITICAL — No Tenant-Scoping Middleware in org-core HTTP

**File:** `apps/Control Plane/org-core/internal/http/handlers.go` (~530 lines)

**Finding:** Each HTTP handler individually extracts `orgID` from URL path parameters. There is **no centralized middleware** that enforces tenant scoping before handlers execute.

**Risk:** A new handler added by a developer could accidentally omit `orgID` extraction, creating an unscoped endpoint that leaks or modifies cross-tenant data.

**Current pattern (per-handler, error-prone):**
```go
func (h *Handler) GetOrganization(w http.ResponseWriter, r *http.Request) {
    orgID := chi.URLParam(r, "orgID")  // Manual extraction per handler
    // ... use orgID
}
```

**Recommendation — Add tenant-scoping middleware:**

```go
// middleware/tenant.go
func TenantScope(next http.Handler) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        orgID := chi.URLParam(r, "orgID")
        if orgID == "" {
            http.Error(w, "missing org_id", http.StatusBadRequest)
            return
        }
        if _, err := uuid.Parse(orgID); err != nil {
            http.Error(w, "invalid org_id format", http.StatusBadRequest)
            return
        }
        ctx := context.WithValue(r.Context(), tenantKey, orgID)
        next.ServeHTTP(w, r.WithContext(ctx))
    })
}
```

**Apply to all org-scoped routes:**
```go
r.Route("/orgs/{orgID}", func(r chi.Router) {
    r.Use(TenantScope)  // Centralized enforcement
    r.Get("/", h.GetOrganization)
    r.Put("/", h.UpdateOrganization)
    r.Delete("/", h.DeleteOrganization)
    r.Get("/members", h.ListMembers)
    // ... all org-scoped routes automatically protected
})
```

**Priority:** HIGH — Implement before adding any new org-scoped endpoints.

### 3.2 HIGH — Consider PostgreSQL Row-Level Security (RLS)

**Finding:** Tenant isolation currently relies entirely on application-level `WHERE org_id = $1` clauses. A single missed clause in a new query would be a cross-tenant data breach.

**Recommendation — Defense-in-depth with RLS:**

```sql
-- Enable RLS on tenant-scoped tables
ALTER TABLE organizations ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE org_quotas ENABLE ROW LEVEL SECURITY;

-- Policy: application role can only access rows matching session org_id
CREATE POLICY tenant_isolation ON organizations
    USING (id = current_setting('app.current_org_id')::uuid);

CREATE POLICY tenant_isolation ON org_members
    USING (org_id = current_setting('app.current_org_id')::uuid);

-- Set session variable before each transaction
SET LOCAL app.current_org_id = '<org_id>';
```

**Trade-offs:**
- ✅ Defense-in-depth: even if application code misses a WHERE clause, database denies cross-tenant access
- ✅ GDPR Article 25 compliance (data protection by design and default)
- ⚠️ Requires connection-per-request or SET LOCAL per transaction
- ⚠️ pgxpool connection pooling (MaxConns=5) needs adjustment for RLS session variables

**Priority:** MEDIUM — Significant protection uplift, but requires careful connection pool management.

### 3.3 MEDIUM — Cache Invalidation Race Condition

**Finding:** When an organization is deleted or a member is removed, cache invalidation relies on NATS event propagation. If NATS delivery is delayed:

1. org-core publishes `org.deleted` to NATS
2. Data Plane cache still holds stale org metadata
3. Brief window where deleted org's data remains accessible

**Recommendation:**
- Add explicit TTL to all Redis cache entries (recommend: 5 minutes max for org metadata)
- On `org.deleted` event, Data Plane consumer should explicitly `DEL` all related cache keys
- Add `Cache-Control: no-store` headers on sensitive endpoints
- Log cache invalidation events for audit trail

```go
// org-core: add TTL to all cache SET operations
err := h.redis.Set(ctx, "org:"+orgID, data, 5*time.Minute).Err()
```

**Priority:** MEDIUM — Small time window, but exploitable in a targeted attack.

### 3.4 MEDIUM — Token org_id Scope Verification

**Finding:** Data Plane extracts `org_id` from the JWT validated by auth-core. Need to verify:

1. Token issuance (auth-core) embeds the correct `org_id` based on the user's active organization
2. Token cannot be modified to contain a different `org_id` (JWT signature verification)
3. User cannot switch orgs without obtaining a new token
4. Multi-org users get separate tokens per org (or token refresh on org switch)

**Recommendation:**
- Verify auth-core JWT issuance includes `org_id` claim
- Confirm JWT signature verification uses RS256 or ES256 (not HS256 with shared secrets)
- Add `org_id` to token revocation checks
- On org switch, invalidate previous token and issue new one

**Priority:** MEDIUM — JWT signature prevents tampering, but token lifecycle needs verification.

### 3.5 LOW — Connection Pool Sizing

**Finding:** `pgxpool.MaxConns = 5` is suitable for development but insufficient for production multi-tenant load.

**Recommendation:**
- Development: 5 connections (current)
- Staging: 10-15 connections
- Production: 20-50 connections (based on expected concurrent org count)
- Add connection pool metrics to Prometheus (pool_size, active, idle, wait_count)
- Consider PgBouncer for connection multiplexing at scale

**Priority:** LOW — Operational concern, not a security vulnerability.

---

## 4. GDPR Compliance Assessment

### 4.1 Article 17 — Right to Erasure

**Existing Implementation:** `gdpr_hard_delete_organization` PL/pgSQL function performs cascading deletion of all tenant-linked data in PostgreSQL.

**Gap Analysis — Data Stores Requiring Verification:**

| Store | Covered by GDPR Function? | Action Required |
|-------|--------------------------|-----------------|
| PostgreSQL (org-core) | ✅ Yes | Verify cascade completeness |
| PostgreSQL (Data Plane) | ⚠️ Verify | Confirm documents, embeddings, usage data deleted |
| Redis (org-core) | ❌ Not in PL/pgSQL | Add Redis key cleanup to deletion workflow |
| Redis (Data Plane) | ❌ Not in PL/pgSQL | Add Redis key cleanup to deletion workflow |
| Qdrant (vector DB) | ❌ Not in PL/pgSQL | Add collection/point deletion for org |
| NATS JetStream | ⚠️ Verify | Messages have 14-day retention — verify org data purge |
| MinIO/S3 (file storage) | ❌ Not in PL/pgSQL | Add bucket/prefix cleanup for org |

**Recommendation — Comprehensive GDPR Deletion Workflow:**

```
1. Receive deletion request
2. Validate authorization (org admin or GDPR controller)
3. PostgreSQL: Execute gdpr_hard_delete_organization (cascade)
4. Redis org-core: DEL org:{id}, org:slug:{slug}, org:members:{org_id}
5. Redis Data Plane: SCAN and DEL keys matching org:{id}:*
6. Qdrant: Delete collection or filter points by org_id metadata
7. NATS JetStream: Purge messages with org_id in subject/payload
8. MinIO/S3: Delete all objects under org_id prefix
9. Publish org.gdpr_deleted event (audit trail)
10. Log completion with timestamp for compliance record
```

### 4.2 Article 25 — Data Protection by Design and Default

| Principle | Current Status | Recommendation |
|-----------|---------------|----------------|
| Data minimization | ✅ org-core stores only essential org metadata | — |
| Purpose limitation | ✅ org_id scoping enforces per-tenant boundaries | — |
| Storage limitation | ⚠️ No automatic data retention policy | Add retention TTLs |
| Integrity & confidentiality | ✅ TLS in transit, scoped access | Add encryption at rest |
| Privacy by default | ⚠️ No RLS, relies on app-level scoping | Add RLS (Section 3.2) |
| Accountability | ⚠️ Limited audit logging | Add audit log for all org mutations |

### 4.3 Article 32 — Security of Processing

| Measure | Status |
|---------|--------|
| Encryption in transit (TLS) | ✅ gRPC + HTTPS |
| Encryption at rest | ⚠️ Depends on infrastructure config |
| Access control (RBAC) | ✅ Role-based via org-core |
| Fail-closed on auth errors | ✅ Data Plane denies access if org-core unreachable |
| Audit logging | ⚠️ NATS events exist but no dedicated audit log |
| Regular security testing | ⚠️ Not evidenced in codebase |

### 4.4 Data Processing Records (Article 30)

**Recommendation:** Create a data processing inventory that maps:
- What personal data is stored per organization
- Where it is stored (which service, which database)
- Retention periods
- Legal basis for processing
- Sub-processors (if any cloud services are used)

---

## 5. Recommended Implementation Roadmap

### Immediate (Before Next Release)

| # | Action | Files | Priority |
|---|--------|-------|----------|
| 1 | Add tenant-scoping middleware to org-core HTTP routes | `internal/http/handlers.go`, new `internal/http/middleware/tenant.go` | HIGH |
| 2 | Add UUID validation for all org_id path parameters | `internal/http/middleware/tenant.go` | HIGH |
| 3 | Add Redis TTL (5 min) to all org-core cache entries | `internal/org/repository.go` | MEDIUM |
| 4 | Verify GDPR hard-delete covers Data Plane PostgreSQL tables | Data Plane migrations | MEDIUM |

### Short-Term (Next Sprint)

| # | Action | Files | Priority |
|---|--------|-------|----------|
| 5 | Implement comprehensive GDPR deletion across all stores | New `internal/org/gdpr.go` | HIGH |
| 6 | Add PostgreSQL RLS policies for defense-in-depth | New migration | MEDIUM |
| 7 | Add audit logging for all org mutations | `internal/org/service_enhanced.go` | MEDIUM |
| 8 | Verify JWT org_id claim issuance in auth-core | auth-core token service | MEDIUM |

### Medium-Term (Next Quarter)

| # | Action | Priority |
|---|--------|----------|
| 9 | Implement data retention policies per GDPR Article 5(1)(e) | MEDIUM |
| 10 | Add connection pool metrics to Prometheus | LOW |
| 11 | Scale pgxpool MaxConns for production workload | LOW |
| 12 | Implement regular automated security scanning | MEDIUM |

---

## 6. Conclusion

The org-core → Data Plane multi-tenant isolation pattern is **architecturally sound** and follows the correct authority-flows-downward model. The fail-closed security posture is a strong foundation.

**Key strengths:**
- Every SQL query is org_id-scoped
- Redis cache is properly namespaced
- Data Plane auth is fail-closed
- Dual-NATS provides reliable event propagation

**Key gaps requiring attention:**
- No centralized tenant-scoping middleware (HIGH risk for future regressions)
- GDPR deletion is PostgreSQL-only, missing Redis/Qdrant/NATS/MinIO cleanup
- No RLS as defense-in-depth
- Cache entries lack explicit TTL

Addressing these gaps will elevate the multi-tenant security posture from **good** to **best-practice compliant** and ensure full GDPR alignment.
