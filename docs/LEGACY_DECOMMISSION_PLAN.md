# Legacy Decommission Plan

> **Step 10 of 10** — Phase D: Runtime Enforcement  
> Decommission legacy authority and compatibility surfaces.  
> Created as the final deliverable of the CoreSystem Pyramid Architecture migration.

## Guiding Principles

1. **Authority flows DOWN the pyramid only** — L1 Control → L2 Data → L3 Ingestion → L4 Model → L5 Application → L6 Frontend.
2. **org-core's multi-tenant isolation is CORRECT** — strengthen for GDPR, never weaken.
3. **Pyramid model must be enforced in runtime configuration**, not just docs.
4. **Keep legacy compatibility shims ONLY when they have an explicit owner, migration target, and removal gate.**
5. **Before removing any legacy surface**, verify no remaining compose/env/config references and no active callers in runtime configs.

---

## 1. Model Plane v1 Authority Behavior Catalog

### 1.1 v1 Architecture (apps/Model Plane/)

| Service | Container Port | Host Port | gRPC Host Port | Key Dependencies |
|---------|---------------|-----------|-----------------|------------------|
| ai-core | 8000 | 8000 | 51051→50051 | Qdrant, Redis, NATS, Letta |
| agent-core | 8001 | 8101 | 51052→50052 | **Neo4j**, PG, Redis, NATS |

- **ai-core** acts as a direct LLM gateway with tool-calling, Letta memory integration, and Qdrant vector search.
- **agent-core** implements graph-based reasoning via Neo4j, orchestrating multi-step agent workflows.
- **Infrastructure:** PG 15 (:55432), Redis (:6389), Qdrant (:7333/:7334), **Neo4j 5-community** (:7574→7474/:7787→7687), NATS 2.10 (:4225→4222/:8225→8222).
- **Network:** `model-plane-net` (bridge mode).
- **ai-core sub-compose** (`apps/Model Plane/ai-core/docker-compose.yml`): Standalone dev compose for ai-core isolation. Uses `model-plane-network` (external, separate from main `model-plane-net`).
- **Letta (v1):** Azure OpenAI, plain-text password, `letta-data` volume, port 8283:8283.

### 1.2 v2 Architecture (apps/Model Plane v2/)

| Service | Container Port | Host Port | Key Dependencies |
|---------|---------------|-----------|------------------|
| ai-core (v2) | 8001 | 8101 (gRPC: 50061→50051) | All v2 services, Letta |
| agent-core-v2 | 8002 | 8102 | PG, Redis, NATS |
| execution-core-v2 | 8003 | 8103 | PG, Redis, NATS, Temporal |
| capability-core-v2 | 8004 | 8104 | PG, Redis, NATS |
| llm-worker | 8005 | 8105 | Redis, NATS |

- **v2 is a FULL replacement** — gated by `MODEL_PLANE_V2_ROLLOUT_PCT` (default 0, held in session-core).
- **Retired in v2:** Neo4j (graph reasoning replaced), v1 Qdrant, v1 NATS (replaced by reasoning-v2-nats :4227/:8227).
- **Evolved:** Letta (Azure OpenAI/plain pwd → direct OpenAI/secure pwd, feature-flagged via `LETTA_ENABLED`).
- **Network:** `reasoning-net` which is aliased to `ingestion-net` (external: true) — shares Docker network with Ingestion Plane.
- **Redis DB allocation:** DB0=agent-core-v2, DB2=execution-core-v2, DB4=capability-core-v2, DB5=llm-worker, DB6=ai-core(v2).

### 1.3 Port Conflicts (v1 ↔ v2 — MUTUALLY EXCLUSIVE)

| Resource | v1 | v2 | Conflict |
|----------|----|----|----------|
| Host port 8101 | agent-core | ai-core (v2) | ❌ Cannot coexist |
| Host port 8283 | letta-server (v1) | letta-server (v2) | ❌ Cannot coexist |

These conflicts confirm v1 and v2 are **mutually exclusive** at the infrastructure level.

---

## 2. Compatibility Shims Inventory

Every shim below requires an **explicit owner**, **migration target**, and **removal gate** before it can remain in the codebase.

### 2.1 Traffic Gate

| Shim | Location | Current Value | Owner | Migration Target | Removal Gate |
|------|----------|---------------|-------|-----------------|--------------|
| `MODEL_PLANE_V2_ROLLOUT_PCT` | session-core (Control Plane compose) | `0` (default) | Platform Team | Ramp to `100` | All v2 services stable for 2 weeks at 100% traffic |

### 2.2 Stale Cross-Plane References in Application Plane

**File:** `apps/Application Plane/docker-compose.yml` — convex-backend service

| Env Var | Current Value | Stale? | Migration Target |
|---------|---------------|--------|-----------------|
| `AI_CORE_URL` | `http://ai-core:8000` | ❌ v1 hostname + port | `http://ai-core-v2:8001` (v2 unified gateway) |
| `AUTH_SERVER_URL` | `http://auth-service:3011` | ❌ Stale env name + hostname | `AUTH_CORE_URL=http://auth-core:3011` |
| `ORG_CORE_URL` | `http://org-core-service:8080` | ❌ Stale hostname | `http://org-core:8080` |

**Owner:** Application Plane team  
**Removal Gate:** After v2 cutover (for AI_CORE_URL); immediately addressable (for AUTH_SERVER_URL, ORG_CORE_URL)

### 2.3 Stale Container Names in Control Plane

**File:** `apps/Control Plane/docker-compose.yml`

| Service Key (Canonical) | container_name (Stale) | Migration Target |
|------------------------|----------------------|-----------------|
| auth-core | `auth-service` | `auth-core` |
| user-core | `user-service` | `user-core` |
| org-core | `org-core-service` | `org-core` |
| billing-core | `billing-core-service` | `billing-core` |
| session-core | `session-core-service` | `session-core` |

**Additional:** `apps/Control Plane/docker-compose.recovery.override.yml` line 18: `container_name: billing-core-service-recovery` → `billing-core-recovery`

**Owner:** Control Plane team  
**Removal Gate:** Verify no external tooling (monitoring dashboards, CI scripts, health checks) references the old container names.

### 2.4 Feature Flags in Model Plane v2

| Flag | Service | Default | Purpose | Removal Gate |
|------|---------|---------|---------|--------------|
| `LETTA_ENABLED` | ai-core (v2) | `false` | Gates Letta memory integration | Letta v2 validated in staging |
| `INTENT_LLM_ENABLED` | ai-core (v2) | `true` | Gates intent classification LLM | Permanent feature flag (retain) |

### 2.5 Cross-Plane Bridge

| Shim | Location | Purpose | Removal Gate |
|------|----------|---------|--------------|
| `CONVEX_URL` | session-core (Control Plane) | L1→L5 bridge for session sync | Evaluate if session-core should call Application Plane (violates downward-only authority). Document exception or refactor. |

### 2.6 Root docker-compose.yml (ENTIRE FILE IS LEGACY)

**File:** `docker-compose.yml` (root, ~440 lines) — 20+ violations:

- Stale service keys: `auth-service`, `user-service` (should be `auth-core`, `user-core`)
- Stale container names matching stale service keys
- Legacy ports 8040, 50014 (all 8 occurrences confined to this file)
- Networks `aquatiq-local`, `controlplane-network` (external) — not used by canonical plane composes
- Duplicates services already defined in `apps/Control Plane/docker-compose.yml`

**Owner:** Platform Team  
**Removal Gate:** Verify no CI/CD pipeline, developer script, or Makefile target references `docker-compose.yml` at root. All operators must use plane-specific compose files.

---

## 3. Phased Decommission Plan

### Phase 1: Fix Stale Container Names (LOW RISK — Immediate)

**Scope:** Control Plane compose container_name values  
**Risk:** Low — service keys are already canonical; only Docker container labels change  
**Pre-check:** Scan monitoring dashboards, alerting rules, and CI scripts for old container names

1. Update `apps/Control Plane/docker-compose.yml`:
   - `auth-core` → `container_name: auth-core`
   - `user-core` → `container_name: user-core`
   - `org-core` → `container_name: org-core`
   - `billing-core` → `container_name: billing-core`
   - `session-core` → `container_name: session-core`
2. Update `apps/Control Plane/docker-compose.recovery.override.yml`:
   - `billing-core-service-recovery` → `billing-core-recovery`
3. Verify all health checks and inter-service DNS still work (DNS uses service key, not container_name).
4. Update `docs/SERVICE_NAME_REFERENCE_FIXES.md` with completion status.

### Phase 2: Update Application Plane Cross-Plane References (MEDIUM RISK)

**Scope:** convex-backend env vars in `apps/Application Plane/docker-compose.yml`  
**Risk:** Medium — incorrect hostname breaks cross-plane calls  
**Pre-check:** Verify target services are reachable on shared `verevon-net`

1. `AUTH_SERVER_URL=http://auth-service:3011` → `AUTH_CORE_URL=http://auth-core:3011`
2. `ORG_CORE_URL=http://org-core-service:8080` → `ORG_CORE_URL=http://org-core:8080`
3. `AI_CORE_URL=http://ai-core:8000` → **DEFER** to Phase 3 (depends on v2 cutover)
4. Test: `curl http://auth-core:3011/health` and `curl http://org-core:8080/health` from convex-backend container.
5. Verify convex-backend application code uses the corrected env var names.

### Phase 3: v1 → v2 Cutover via Rollout Gate (HIGH RISK — Staged)

**Scope:** Model Plane traffic migration  
**Risk:** High — full reasoning pipeline replacement  
**Pre-check:** All v2 services pass health checks; integration tests green; Letta v2 validated

1. **Stage 3a — Shadow mode (5%):** Set `MODEL_PLANE_V2_ROLLOUT_PCT=5` in session-core. Monitor latency, error rates, and result quality for 48 hours.
2. **Stage 3b — Canary (25%):** Ramp to 25%. Compare v1 and v2 outputs for consistency. Monitor for 1 week.
3. **Stage 3c — Majority (75%):** Ramp to 75%. v1 serves only fallback traffic. Monitor for 1 week.
4. **Stage 3d — Full cutover (100%):** Set to 100%. v1 receives zero traffic.
5. **Stage 3e — AI_CORE_URL migration:** Update convex-backend `AI_CORE_URL` from `http://ai-core:8000` to `http://ai-core-v2:8001` (or whatever v2 canonical hostname resolves to on `verevon-net`).
6. **Bake period:** Run at 100% v2 for **2 weeks minimum** before proceeding to Phase 4.

### Phase 4: Retire Root docker-compose.yml (MEDIUM RISK)

**Scope:** Remove or archive the root compose file  
**Risk:** Medium — may break developer workflows and CI  
**Pre-check:** Audit all Makefiles, scripts/, CI pipelines, and README for `docker-compose up` at root level

1. Search entire repo: `grep -rn "docker-compose" scripts/ Makefile* .github/ README* docs/`
2. Replace all root-level compose invocations with plane-specific compose paths.
3. Rename `docker-compose.yml` → `docker-compose.legacy.yml` with a deprecation header.
4. After 30 days with no usage, delete `docker-compose.legacy.yml`.
5. Remove `aquatiq-local` and `controlplane-network` (external) network definitions if no other compose references them.

### Phase 5: Remove Model Plane v1 (HIGH RISK — After Bake Period)

**Scope:** Delete entire `apps/Model Plane/` directory and its infrastructure  
**Risk:** High — permanent removal of fallback path  
**Pre-check:** `MODEL_PLANE_V2_ROLLOUT_PCT=100` stable for 2+ weeks; no active v1 callers in logs

1. Stop all v1 containers: `docker-compose -f apps/Model\ Plane/docker-compose.yml down`
2. Stop ai-core sub-compose: `docker-compose -f apps/Model\ Plane/ai-core/docker-compose.yml down`
3. Remove v1 volumes (after backup):
   - `model-plane-postgres-data`
   - `model-plane-redis-data`
   - `model-plane-qdrant-data`
   - `model-plane-neo4j-data`
   - `model-plane-nats-data`
   - `ai-core-postgres-data`, `letta-data`, `letta-postgres-data` (from ai-core sub-compose)
4. Archive `apps/Model Plane/` to a separate branch (e.g., `archive/model-plane-v1`).
5. Delete `apps/Model Plane/` from main branch.
6. Remove `MODEL_PLANE_V2_ROLLOUT_PCT` from session-core (no longer needed — v2 is the only path).
7. Clean up any remaining v1 references in `docs/`.

### Phase 6: Network Cleanup (LOW RISK — Final)

**Scope:** Remove orphaned Docker networks  
**Risk:** Low — networks are empty after v1 removal  
**Pre-check:** `docker network inspect <name>` shows no connected containers

1. Remove `model-plane-net` (v1 main compose network).
2. Remove `model-plane-network` (v1 ai-core sub-compose external network).
3. **Evaluate `reasoning-net` / `ingestion-net` aliasing:**
   - Currently `reasoning-net` = `ingestion-net` (external: true) — Model Plane v2 shares Ingestion Plane's network.
   - If this coupling is intentional and stable, document it as a permanent architectural decision.
   - If separation is desired, create a dedicated `reasoning-net` and update v2 compose + Temporal connectivity.
4. Verify remaining networks: `controlplane-net`, `data-net`, `ingestion-net`, `app-net`, `verevon-net` — all should have active containers.

---

## 4. NATS Topology Decommission

Three separate NATS instances exist:

| Instance | Plane | Host Ports | Network | Decommission? |
|----------|-------|------------|---------|--------------|
| controlplane-nats | L1 Control | 4223/8223 | controlplane-net + verevon-net | ❌ Retain |
| reasoning-nats (v1) | L4 v1 | 4225/8225 | model-plane-net | ✅ Remove with Phase 5 |
| reasoning-v2-nats | L4 v2 | 4227/8227 | reasoning-net (= ingestion-net) | ❌ Retain |

After v1 removal, only 2 NATS instances remain (controlplane + reasoning-v2).

---

## 5. Temporal Topology

| Instance | Plane | Host Port | Network |
|----------|-------|-----------|---------|
| Ingestion Plane temporal | L3 | 7233 | ingestion-net |
| Model Plane v2 temporal | L4 v2 | NONE (internal only) | reasoning-net (= ingestion-net) |

Both are on the same Docker network (`ingestion-net`). The v2 temporal-server intentionally has no host port — it is reachable only via the shared network. The Temporal UI is exposed on host port 8233→8080.

---

## 6. GDPR / Multi-Tenant Isolation Notes

- **org-core** (L1 Control Plane) enforces tenant isolation at the data layer. This boundary is **CORRECT** and must be **STRENGTHENED**, never weakened during decommission.
- No decommission phase should introduce cross-tenant data leakage paths.
- When removing v1 infrastructure (especially Neo4j), verify that no tenant-scoped graph data requires migration to v2.
- Letta memory stores (v1 → v2) may contain tenant-scoped conversation history — coordinate data migration before volume deletion.

---

## 7. Verification Protocol

Before executing each phase:

- [ ] Run `docker network inspect <network>` to confirm no unexpected containers
- [ ] Run `docker-compose config` on target compose to validate syntax
- [ ] Verify health endpoints for all affected services
- [ ] Check runtime logs for 404s / connection refused on migrated endpoints
- [ ] Run smoke test suite (`scripts/smoke_test.sh`)
- [ ] Confirm no remaining references: `grep -rn "<old_value>" apps/ docker-compose* scripts/`

---

## 8. Summary Timeline

| Phase | Risk | Duration | Depends On |
|-------|------|----------|------------|
| 1. Fix container names | Low | 1 day | — |
| 2. Update App Plane refs | Medium | 1 day | Phase 1 |
| 3. v1→v2 cutover | High | 4–6 weeks | Phase 2 |
| 4. Retire root compose | Medium | 1 week + 30-day bake | Phase 1 |
| 5. Remove Model Plane v1 | High | 1 day + backup | Phase 3 (2-week bake) |
| 6. Network cleanup | Low | 1 day | Phase 5 |

**Total estimated duration:** 8–10 weeks from start to full decommission.

---

*Document created as Step 10 of the CoreSystem Pyramid Architecture 10-step master plan.*  
*All preceding steps (1–9) are complete. See `docs/SERVICE_NAME_REFERENCE_FIXES.md`, `docs/CROSS_PLANE_CONTRACT_MATRIX.md`, and plane-specific architecture docs for prior work.*
