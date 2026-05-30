# Legacy Model Plane v1 — Cleanup & Decommission Plan

> **Step 7 Deliverable** — Phase B of the CoreSystem Architecture Hardening Plan  
> **Created:** 2026-07-08  
> **Status:** READY FOR EXECUTION  
> **Risk Level:** MEDIUM — Active canary routing in Control Plane SESSION-CORE

---

## 1. Executive Summary

CoreSystem currently operates **two** Model Plane directories in parallel:

| Attribute | Model Plane v1 (`apps/Model Plane/`) | Model Plane v2 (`apps/Model Plane v2/`) |
|---|---|---|
| Status | **Legacy — migration complete** | **Canonical — active** |
| Services | ai-core, agent-core, operator-core | ai-core, agent-core-v2, execution-core-v2, capability-core-v2, llm-worker |
| Infra | postgres:15, redis:7, qdrant, neo4j, nats | postgres:16, redis, nats, minio |
| Network | `model-plane-net` | `reasoning-net` |
| Migration evidence | `IMPORT_MIGRATION_COMPLETE.md` (2026-02-19 ✅) | N/A (active target) |

The v1 migration was **completed on 2026-02-19** but the directory was never decommissioned. Control Plane `session-core` still contains **active canary routing** between v1 and v2, meaning v1 infrastructure may still receive traffic depending on the `MODEL_PLANE_V2_ROLLOUT_PCT` environment variable (default: `0`, meaning 100% goes to v1 unless explicitly changed).

**⚠️ CRITICAL:** Before decommissioning v1, confirm `MODEL_PLANE_V2_ROLLOUT_PCT=100` is set and verified in production.

---

## 2. Legacy v1 Full Infrastructure Inventory

### 2.1 Infrastructure Containers

| Container | Image | Host Port(s) | Internal Port(s) | Memory Limit | Notes |
|---|---|---|---|---|---|
| `reasoning-postgres` | postgres:15-alpine | 55432 | 5432 | 512M / 1 CPU | DB: `model_plane_db`, User: `reasoning_user` |
| `reasoning-redis` | redis:7-alpine | 6389 | 6379 | 128M | Password: `reasoning_redis_password_2026` |
| `reasoning-qdrant` | qdrant/qdrant:latest | 7333, 7334 | 6333, 6334 | 512M | API key: `reasoning_qdrant_key_2026` |
| `reasoning-neo4j` | neo4j:5-community | 7574, 7787 | 7474, 7687 | 768M | GDS plugin, auth: `neo4j/reasoning_neo4j_password_2026` |
| `reasoning-nats` | nats:2.10-alpine | 4225, 8225 | 4222, 8222 | 128M | JetStream enabled |

**Total v1 infra memory reservation: ~2.0 GB**

### 2.2 Application Services

| Container | Host Port(s) | Internal Port(s) | Memory Limit | Dependencies |
|---|---|---|---|---|
| `reasoning-ai-core` | 8000, 51051 | 8000, 50051 | 768M / 1 CPU | postgres, redis, qdrant, nats |
| `agent-core` | 8101, 51052 | 8001, 50052 | 384M / 0.5 CPU | postgres, redis, neo4j, ai-core |
| `operator-core` | (see compose) | — | — | ai-core, agent-core |

### 2.3 Monitoring Stack (Optional Profile)

| Container | Host Port | Memory Limit |
|---|---|---|
| `reasoning-prometheus` | 39090 | 256M |
| `reasoning-grafana` | 33000 | 128M |

### 2.4 Docker Volumes

```
reasoning-postgres-data
reasoning-redis-data
reasoning-qdrant-data
reasoning-neo4j-data
reasoning-nats-data
```

### 2.5 Docker Network

- **`model-plane-net`** — Private bridge network for all v1 services
- **`velion-net`** — Shared external network for cross-plane communication

### 2.6 Codebase Inventory

**ai-core (70+ files):**
- Python service with FastAPI + gRPC
- Config: `pyproject.toml`, `pyrightconfig.json`, `mypy.ini`, `requirements.txt`
- Dual virtual environments: `.venv/` + `venv/`
- TOML-based Skills & Tools ecosystem (85% complete, SQLAlchemy+UUID+versioning)
- Phase docs: PHASE_1_3 through PHASE_4_3
- Migration records: `IMPORT_MIGRATION_COMPLETE.md`, `ORG_CORE_MIGRATION_COMPLETE.md`
- 6 Python scripts: `assess_frameworks.py`, `assess_letta.py`, `fix_all_params.py`, `fix_params.py`, `reorder_all.py`, `test_framework_functionality.py`
- 6 Shell scripts: `test_frameworks_final.sh`, `test_functional.sh`, `test_letta_chat.sh`, `test_letta_integration.sh`, `test_quick.sh`, `verify_framework_functionality.sh`
- Compliance: `TOON_COMPLIANCE_AUDIT.md`, `TOON_COMPLIANCE_VERIFICATION.txt`
- Multiple .env files: `.env`, `.env.docker`, `.env.example`, `.env.test`

**agent-core:**
- Python service with Neo4j graph connectivity
- Dependencies on ai-core via HTTP + gRPC

**operator-core:**
- Orchestration layer (superseded by `capability-core-v2` + `execution-core-v2` in v2)

---

## 3. Model Plane v2 Service Evolution

### 3.1 Service Mapping (v1 → v2)

| v1 Service | v2 Service(s) | Change |
|---|---|---|
| ai-core | ai-core | Rewritten, new ports (:8001/:50051) |
| agent-core | agent-core-v2 | Rewritten (:8002/:50053) |
| operator-core | capability-core-v2 + execution-core-v2 | **Split** into two services (:8003, :8004) |
| — | llm-worker | **New** service (:8005) |

### 3.2 Infrastructure Differences

| Resource | v1 | v2 | Notes |
|---|---|---|---|
| PostgreSQL | 15-alpine (:55432) | 16-alpine (:55433) | Adjacent port, different DB name |
| Redis | 7-alpine (:6389) | latest (:6390) | Adjacent port |
| Qdrant | ✅ (:7333/:7334) | ❌ | Moved to Data Plane |
| Neo4j | ✅ (:7574/:7787) | ❌ | Moved to Data Plane |
| NATS | ✅ (:4225/:8225) | ✅ (:4227/:8227) | Both have JetStream |
| MinIO | ❌ | ✅ (:9000/:9001) | New in v2 |
| Network | `model-plane-net` | `reasoning-net` | Complete isolation |

### 3.3 Key Architectural Shifts

1. **Vector/graph storage externalized** — Qdrant and Neo4j are no longer part of Model Plane; Data Plane handles vector and graph storage centrally
2. **Object storage added** — MinIO for model artifacts, execution results
3. **operator-core decomposed** — Cleaner separation of concerns between capabilities registry and execution engine
4. **LLM processing isolated** — Dedicated llm-worker for model inference workloads

---

## 4. Migration Completion Evidence

| Document | Location | Date | Status |
|---|---|---|---|
| `IMPORT_MIGRATION_COMPLETE.md` | `apps/Model Plane/ai-core/` | 2026-02-19 | ✅ Complete |
| `ORG_CORE_MIGRATION_COMPLETE.md` | `apps/Model Plane/ai-core/` | — | ✅ Complete |
| `MODULAR_ARCHITECTURE_COMPLETE.md` | `apps/Model Plane/ai-core/` | — | ✅ Complete |
| `OPTIMIZATION_COMPLETE.md` | `apps/Model Plane/ai-core/` | — | ✅ Complete |

---

## 5. Cross-Plane Dependencies (CRITICAL)

### 5.1 Control Plane SESSION-CORE

**File:** `apps/Control Plane/docker-compose.yml`

```yaml
# Line 331 — Service definition comment:
# SESSION-CORE (session harness for Model Plane v1/v2)

# Line 356 — Environment variable:
# Canary ramp: set to 0-100 to route that % of tenants to Model Plane v2
MODEL_PLANE_V2_ROLLOUT_PCT: ${MODEL_PLANE_V2_ROLLOUT_PCT:-0}
```

**⚠️ Default value is `0`**, meaning ALL traffic goes to v1 by default. This MUST be set to `100` before any decommission activity.

### 5.2 Shared Network

Both v1 and v2 join `velion-net` (external bridge network) for cross-plane communication. After v1 decommission, only v2 services will remain on `velion-net`.

---

## 6. Data Migration Verification Checklist

Before decommission, verify no data remains exclusively in v1 infrastructure:

- [ ] **Qdrant collections** — All vectors migrated to Data Plane Qdrant (v1.9.0 on :6333/:6334)
- [ ] **Neo4j graph data** — All graph relationships migrated or no longer needed
- [ ] **PostgreSQL `model_plane_db`** — All tables empty or migrated to `model_plane_v2_db`
- [ ] **Redis cache** — No persistent state (cache-only, safe to drop)
- [ ] **NATS JetStream** — All streams/consumers recreated in v2 NATS
- [ ] **Skills TOML files** — Historical skills data archived if needed
- [ ] **ai-core `/app/skills` mount** — Skills system superseded by v2 implementation

---

## 7. Decommission Roadmap

### Phase 1: Canary Ramp to 100% v2
```bash
# Set in Control Plane .env or docker-compose override:
MODEL_PLANE_V2_ROLLOUT_PCT=100

# Restart session-core to apply:
cd "apps/Control Plane"
docker-compose restart session-core

# Monitor for 48-72 hours, verify:
# - Zero requests reaching v1 services
# - All session creation succeeds via v2
# - No error spikes in session-core logs
```

### Phase 2: Git Archive v1 Directory
```bash
# Create archive branch for historical reference:
git checkout -b archive/model-plane-v1
git add "apps/Model Plane/"
git commit -m "chore: archive Model Plane v1 before decommission"
git push origin archive/model-plane-v1
```

### Phase 3: Stop v1 Containers
```bash
cd "apps/Model Plane"
docker-compose down

# Verify stopped:
docker ps | grep -E "reasoning-(ai-core|postgres|redis|qdrant|neo4j|nats|prometheus|grafana)|agent-core"
```

### Phase 4: Remove Docker Volumes (DESTRUCTIVE)
```bash
# ⚠️ Only after data migration verification (Section 6) is complete
docker volume rm \
  reasoning-postgres-data \
  reasoning-redis-data \
  reasoning-qdrant-data \
  reasoning-neo4j-data \
  reasoning-nats-data
```

### Phase 5: Update Control Plane References
```yaml
# In apps/Control Plane/docker-compose.yml:
# 1. Remove v1/v2 comment from SESSION-CORE header (line 331)
# 2. Remove MODEL_PLANE_V2_ROLLOUT_PCT env var (line 356-357)
# 3. Update session-core code to remove canary routing logic
```

### Phase 6: Remove v1 Directory from Main Branch
```bash
git rm -r "apps/Model Plane/"
git commit -m "chore: remove decommissioned Model Plane v1

Migration completed 2026-02-19. Archived in branch archive/model-plane-v1.
All services now run exclusively on Model Plane v2."
```

### Phase 7: Reclaim Port Reservations
The following ports become available after v1 removal:

| Port | Was Used By | Status After Decommission |
|---|---|---|
| 55432 | v1 postgres | Available |
| 6389 | v1 redis | Available |
| 7333, 7334 | v1 qdrant | Available |
| 7574, 7787 | v1 neo4j | Available |
| 4225, 8225 | v1 nats | Available |
| 8000, 51051 | v1 ai-core | Available |
| 8101, 51052 | v1 agent-core | Available |
| 39090 | v1 prometheus | Available |
| 33000 | v1 grafana | Available |

---

## 8. Risk Assessment

| Risk | Severity | Mitigation |
|---|---|---|
| Canary ramp default is `0` (100% v1) | **CRITICAL** | Explicitly set `MODEL_PLANE_V2_ROLLOUT_PCT=100` and verify |
| Port adjacency confusion (55432 vs 55433) | MEDIUM | Document port assignments; remove v1 ports from reservation |
| Legacy container name references in scripts | MEDIUM | Search all scripts for `reasoning-postgres`, `reasoning-redis`, `reasoning-qdrant`, `reasoning-neo4j` |
| Qdrant data not migrated to Data Plane | HIGH | Run verification query against Data Plane Qdrant before removing v1 volumes |
| Neo4j graph data loss | HIGH | Export graph dump before volume removal |
| v1 skills/ historical data | LOW | Archive to git branch; v2 has independent skills system |
| Shared `velion-net` disruption | LOW | v2 already on `velion-net`; removing v1 only removes v1 connections |
| Hardcoded credentials in v1 .env files | MEDIUM | Rotate all credentials listed in v1 compose after decommission |

---

## 9. Credential Rotation (Post-Decommission)

After v1 is fully removed, rotate these credentials to prevent stale references:

| Credential | Current Value (v1) | Action |
|---|---|---|
| PostgreSQL password | `reasoning_secure_password_2026` | Rotate if shared with any other system |
| Redis password | `reasoning_redis_password_2026` | Rotate if shared |
| Qdrant API key | `reasoning_qdrant_key_2026` | Revoke (service removed) |
| Neo4j auth | `reasoning_neo4j_password_2026` | Revoke (service removed) |
| Grafana admin | `reasoning_grafana_admin_2026` | Revoke (monitoring removed) |

---

## 10. Success Criteria

The decommission is complete when:

- [ ] `MODEL_PLANE_V2_ROLLOUT_PCT=100` confirmed running for 72+ hours with zero v1 traffic
- [ ] All data migration verification checks (Section 6) pass
- [ ] `apps/Model Plane/` archived in separate git branch
- [ ] `apps/Model Plane/` removed from main branch
- [ ] All v1 Docker containers stopped and volumes removed
- [ ] Control Plane session-core canary routing code removed
- [ ] Port reservations updated in documentation
- [ ] Credentials rotated
- [ ] No remaining references to v1 container names in scripts or configs
- [ ] Zero errors in v2 services for 7 days post-decommission
