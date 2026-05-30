# Phase 6: Cross-Plane Event Consumption & Validation

**Phase Status:** 🟡 **STARTING**  
**Date Started:** March 1, 2026  
**Prerequisite:** Phase 5 Complete ✅

---

## 🎯 Phase 6 Objectives

This phase implements **event consumption** across downstream planes to create a fully integrated, event-driven system:

1. ✅ **Build subscriber infrastructure** in Ingestion, Data, and Reasoning planes
2. ✅ **Wire subscribers to shared NATS** with proper configuration
3. ✅ **Implement business logic handlers** for each event type
4. ✅ **Create cross-plane E2E tests** validating end-to-end flows
5. ✅ **Validate tenant isolation** and event routing

---

## 📋 Phase 6 Scope

### Subscriber Architecture

```
Control Plane (Events)          Shared NATS               Downstream Planes (Subscribers)
═════════════════════════       ════════════              ══════════════════════════════

auth-core
  ↓ user.registered            ┌──────────────┐
user-core                       │              │
  ↓ user.provider_linked ────→  │ AQENCIA_     │  ───→  Ingestion Plane
  ↓ user.updated       ────→    │ CONTROLPLANE │        (Setup M365 integration)
  ↓ user.deleted       ────→    │              │
                                │              │         Data Plane
org-core                        │              │  ───→  (Enforce quotas)
  ↓ org.created        ────→    │              │
  ↓ org.updated        ────→    │  JetStream   │         Reasoning Plane
  ↓ org.deleted        ────→    │ (Stream)     │  ───→  (Restrict operations)
  ↓ org.plan_changed   ────→    │              │
  ↓ org.member_*       ────→    │              │
                                │              │
billing-core                    │              │
  ↓ billing.account_*  ────→    │              │
  ↓ billing.quota_*    ────→    │              │
  ↓ billing.invoice_*  ────→    │              │
  ↓ billing.plan_*     ────→    │              │
                                └──────────────┘
```

### Subscribers by Plane

#### 1. Ingestion Plane Subscriber
**Topic:** M365 Graph API Setup  
**Subscribes to:** `aqencia.controlplane.user.provider_linked`  
**Action:** When a user links their Microsoft account, automatically:
- Get M365 tenant ID from event
- Initialize Graph API OAuth token exchange
- Create M365 connector instance
- Trigger initial data sync (calendar, emails, etc.)
- Store connector credentials securely

**Example Event:**
```json
{
  "type": "aqencia.controlplane.user.provider_linked",
  "user_id": "user-123",
  "email": "user@company.com",
  "provider": "microsoft",
  "tenant_id": "tenant-xyz"
}
```

**Expected Action Flow:**
```
user.provider_linked event
  ↓
Check provider == "microsoft"
  ↓
Get tenant_id from event
  ↓
Call M365 API to list user's organizations
  ↓
Create bi-directional sync job
  ↓
Store sync state in database
  ↓
Return success/failure to audit log
```

#### 2. Data Plane Subscriber
**Topic:** Plan-Based Quota Enforcement  
**Subscribes to:** `aqencia.controlplane.org.plan_changed`  
**Action:** When an organization's plan changes:
- Update org's quota limits based on new plan tier
- Apply retrospective quota adjustments if downgrading
- Reset usage counters if upgrading
- Notify org if usage exceeds new limits
- Enforce soft/hard limits in query engine

**Example Event:**
```json
{
  "type": "aqencia.controlplane.org.plan_changed",
  "org_id": "org-456",
  "plan": "professional",
  "tier": "paid",
  "previous_plan": "free",
  "effective_date": "2026-03-01T00:00:00Z"
}
```

**Expected Action Flow:**
```
org.plan_changed event
  ↓
Load org quota config from billing-core
  ↓
Update quota limits:
  - API calls/month
  - Document storage GB
  - Concurrent queries
  - Embedding budget
  ↓
Check current usage vs new limits
  ↓
If downgrade: Apply hard limits
If upgrade: Apply soft limits
  ↓
Store quota update with timestamp
  ↓
Notify org of new limits
```

#### 3. Reasoning Plane Subscriber
**Topic:** Quota Enforcement & Operation Gating  
**Subscribes to:** `aqencia.controlplane.billing.quota_exceeded`  
**Action:** When an organization exceeds quota:
- Check which metric was exceeded (API calls, storage, cost, etc.)
- Throttle/block reasoning operations accordingly
- Return rate-limit headers to client
- Log quota violation
- Suggest plan upgrade to user

**Example Event:**
```json
{
  "type": "aqencia.controlplane.billing.quota_exceeded",
  "org_id": "org-456",
  "metric": "api_calls",
  "limit": 100000,
  "current": 102500,
  "period": "monthly",
  "reset_date": "2026-04-01T00:00:00Z"
}
```

**Expected Action Flow:**
```
quota_exceeded event
  ↓
Parse metric type (api_calls, storage, cost, etc.)
  ↓
Update org's quota status in cache/DB
  ↓
For existing requests:
  - Return 429 Too Many Requests
  - Add Retry-After header
  - Suggest plan upgrade
  ↓
For new requests:
  - Check quota before processing
  - Return error if exceeded
  ↓
Log event to audit trail
```

---

## 🔧 Implementation Pattern

### Standard Subscriber Pattern

All subscribers follow this pattern:

```go
// 1. Define subscription handler interface
type EventHandler interface {
    HandleEvent(ctx context.Context, event *Event) error
}

// 2. Create struct for handler
type M365ProviderLinkedHandler struct {
    m365Service *M365Service
    logger      *log.Logger
}

// 3. Implement event handling logic
func (h *M365ProviderLinkedHandler) HandleEvent(ctx context.Context, event *Event) error {
    // Parse event
    userID := event.Data["user_id"].(string)
    provider := event.Data["provider"].(string)
    tenantID := event.Data["tenant_id"].(string)
    
    // Validate
    if provider != "microsoft" {
        return nil // Ignore non-Microsoft events
    }
    
    // Handle event
    connector, err := h.m365Service.SetupConnector(ctx, userID, tenantID)
    if err != nil {
        return fmt.Errorf("setup failed: %w", err)
    }
    
    // Log success
    h.logger.Printf("M365 connector created for user %s, tenant %s", userID, tenantID)
    return nil
}

// 4. Wire into NATS subscriber
subscriber, err := nats.NewSubscriber(
    natsConn,
    "aqencia.controlplane.user.provider_linked",
    queueGroup,
    func(msg *nats.Msg) {
        var event Event
        json.Unmarshal(msg.Data, &event)
        handler.HandleEvent(ctx, &event)
    },
)
```

### Configuration Pattern

Each subscriber needs:
- **NATS Broker:** `nats://velion-nats:4222`
- **Auth Token:** `aqencia-shared-nats-token-2026`
- **Subject Filter:** `aqencia.controlplane.plane.*`
- **Queue Group:** `{plane}-subscribers` (for load balancing)
- **Consumer Offset:** `nats.StartWithLast()` (get new messages only)

---

## 📂 Files to Create

### Ingestion Plane
- `ingestion/services/m365-provider-handler.go` - Event handler for provider_linked
- `ingestion/nats/m365-subscriber.go` - NATS subscription setup
- `ingestion/config.go` - M365 subscription configuration

### Data Plane
- `data-plane/quota-handler.go` - Event handler for plan_changed
- `data-plane/nats-subscriber.go` - NATS subscription setup
- `data-plane/quota-config.go` - Quota enforcement rules

### Reasoning Plane
- `reasoning/handlers/quota-exceeded-handler.go` - Event handler for quota_exceeded
- `reasoning/nats/subscriber.go` - NATS subscription setup
- `reasoning/middleware/quota-limiter.go` - Quota enforcement middleware

### Tests
- `tests/phase6_e2e_test.sh` - Full end-to-end test
- `tests/cross_plane_integration_test.go` - Integration tests

---

## 🚀 Implementation Order

### Step 1: Ingestion Plane M365 Subscriber (TODAY)
- Create event handler for `user.provider_linked`
- Wire to shared NATS
- Test with manual provider linking
- Verify M365 connector creation

### Step 2: Data Plane Quota Subscriber (NEXT)
- Create event handler for `org.plan_changed`
- Wire quota enforcement rules
- Test plan changes and quota updates
- Verify limits are enforced

### Step 3: Reasoning Plane Limiter (THEN)
- Create event handler for `quota_exceeded`
- Implement request throttling
- Add quota checking middleware
- Test rate limiting

### Step 4: Cross-Plane E2E (FINAL)
- Create test that triggers full flow:
  - Register user → Create org → Link provider → Verify M365 sync
  - Change plan → Verify quotas updated
  - Exceed quota → Verify operations throttled
- Validate all event routing
- Test tenant isolation

---

## 🔄 Event Flow Examples

### Example 1: M365 Onboarding Flow

```
User Action: Links Microsoft Account
  ↓
auth-core publishes user.provider_linked event
  ↓
NATS routes to Ingestion Plane
  ↓
Ingestion Plane handler:
  1. Extract user_id, provider="microsoft", tenant_id
  2. Call M365 API to verify tenant
  3. Create M365 connector in database
  4. Start background sync job
  ↓
M365 data begins syncing to organization
```

### Example 2: Plan Upgrade Flow

```
Billing Action: Org upgrades from Free → Professional
  ↓
billing-core publishes org.plan_changed event
  ↓
NATS routes to Data Plane
  ↓
Data Plane handler:
  1. Load org quota config
  2. Update limits: 1M API calls/month, 100GB storage, 50 concurrent queries
  3. Clear hard limits (was 10K calls)
  4. Notify org of new benefits
  ↓
Org immediately has access to higher quotas
```

### Example 3: Quota Exceeded Flow

```
Data Plane Query: Org reaches 100K API calls (limit=10K for Free plan)
  ↓
Data Plane publishes billing.quota_exceeded event
  ↓
NATS routes to Reasoning Plane
  ↓
Reasoning Plane:
  1. Check quota status in cache
  2. Return 429 Too Many Requests
  3. Add "Upgrade to Professional" suggestion
  ↓
User sees rate limit error with upgrade prompt
```

---

## ✅ Success Criteria

### Ingestion Plane M365 Subscriber
- [ ] Listens to `aqencia.controlplane.user.provider_linked`
- [ ] Extracts user_id, provider, tenant_id from event
- [ ] Calls M365 Graph API to verify tenant
- [ ] Creates M365 connector record
- [ ] Starts background sync
- [ ] Handles errors gracefully
- [ ] Logs all actions for audit trail

### Data Plane Quota Subscriber
- [ ] Listens to `aqencia.controlplane.org.plan_changed`
- [ ] Updates quota limits based on plan
- [ ] Applies changes to quota enforcement rules
- [ ] Notifies org of new limits
- [ ] Handles plan downgrades with soft limits
- [ ] Validates quota limits before queries

### Reasoning Plane Quota Limiter
- [ ] Listens to `aqencia.controlplane.billing.quota_exceeded`
- [ ] Updates quota status in memory/cache
- [ ] Returns 429 for quota-exceeded requests
- [ ] Provides helpful error messages
- [ ] Tracks quota violations for analytics

### Cross-Plane E2E Test
- [ ] User registration → org creation → provider linking all work
- [ ] M365 connector created automatically
- [ ] Plan changes reflected in quota limits
- [ ] Quota enforcement prevents operations
- [ ] All events flow through NATS correctly
- [ ] Tenant isolation prevents data crossing

---

## 📊 Testing Strategy

### Unit Tests
- Event parsing and validation
- Event handler logic
- Quota calculation and enforcement

### Integration Tests
- NATS subscription and message delivery
- Event handler execution
- Database state updates

### E2E Tests
- Full user flow: registration → org → provider linking → billing
- Event routing through all planes
- Quota enforcement end-to-end
- Tenant isolation validation

### Load Tests
- Subscriber can handle event throughput
- Quota checks don't cause bottlenecks
- Concurrent events processed correctly

---

## 📝 Documentation Maintained

Following files will be updated/created:
- `docs/PHASE_6_PLAN.md` - This plan
- `docs/PHASE_6_IMPLEMENTATION_PROGRESS.md` - Progress tracking
- `docs/PHASE_6_EVENT_HANDLERS.md` - Handler implementations
- `docs/PHASE_6_E2E_TESTS.md` - Test documentation
- `docs/CROSS_PLANE_INTEGRATION_ARCHITECTURE.md` - Full system view

---

## 🎯 Next Immediate Steps

1. **Examine Ingestion Plane structure** to understand where to add M365 subscriber
2. **Create M365 provider handler** that responds to `user.provider_linked` events
3. **Wire NATS subscription** in Ingestion Plane service initialization
4. **Test manually** by linking a Microsoft account
5. **Verify M365 connector** is created and syncing

---

## 🔍 Key Files from Phase 5 (Reference)

- `apps/Control Plane/user-core/internal/users/service.go` - Publishes user.provider_linked
- `apps/Control Plane/org-core/internal/org/service.go` - Publishes org.plan_changed
- `apps/Control Plane/billing-core/internal/billing/service.go` - Publishes quota_exceeded
- `docs/PHASE_5_COMPLETION_SUMMARY.md` - Phase 5 recap

---

**Phase 6 Target Duration:** 2-3 hours  
**Current Status:** Planning phase ✅  
**Ready to start implementation:** Yes 🚀
