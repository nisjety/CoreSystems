# Service Name Reference Fixes

> Step 9 deliverable — Enforcement and observability gates for cross-plane naming contracts.

## Executive Summary

**Total stale references identified: ~80+** across 6 planes and 4 file types.

| Category | Count | Severity |
|----------|-------|----------|
| Container names (docker-compose) | 5 | HIGH — breaks `docker exec` and inter-service DNS |
| Docker-compose hostnames / env vars | ~12 | HIGH — breaks runtime service discovery |
| Environment files (.env / .env.example) | ~25 | MEDIUM — breaks local + Docker dev |
| Go source code (`os.Getenv`) | ~13 | MEDIUM — reads stale var names |
| TypeScript source code | ~30 | MEDIUM — reads stale var names, some with stale fallback hostnames |
| Port inconsistencies | 2 clusters | HIGH — wrong port = silent failures |

**"Double stale"** = both the env var name AND the fallback hostname are wrong (highest priority).

---

## Canonical Naming Convention

All cross-plane service references MUST follow:

| Service | Env Var | Hostname | Port |
|---------|---------|----------|------|
| Auth | `AUTH_CORE_URL` | `auth-core` | 3011 |
| User | `USER_CORE_URL` | `user-core` | 3012 |
| Org | `ORG_CORE_URL` | `org-core` | 8080 |
| Billing | `BILLING_CORE_URL` | `billing-core` | 3014 |
| Session | `SESSION_CORE_URL` | `session-core` | — |
| AI | `AI_CORE_URL` | `ai-core` | 8001 (HTTP), 50051 (gRPC) |
| Documents | `DOCUMENT_CORE_URL` | `documents-service` | 8001 (HTTP), 50051 (gRPC) |
| User gRPC | `USER_CORE_GRPC_URL` | `user-core` | 50012 |

**Internal self-references** (e.g., `BETTER_AUTH_URL` inside auth-core) are exempt from the cross-plane naming rule but must use the correct port.

---

## 1. Container Name Fixes (docker-compose)

### `apps/Control Plane/docker-compose.yml`

| Line | Current (Stale) | Canonical |
|------|----------------|-----------|
| 136 | `container_name: auth-service` | `container_name: auth-core` |
| 182 | `container_name: user-service` | `container_name: user-core` |
| 231 | `container_name: org-core-service` | `container_name: org-core` |
| 285 | `container_name: billing-core-service` | `container_name: billing-core` |
| 338 | `container_name: session-core-service` | `container_name: session-core` |

### `docker-compose.yml` (root)

| Line | Current (Stale) | Canonical |
|------|----------------|-----------|
| 55 | `container_name: org-core-service` | `container_name: org-core` |
| 92 | `container_name: user-service` | `container_name: user-core` |
| 136 | `container_name: auth-service` | `container_name: auth-core` |

### `apps/Control Plane/docker-compose.recovery.override.yml`

| Line | Current | Note |
|------|---------|------|
| 15 | `container_name: org-core-service-recovery` | → `org-core-recovery` |
| 18 | `container_name: billing-core-service-recovery` | → `billing-core-recovery` |

---

## 2. Docker-Compose Hostname & Env Var Fixes

### Root `docker-compose.yml`

| Line(s) | Current | Fix |
|---------|---------|-----|
| 61, 154 | `USER_SERVICE_GRPC_URL=user-service:50012` | `USER_CORE_GRPC_URL=user-core:50012` |
| 249 | `ORG_CORE_URL=http://org-core-service:8080` | `ORG_CORE_URL=http://org-core:8080` |
| 250 | `AUTH_CORE_URL=http://auth-service:3011` (if present) | `AUTH_CORE_URL=http://auth-core:3011` |
| 36, 39, 75, 88, 114, 132, 188 | Various `depends_on` / service refs to `user-service`, `auth-service` | Update service definition keys if renaming |

### `apps/Application Plane/docker-compose.yml`

| Line | Current | Fix |
|------|---------|-----|
| 34 | `ORG_CORE_URL: ${ORG_CORE_URL:-http://org-core-service:8080}` | `…http://org-core:8080}` |
| 35 | `AUTH_CORE_URL: ${AUTH_CORE_URL:-http://auth-service:3011}` | `…http://auth-core:3011}` |

### `apps/Application Plane/convex-core/docker-compose.yml`

| Line | Current | Fix |
|------|---------|-----|
| 34 | `ORG_CORE_URL: ${ORG_CORE_URL:-http://org-core-service:8080}` | `…http://org-core:8080}` |
| 35 | `AUTH_CORE_URL: ${AUTH_CORE_URL:-http://auth-service:3011}` | `…http://auth-core:3011}` |

---

## 3. Environment File Fixes

### Ingestion Plane

| File | Line | Current | Fix |
|------|------|---------|-----|
| `imports-core/.env.example:10` | `DOCUMENT_SERVICE_URL=http://document-service:3021` | Var→`DOCUMENT_CORE_URL`, host→`documents-service`, port→`8001` |
| `imports-core/.env.example:12` | `ORG_SERVICE_URL=http://org-core-service:8080` | Var→`ORG_CORE_URL`, host→`org-core` |
| `imports-core/.env:7` | `DOCUMENT_SERVICE_URL=http://mock-document-service:3030` | Var→`DOCUMENT_CORE_URL` (mock context) |
| `imports-core/.env:9` | `ORG_SERVICE_URL=http://org-core-service:8080` | Var→`ORG_CORE_URL`, host→`org-core` |
| `Quarry/.../user-core/.env.example:52-53` | `AUTH_SERVICE_URL`, `ORG_SERVICE_URL` (localhost) | Var names→`AUTH_CORE_URL`, `ORG_CORE_URL` |
| `Quarry/.../user-core/.env.docker:43-44` | `AUTH_SERVICE_URL`, `ORG_SERVICE_URL` (canonical hosts) | Var names only |

### Control Plane

| File | Line | Current | Fix |
|------|------|---------|-----|
| `org-core/.env.example:28-29` | `AUTH_SERVICE_URL`, `USER_SERVICE_URL` (localhost) | →`AUTH_CORE_URL`, `USER_CORE_URL` |
| `org-core/.env.docker:21-22` | `AUTH_SERVICE_URL`, `USER_SERVICE_URL` (canonical hosts) | Var names only |
| `org-core/.env:23-24` | `AUTH_SERVICE_URL`, `USER_SERVICE_URL` (localhost) | Var names |
| `org-core/.env.local:7-8` | `AUTH_SERVICE_URL=http://auth-service:3011`, `USER_SERVICE_URL=http://user-service:3012` | **DOUBLE STALE**: var + hostname |

### Application Plane

| File | Line | Current | Fix |
|------|------|---------|-----|
| `.env.example:37-38` | `AUTH_SERVICE_URL`, `USER_SERVICE_URL` (canonical hosts) | Var names→`AUTH_CORE_URL`, `USER_CORE_URL` |
| `.env:39-40` | Same | Same |
| `affine-core/.env:7-8` | `AUTH_SERVICE_URL`, `USER_SERVICE_URL` (canonical hosts) | Var names |
| `convex-core/.env.local:30` | `ORG_CORE_URL=http://org-core-service:8080` | Canonical var but **stale hostname** |
| `convex-core/.env.local:36` | `AUTH_SERVER_URL=http://auth-service:3011` | **DOUBLE STALE**: var→`AUTH_CORE_URL`, host→`auth-core` |
| `convex-core/.env.local:41` | `CONVEX_AUTH_JWKS_URL=http://auth-service:3011/…` | Stale hostname→`auth-core` |

---

## 4. Go Source Code Fixes

### Application Plane — `affine-core`

| File | Line | Current | Fix |
|------|------|---------|-----|
| `internal/config/config.go:38` | `getEnv("AUTH_SERVICE_URL", "http://auth-core:3011")` | Var→`AUTH_CORE_URL` |
| `internal/config/config.go:39` | `getEnv("USER_SERVICE_URL", "http://user-core:3012")` | Var→`USER_CORE_URL` |

### Control Plane — `org-core`

| File | Line | Current | Fix |
|------|------|---------|-----|
| `internal/config/config.go:45` | `getEnv("AUTH_SERVICE_URL", "http://auth-service:3011")` | **DOUBLE STALE**: var→`AUTH_CORE_URL`, fallback host→`auth-core` |
| `internal/config/config.go:46` | `getEnv("USER_SERVICE_URL", "http://user-service:3012")` | **DOUBLE STALE**: var→`USER_CORE_URL`, fallback host→`user-core` |
| `internal/config/config_test.go:19` | Test references `AUTH_SERVICE_URL`, `USER_SERVICE_URL` | Update to match |

### Control Plane — `billing-core`

| File | Line | Current | Fix |
|------|------|---------|-----|
| `internal/billing/service.go:47` | `os.Getenv("ORG_SERVICE_URL")` | Var→`ORG_CORE_URL` |

### Control Plane — `user-core`

| File | Line | Current | Fix |
|------|------|---------|-----|
| `internal/http/handlers.go:1710` | `os.Getenv("AUTH_SERVICE_URL")` | Var→`AUTH_CORE_URL` |
| `internal/http/server.go:45` | `os.Getenv("ORG_SERVICE_URL")` | Var→`ORG_CORE_URL` |
| `internal/http/server.go:330` | `os.Getenv("AUTH_SERVICE_URL")` | Var→`AUTH_CORE_URL` |

### Ingestion Plane — `Quarry/third_party/user-core` (vendored copy)

| File | Line | Current | Fix |
|------|------|---------|-----|
| `internal/http/handlers.go:1710` | `os.Getenv("AUTH_SERVICE_URL")` | Var→`AUTH_CORE_URL` |
| `internal/http/server.go:45` | `os.Getenv("ORG_SERVICE_URL")` | Var→`ORG_CORE_URL` |
| `internal/http/server.go:330` | `os.Getenv("AUTH_SERVICE_URL")` | Var→`AUTH_CORE_URL` |

> **Note:** Quarry's `third_party/user-core` is a vendored copy of the Control Plane `user-core`. Fixes must be applied to both or the vendor copy re-synced.

---

## 5. TypeScript Source Code Fixes

### Frontend Plane — `velion`

| File | Line | Current | Fix |
|------|------|---------|-----|
| `src/lib/rpc/server.ts:20` | `AUTH_SERVICE_URL` | Var→`AUTH_CORE_URL` |
| `src/lib/rpc/server.ts:21` | `USER_SERVICE_URL` | Var→`USER_CORE_URL` |
| `src/lib/rpc/server.ts:22` | `ORG_SERVICE_URL` | Var→`ORG_CORE_URL` |
| `src/lib/rpc/server.ts:23` | `BILLING_SERVICE_URL` | Var→`BILLING_CORE_URL` |
| `src/lib/server/sidebar-data.ts:11` | `USER_SERVICE_URL` | Var→`USER_CORE_URL` |
| `src/lib/server/active-org.ts:4-5` | `USER_SERVICE_URL`, `ORG_SERVICE_URL` | Var names |
| `src/components/auth/lib/auth-server.ts:7` | `AUTH_SERVICE_URL \|\| BACKEND_URL \|\| 'http://auth-service:3011'` | **DOUBLE STALE**: var + fallback host |
| `src/app/api/notifications/_lib/auth-session.ts:3` | `DEFAULT_AUTH_SERVICE_URL = 'http://auth-service:3011'` | **DOUBLE STALE** |
| `src/app/api/notifications/_lib/auth-session.ts:16` | `AUTH_SERVICE_URL ?? DEFAULT_AUTH_SERVICE_URL` | Var→`AUTH_CORE_URL` |
| `src/app/api/user/current/route.ts:4` | `AUTH_SERVICE_URL \|\| 'http://auth-service:3011'` | **DOUBLE STALE** |
| `src/app/api/user/[...path]/route.ts:3-4` | `USER_SERVICE_URL`, `AUTH_SERVICE_URL` | Var names |

### Frontend Plane — `avelis`

| File | Line | Current | Fix |
|------|------|---------|-----|
| `src/lib/config.ts:3` | `DEFAULT_AUTH_SERVICE_URL = 'http://auth-core:3011'` | Var→`DEFAULT_AUTH_CORE_URL` (hostname already canonical) |
| `src/lib/config.ts:16,18` | Reads `AUTH_SERVICE_URL` | Var→`AUTH_CORE_URL` |
| `jest.setup.ts:1` | `process.env.AUTH_SERVICE_URL = 'http://auth-core:3011'` | Var→`AUTH_CORE_URL` |
| `__tests__/lib/config.test.ts:28-35` | `AUTH_SERVICE_URL` | Update tests |
| `__tests__/lib/auth-session.test.ts:12` | `AUTH_SERVICE_URL: 'http://auth-core:3011'` | Var→`AUTH_CORE_URL` |

### Application Plane — `convex-core`

| File | Line | Current | Fix |
|------|------|---------|-----|
| `convex.config.ts:13` | `AUTH_SERVER_URL \|\| "http://localhost:3011"` | Var→`AUTH_CORE_URL` |

---

## 6. Port Inconsistencies

### AI_CORE_URL — Port 8000 vs 8001

| File | Value | Correct? |
|------|-------|----------|
| `Application Plane/convex-core/.env.local:26` | `http://ai-core:8000` | ❌ Should be `8001` |
| `Data Plane/retrieval/.env:9` | `http://ai-core:8001` | ✅ |
| Cross-plane contract matrix | `8001` (HTTP), `50051` (gRPC) | ✅ |

### BETTER_AUTH_URL — Port 3000 vs 3011

`BETTER_AUTH_URL` is auth-core's internal self-reference (not a cross-plane contract violation) but has inconsistent fallback ports inside `auth-core/src/auth/orpc-router.ts`:

| Lines | Fallback | Correct? |
|-------|----------|----------|
| 754, 814, 869, 925, 977, 1043, 1096, 1155, 1606, 1682 | `'http://localhost:3011'` | ✅ |
| 1956, 2225, 2893, 2953, 3011, 3153, 3230 | `'http://localhost:3000'` | ❌ Should be `3011` |

---

## 7. Verified Canonical References (no changes needed)

These already follow the naming convention:

| File | Variable | Value |
|------|----------|-------|
| `Ingestion Plane/.env.example:27-32` | `AUTH_CORE_URL`, `USER_CORE_URL`, `ORG_CORE_URL`, `BILLING_CORE_URL` | ✅ |
| `Ingestion Plane/integration-core/.env:14-18` | All `*_CORE_URL` | ✅ |
| `Control Plane/session-core/.env.docker:21` | `AUTH_CORE_URL` | ✅ |
| `Data Plane/retrieval/.env:9` | `AI_CORE_URL=http://ai-core:8001` | ✅ |
| `Model Plane v2/.env.example:27` | `AUTH_CORE_URL` | ✅ |

---

## 8. Enforcement Recommendations

### 8.1 CI Lint Rule (grep-based contract test)

Add to CI pipeline:

```bash
#!/usr/bin/env bash
# scripts/lint-service-names.sh
set -euo pipefail

STALE_PATTERNS=(
  'AUTH_SERVICE_URL'
  'USER_SERVICE_URL'
  'ORG_SERVICE_URL'
  'BILLING_SERVICE_URL'
  'AUTH_SERVER_URL'
  'DOCUMENT_SERVICE_URL'
  'auth-service:3011'
  'user-service:3012'
  'org-core-service:8080'
  'billing-core-service:3014'
  'document-service:3021'
)

EXIT_CODE=0
for pattern in "${STALE_PATTERNS[@]}"; do
  MATCHES=$(grep -rn "$pattern" \
    --include='*.ts' --include='*.tsx' --include='*.go' \
    --include='*.yml' --include='*.yaml' --include='*.env*' \
    apps/ docker-compose.yml 2>/dev/null || true)
  if [[ -n "$MATCHES" ]]; then
    echo "❌ STALE: $pattern"
    echo "$MATCHES" | head -5
    EXIT_CODE=1
  fi
done

exit $EXIT_CODE
```

### 8.2 Pre-commit Hook

```yaml
# .pre-commit-config.yaml (append)
- repo: local
  hooks:
    - id: no-stale-service-names
      name: Block stale service name references
      entry: bash scripts/lint-service-names.sh
      language: system
      pass_filenames: false
```

### 8.3 Observable Warnings (runtime)

For Go services, add startup warnings:

```go
// Emit a deprecation warning if stale env var is set
if v := os.Getenv("AUTH_SERVICE_URL"); v != "" {
    log.Warn("DEPRECATED: AUTH_SERVICE_URL is set. Migrate to AUTH_CORE_URL.")
}
```

For TypeScript services:

```typescript
if (process.env.AUTH_SERVICE_URL) {
  console.warn('DEPRECATED: AUTH_SERVICE_URL is set. Migrate to AUTH_CORE_URL.');
}
```

### 8.4 Migration Path

To avoid breaking running deployments, set both old and new vars during migration:

```env
# Phase 1: Set both (backwards compatible)
AUTH_SERVICE_URL=http://auth-core:3011
AUTH_CORE_URL=http://auth-core:3011

# Phase 2: Code reads new var, falls back to old
# Phase 3: Remove old var from all configs
```

---

## 9. Org-Core Multi-Tenant Isolation Note

> **CRITICAL DIRECTIVE:** org-core's multi-tenant isolation is architecturally CORRECT and must be STRENGTHENED for GDPR compliance — never weakened. Renaming `org-core-service` → `org-core` is purely a naming/DNS change and does NOT affect the isolation boundary.

---

## 10. Legacy Model Plane v1

The legacy Model Plane v1 (`reasoning-ai-core`, `agent-core`, etc.) coexists alongside v2. Service name deduplication between v1 and v2 is deferred to **Step 10** (decommission legacy authority and compatibility surfaces).
