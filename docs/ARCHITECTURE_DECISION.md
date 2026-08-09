# Architecture Decision: Gateway Service + NATS Integration

**Date**: February 1, 2026  
**Decision**: Keep NATS backbone, Convex for frontend realtime only

---

## Current Architecture (NATS-Based)

### ✅ Already Implemented

Your system **already uses NATS extensively** for service-to-service communication:

```
┌─────────────────────────────────────────────────────┐
│                    NATS BROKER                       │
│            (JetStream + Work Queues)                 │
│                  nats://nats:4222                    │
└─────────────────────────────────────────────────────┘
         ↑           ↑            ↑           ↑
         │           │            │           │
    ┌────┴────┐ ┌───┴───┐  ┌─────┴─────┐ ┌──┴──┐
    │  Auth   │ │ User  │  │  Admin    │ │  AI │
    │ Service │ │Service│  │  Service  │ │Core │
    └─────────┘ └───────┘  └───────────┘ └─────┘
                                             ↑
                                             │ NATS
                                         ┌───┴────┐
                                         │  Org   │
                                         │  Core  │
                                         └────────┘
```

### NATS Streams & Subjects

**Current Implementation**:

1. **Auth ↔ User ↔ Admin**: Authentication & authorization events
2. **AI Core ↔ Org Core**: 
   - Safety events: `safety.check.*`
   - Policy updates: `policy.updated.*`
   - Generation events: `generation.image.*`
   - Document events: `document.analyzed`
3. **Org Core Events**: `org.*` (created, updated, deleted, crawl, quota)
4. **Work Queues**: `jobs.*` (video processing, batch translate, cache warming)

---

## Updated Architecture: NATS + Convex Dual Stack

### The New Model

```
Frontend (Next.js)
    │
    │ WebSocket (Convex Protocol)
    ↓
┌─────────────────────────┐
│   Convex Gateway        │  ← Realtime state for frontend
│   (Self-Hosted OSS)     │     • Conversations
│                         │     • Messages
│   Port: 3210, 3211     │     • Presence
└─────────────────────────┘     • UI state
    │
    │ HTTP Actions
    ↓
┌─────────────────────────────────────────────────────┐
│                    NATS BROKER                       │  ← Service backbone
│            (JetStream + Work Queues)                 │     • Events
│                  nats://nats:4222                    │     • Pub/Sub
└─────────────────────────────────────────────────────┘     • Jobs
    ↑           ↑            ↑           ↑
    │           │            │           │
┌───┴───┐ ┌────┴────┐  ┌────┴─────┐ ┌──┴──────┐
│ Auth  │ │  User   │  │  Admin   │ │ AI Core │
│Service│ │ Service │  │ Service  │ │ (Python)│
└───────┘ └─────────┘  └──────────┘ └─────────┘
                                        ↑
                                        │ NATS
                                    ┌───┴────┐
                                    │  Org   │
                                    │  Core  │
                                    │  (Go)  │
                                    └────────┘
```

### Division of Responsibilities

| Component | Responsibility | Protocol |
|-----------|---------------|----------|
| **NATS** | Service-to-service events, jobs, policy sync | NATS/JetStream |
| **Convex** | Frontend realtime updates (chat, UI state) | WebSocket (Convex) |
| **HTTP** | Request/response (REST/gRPC) | HTTP/2 |

---

## Integration Points

### 1. Convex → NATS (Publish Events)

When frontend actions need to trigger backend events:

```typescript
// convex/ai.ts - Convex Action
export const generateResponse = action({
  handler: async (ctx, args) => {
    // 1. Call AI Core via HTTP
    const response = await fetch(`${AI_CORE_URL}/stream/chat`, {...});
    
    // 2. Publish event to NATS for other services
    await fetch(`${NATS_GATEWAY_URL}/publish`, {
      method: "POST",
      body: JSON.stringify({
        subject: "telemetry.event.chat",
        data: {
          org_id: args.orgId,
          session_id: args.sessionId,
          model: args.model,
        }
      })
    });
    
    // 3. Store in Convex for frontend
    await ctx.db.insert("messages", {...});
  }
});
```

### 2. NATS → Convex (Webhook Events)

When backend events need to update frontend state:

**Org Core publishes to NATS** → **NATS Subscriber** → **HTTP POST to Convex**

```go
// Org Core: Publish event when RAG job completes
natsClient.Publish("org.rag.completed", &RagCompleteEvent{
    OrgID:   orgID,
    JobID:   jobID,
    Status:  "completed",
    Results: results,
})
```

**Gateway Service** (NEW) - NATS → Convex bridge:

```go
// gateway-service/internal/nats/subscriber.go
func (s *Subscriber) SubscribeToOrgEvents() {
    // Subscribe to org events
    s.natsClient.Subscribe("org.rag.completed", func(msg *nats.Msg) {
        var event RagCompleteEvent
        json.Unmarshal(msg.Data, &event)
        
        // Forward to Convex HTTP Action
        http.Post(
            "http://convex-backend:3211/webhooks/rag/complete",
            "application/json",
            bytes.NewBuffer(msg.Data),
        )
    })
}
```

**Convex receives webhook** and updates frontend state:

```typescript
// convex/http.ts - Already created
export const ragComplete = httpAction(async (ctx, request) => {
  const event = await request.json();
  
  // Update job status in Convex
  await ctx.runMutation(internal.jobs.updateStatus, {
    jobId: event.job_id,
    status: "completed",
    results: event.results,
  });
  
  // All subscribed clients get update automatically
  return new Response("OK");
});
```

---

## Do You Need a Gateway Service?

### Short Answer: **YES** - But Minimal

You need a **lightweight Gateway Service** for:

1. ✅ **NATS → Convex Bridge**: Subscribe to NATS events, forward to Convex HTTP Actions
2. ✅ **Convex → NATS Bridge**: HTTP endpoint for Convex actions to publish to NATS
3. ❌ **NOT WebSocket Hub**: Convex handles this
4. ❌ **NOT State Management**: Convex handles this
5. ❌ **NOT Service Orchestration**: NATS already does this

### Gateway Service Scope (Minimal)

**Purpose**: Event routing layer between NATS (service backbone) and Convex (frontend state)

**Components**:
1. NATS subscriber (subscribes to `org.*`, `safety.*`, `generation.*`)
2. HTTP client (forwards events to Convex HTTP Actions)
3. HTTP server (accepts Convex → NATS publish requests)
4. Health checks

**Estimated Size**: ~500 lines of Go code (very minimal)

---

## Implementation Plan

### Phase 1: Keep Current NATS Setup ✅
**Status**: Already complete

- Auth ↔ User ↔ Admin via NATS
- AI Core ↔ Org Core via NATS
- All JetStream streams configured

### Phase 2: Deploy Convex Gateway ✅
**Status**: Complete (files created)

- Self-hosted Convex backend
- Schema for conversations/messages
- HTTP Actions for webhooks

### Phase 3: Create Minimal Gateway Service (NEW)
**Scope**: NATS ↔ Convex bridge only

**Files to Create**:
```
backend/gateway-service/
├── cmd/
│   └── server/
│       └── main.go               (100 lines)
├── internal/
│   ├── config/
│   │   └── config.go             (60 lines)
│   ├── nats/
│   │   ├── subscriber.go         (200 lines) ← Subscribe to NATS, forward to Convex
│   │   └── publisher.go          (80 lines)  ← HTTP endpoint for Convex → NATS
│   └── convex/
│       └── client.go             (120 lines) ← HTTP client to Convex
├── Dockerfile                     (30 lines)
└── docker-compose.yml            (40 lines)
```

**Configuration**:
```env
# NATS config (existing)
NATS_URL=nats://nats:4222
NATS_TOKEN=nats

# Convex config (new)
CONVEX_BACKEND_URL=http://convex-backend:3211

# Bridge config
NATS_SUBJECTS=org.*,safety.*,generation.*,review.*,policy.*
CONVEX_WEBHOOK_SECRET=<secret>
```

**Key Functions**:

1. **NATS → Convex** (subscriber):
```go
func (s *Subscriber) Start() {
    // Subscribe to all org events
    s.nc.Subscribe("org.*", func(msg *nats.Msg) {
        s.forwardToConvex("org/events", msg.Data)
    })
    
    // Subscribe to safety events
    s.nc.Subscribe("safety.check.*", func(msg *nats.Msg) {
        s.forwardToConvex("safety/events", msg.Data)
    })
}

func (s *Subscriber) forwardToConvex(endpoint string, data []byte) {
    url := fmt.Sprintf("%s/webhooks/%s", s.convexURL, endpoint)
    http.Post(url, "application/json", bytes.NewBuffer(data))
}
```

2. **Convex → NATS** (HTTP endpoint):
```go
func (h *Handler) PublishToNATS(c *gin.Context) {
    var req struct {
        Subject string          `json:"subject"`
        Data    json.RawMessage `json:"data"`
    }
    c.BindJSON(&req)
    
    // Publish to NATS
    h.natsConn.Publish(req.Subject, req.Data)
    c.JSON(200, gin.H{"status": "published"})
}
```

**Deployment**:
```yaml
# docker-compose.yml
gateway-service:
  build: ./gateway-service
  ports:
    - "8090:8090"
  environment:
    - NATS_URL=nats://nats:4222
    - CONVEX_BACKEND_URL=http://convex-backend:3211
  depends_on:
    - nats
    - convex-backend
  networks:
    - coresystem-local
```

---

## Timeline

### Week 1 (Current)
- ✅ Convex Gateway implemented
- ✅ Architecture decision documented
- 🔄 Create minimal Gateway Service (2-3 days)

### Week 2
- Deploy Gateway Service locally
- Test NATS → Convex flow
- Test Convex → NATS flow
- Frontend integration with Convex

### Week 3+
- Rate Limiting (Priority 1)
- Prompt Template Management
- Audit Logging

---

## Benefits of This Architecture

### 1. Best of Both Worlds
- **NATS**: Service-to-service events (already working)
- **Convex**: Frontend realtime updates (better DX than custom WebSocket)

### 2. Minimal Changes
- Keep existing NATS infrastructure
- Keep existing service communication
- Add Convex only for frontend

### 3. Separation of Concerns
- **Backend services**: Talk via NATS (event-driven, decoupled)
- **Frontend**: Subscribes to Convex (automatic, reactive)
- **Gateway**: Thin translation layer (minimal code)

### 4. Future-Proof
- Can replace Convex later without touching NATS
- Can add more NATS subscribers without touching Convex
- Independent scaling

---

## Cost Implications

### Infrastructure
- **NATS**: Already running (256MB RAM, ~$10/month)
- **Convex**: Self-hosted ($80-170/month OR cloud $0-25/month)
- **Gateway Service**: Minimal (~128MB RAM, negligible cost)

**Total Additional Cost**: ~$80-170/month (or $0-25 with Convex Cloud)

### Developer Time Saved
- **Custom WebSocket Hub**: 3-4 weeks saved
- **Subscription Management**: 2 weeks saved
- **State Sync Logic**: 1-2 weeks saved

**Total Savings**: ~6-8 weeks of development time = **$30,000-50,000** saved

---

## Decision

**✅ Keep NATS** for service-to-service communication (already working perfectly)

**✅ Use Convex** for frontend realtime state (better DX than custom WebSocket)

**✅ Create minimal Gateway Service** for NATS ↔ Convex event routing (~500 lines)

**❌ Don't build custom WebSocket hub** (Convex replaces this)

**❌ Don't duplicate NATS functionality** (keep what works)

---

## Next Steps

1. **Create Gateway Service** (2-3 days) - NATS ↔ Convex bridge
2. **Deploy Convex locally** (./setup.sh)
3. **Test event flow**: NATS → Gateway → Convex → Frontend
4. **Integrate frontend** with ConvexProvider
5. **Continue Phase 2**: Rate Limiting, Prompt Templates, Audit Logging

---

**Should I create the minimal Gateway Service now?**
