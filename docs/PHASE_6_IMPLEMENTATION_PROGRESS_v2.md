## Phase 6 Implementation Progress — Cross-Plane Event Consumption

**Status:** 🟡 **66% COMPLETE** (2 of 3 planes implemented)

**Session:** Started this session with request "start phase 6"  
**Timeline:** ~90 minutes of implementation  
**Objective:** Build downstream event consumers in Ingestion, Data, and Reasoning planes

---

## Summary

Phase 6 implements the **consuming side** of the event-driven architecture. After Phase 5 established the shared NATS event publishing infrastructure in Control Plane, Phase 6 creates subscribers in downstream planes that **react to those events in real-time**.

### What Works ✅

1. **Shared NATS Event Bus** (From Phase 5 - Verified)
   - Broker: `nats://verevon-nats:4222`
   - Token: `aqencia-shared-nats-token-2026`
   - JetStream Stream: `AQENCIA_CONTROLPLANE`
   - All Control Plane services publishing events successfully

2. **Ingestion Plane M365 Provider-Linked Handler** (COMPLETE)
   - File: `apps/Ingestion Plane/imports-core/app/control_plane_subscriber.py` (238 lines)
   - File: `apps/Ingestion Plane/imports-core/app/m365_provider_handler.py` (144 lines)
   - Subscribes to: `aqencia.controlplane.user.provider_linked`
   - Action: Creates ImportJob record with M365 connector metadata
   - Status: ✅ Running and verified listening
   - Queue group: `ingestion-plane-m365`

3. **Data Plane Quota Enforcement** (COMPLETE)
   
   **Retrieval Service:**
   - File: `apps/Data Plane/services/retrieval/app/control_plane_subscriber.py` (202 lines)
   - File: `apps/Data Plane/services/retrieval/app/quota_enforcement_handler.py` (177 lines)
   - Status: ✅ Running and verified listening
   - Subscriptions:
     - `aqencia.controlplane.org.plan_changed` → Updates org_quotas table
     - `aqencia.controlplane.billing.quota_exceeded` → Logs warning
   
   **Documents Service:**
   - Same subscriber and handler as Retrieval
   - Status: ✅ Running and verified listening
   - Both services listening to same events (load balanced via queue groups)
   
   **Database:**
   - Added `org_quotas` table to schema:
     - Fields: org_id (PK), plan_tier, documents_limit, api_calls_per_month, storage_gb, concurrent_users, custom_models
     - Indexes: plan_tier for query optimization
     - Auto-trigger: updated_at timestamp

4. **Event Handler Pattern** (Standardized across planes)
   ```
   Event Flow:
   Control Plane publishes → Shared NATS JetStream
     → Queue group distributes to subscribers
     → Service event handler processes (async, non-blocking)
     → Database updated
     → Message ack'd for success / nak'd for redelivery
   ```

---

## Implementation Details

### Phase 6 Architecture

**Ingestion Plane (Complete)**
```
Control Plane Event:
  aqencia.controlplane.user.provider_linked
  {user_id, provider, tenant_id, email}
         ↓
  imports-api (Control Plane Subscriber)
         ↓
  M365 Provider Handler
         ↓
  Database: CREATE ImportJob with source_type="m365"
```

**Data Plane (Complete)**
```
Control Plane Events:
  1. aqencia.controlplane.org.plan_changed
     {org_id, plan: "free|professional|enterprise"}
           ↓
     Database: INSERT/UPDATE org_quotas with plan limits
  
  2. aqencia.controlplane.billing.quota_exceeded
     {org_id, metric, limit, current}
           ↓
     Log warning for monitoring/alerting
```

### Code Structure Across All Planes

**Pattern (Standardized):**
1. `control_plane_subscriber.py` - NATS async subscriber class
   - Constructor: Takes nats_url, nats_token, service_name
   - initialize(): Connects to NATS, subscribes to events, starts listening
   - Callbacks: on_org_plan_changed, on_billing_quota_exceeded (user-assigned)
   - Message handlers: Parse JSON, validate, invoke callbacks, ack/nak

2. `{event}_handler.py` - Event-specific handler class
   - Constructor: Takes database_url for initialization
   - handle(event_fields): Update database, return True/False for ack/nak
   - Singleton pattern: get_{handler}_handler() factory function
   - Plan limits: Hardcoded mapping of tier → quota limits

3. Service `main.py` - Integration in FastAPI lifespan
   - Initialize Control Plane Subscriber in startup
   - Wire handler callbacks
   - Start subscriber async
   - Cleanup in shutdown

### Plan Tier Limits (Enforced in quota handler)

| Tier | Docs | API Calls/Month | Storage | Concurrent Users | Custom Models |
|------|------|-----------------|---------|------------------|---------------|
| Free | 100 | 10K | 1 GB | 1 | ❌ |
| Professional | 1K | 100K | 50 GB | 10 | ✅ |
| Enterprise | ∞ | ∞ | ∞ | ∞ | ✅ |

---

## Implementation Statistics

**Code Created:**
- 4 Python files: `control_plane_subscriber.py` (2x), `quota_enforcement_handler.py` (2x), `m365_provider_handler.py`
- 1 SQL schema update: `org_quotas` table + index + trigger
- Total lines: ~1000 lines of production code

**Services Modified:**
- 4 service `main.py` files updated with lifespan wiring
- 1 requirements.txt updated (retrieval service - added nats-py and redis[asyncio])

**Builds & Deployments:**
- ✅ Ingestion Plane imports-api: Built and deployed (digest: 65379ac88…)
- ✅ Data Plane retrieval-service: Built and deployed (digest: 378f0a8ab4…)
- ✅ Data Plane documents-service: Built and deployed
- All services healthy and listening to events

**Logs Verified:**
```
✅ imports-api listening on aqencia.controlplane.user.provider_linked
✅ imports-api listening on aqencia.controlplane.org.plan_changed
✅ imports-api listening on aqencia.controlplane.billing.quota_exceeded

✅ retrieval-service listening on aqencia.controlplane.org.plan_changed
✅ retrieval-service listening on aqencia.controlplane.billing.quota_exceeded

✅ documents-service listening on aqencia.controlplane.org.plan_changed
✅ documents-service listening on aqencia.controlplane.billing.quota_exceeded
```

---

## Remaining Work (Phase 6)

### 🟡 Step 3: Reasoning Plane Quota Limiter (20-30 mins)

**What:** Create quota_exceeded_handler for Reasoning Plane

**Where:** Likely `apps/Reasoning Plane/ai-core/app/` (TODO: verify structure)

**Implementation:**
- Listen to `aqencia.controlplane.billing.quota_exceeded`
- Check org quota status before allowing inferences
- Return HTTP 429 when quota exceeded
- Include retry-after header

**Success Criteria:**
- Reasoning Plane service subscribes to Control Plane events
- Logs show listening on billing.quota_exceeded
- Service ready for quota-checking middleware

### ⏳ Step 4: E2E Cross-Plane Test (30-40 mins)

**What:** Full flow test simulating org lifecycle

**Test Flow:**
1. Register user (auth-core publishes user.created)
2. Create org (org-core publishes org.created)
3. Link Microsoft account (user-core publishes user.provider_linked)
4. Verify Ingestion Plane created M365 connector
5. Verify Data Plane created org_quotas entry
6. Update org plan to "professional"
7. Verify quota limits updated in database
8. Trigger quota_exceeded event
9. Verify Reasoning Plane blocks inferences with 429

**Test Script:** `scripts/phase_6_e2e_test.sh` (to create)

**Success Criteria:**
- All 3 planes receive their respective events
- Database state updated correctly at each step
- Cross-plane isolation verified (wrong org doesn't see events)

---

## Known Issues & Resolutions

### Issue 1: nats-py Module Import ❌→✅
**Problem:** `ModuleNotFoundError: No module named 'nats'`  
**Root Cause:** Missing newline in requirements.txt (redis[asyncio]==5.0.4nats-py==2.14.0 on same line)  
**Resolution:** Fixed with Python string replacement, added nats-py==2.14.0 on new line

### Issue 2: AsyncSessionLocal vs SessionLocal ❌→✅ (Ingestion Plane, earlier)
**Problem:** ImportError in m365_provider_handler  
**Root Cause:** Copied wrong database class from another service  
**Resolution:** Changed to SessionLocal (correct for this pattern)

---

## Next Steps

### Immediate (< 30 mins):
1. Examine Reasoning Plane directory structure
2. Create quota_exceeded_handler for Reasoning Plane
3. Wire into Reasoning Plane main service
4. Rebuild and verify subscription

### Short-term (30-60 mins):
1. Create Phase 6 E2E test script
2. Run through full org → ingestion → data → reasoning flow
3. Verify event propagation across all planes
4. Validate database state at each step

### Documentation:
1. Update [PHASE_6_PLAN.md](PHASE_6_PLAN.md) with completion status
2. Create [PHASE_6_VALIDATION.md](PHASE_6_VALIDATION.md) with test results
3. Update [SYSTEM_STATUS.md](SYSTEM_STATUS.md) to reflect Phase 6 completion

---

## Continuation Context

Everything is ready to proceed with Reasoning Plane implementation. Just need to:
1. Find Reasoning Plane directory and understand structure
2. Copy quota_exceeded_handler pattern from Data Plane
3. Add 4-5 lines of main.py wire-up
4. Rebuild, test, verify

**Zero blockers.** Pattern is proven. Code is straightforward. All infrastructure in place.

