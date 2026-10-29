# Cross-Plane Shared NATS Publisher Implementation

## Architecture

Each plane operates independently with:
- **Local NATS** (`{plane}-nats`): Intra-plane event bus on `{plane}-net` network
- **Shared NATS** (`verevon-nats`): Cross-plane event broker on `triodelab-net` network

Services publish to **both**:
1. Local NATS for intra-plane subscribers
2. Shared NATS for cross-plane subscribers (if available)

### Network Topology

```
┌─────────────────────────────────────────────────────────────┐
│                      triodelab-net (shared)                 │
│  ┌──────────────────┐                                        │
│  │ verevon-nats      │ ← all planes connect here             │
│  │  (JetStream)     │                                        │
│  │  subjects:       │                                        │
│  │  aqencia.*.>     │                                        │
│  └──────────────────┘                                        │
└─────────────────────────────────────────────────────────────┘
      ↑            ↑           ↑            ↑            ↑
      │            │           │            │            │
┌─────┴────┐ ┌────┴────┐ ┌────┴────┐ ┌────┴────┐ ┌────┴────┐
│ Control  │ │   Data  │ │Ingestion│ │Reasoning│ │   App   │
│  Plane   │ │  Plane  │ │  Plane  │ │  Plane  │ │  Plane  │
└──────────┘ └─────────┘ └─────────┘ └─────────┘ └─────────┘
      via               via              via          via
   triodelab-net    triodelab-net    triodelab-net triodelab-net
```

## Subject Convention

```
aqencia.<plane>.<entity>.<action>

Examples:
  aqencia.controlplane.user.registered    // From Control Plane
  aqencia.controlplane.org.created        // From Control Plane
  aqencia.controlplane.billing.account_updated
  
  aqencia.data.document.ingested          // From Data Plane
  aqencia.data.document.indexed
  aqencia.data.search.executed
  
  aqencia.ingestion.crawl.started         // From Ingestion Plane
  aqencia.ingestion.crawl.completed
  aqencia.ingestion.m365.connected
  
  aqencia.reasoning.query.executed        // From Reasoning Plane
  aqencia.reasoning.insight.generated
  
  aqencia.application.document.added      // From Application Plane
  aqencia.application.conversation.created
```

## JetStream Streams

### AQENCIA_CONTROLPLANE (Priority Topic)
- **Subjects**: `aqencia.controlplane.>`, `aqencia.data.>`, `aqencia.ingestion.m365.>`
- **Purpose**: Cross-plane domain events (user, org, billing, document lifecycle, M365 connections)
- **Retention**: 14 days
- **Max Messages**: 100,000
- **Created by**: Control Plane auth-core / Ingestion Plane integration-core

### AQENCIA_REASONING (Future)
- **Subjects**: `aqencia.reasoning.>`
- **Purpose**: Insight/intelligence events
- **Retention**: 7 days
- **Created by**: Reasoning Plane ai-core

### AQENCIA_FRONTEND (Future)
- **Subjects**: `aqencia.frontend.>`
- **Purpose**: UI-driven events
- **Created by**: Frontend

## Implementation Pattern

### 1. Docker Compose Environment
All service dockerfiles get:
```yaml
environment:
    NATS_SHARED_URL: nats://verevon-nats:4222
  NATS_SHARED_TOKEN: "aqencia-shared-nats-token-2026"
networks:
  - {plane}-net          # Local plane network
  - triodelab-net        # Shared cross-plane network
```

### 2. Language-Specific Implementation

#### Python (Data Plane, Ingestion Plane)
```python
# shared_nats.py
class SharedNatsPublisher:
    def __init__(self, nats_url, nats_token, service_name):
        self.nc = None  # Only initialized if URL non-empty and reachable
    
    async def initialize(self):
        # Graceful degradation: returns False if unavailable
        # No errors thrown; caller continues without publishing
    
    async def publish_document_ingested(self, org_id, document_id, ...):
        await self._publish("aqencia.data.document.ingested", {...})

# In service main.py
shared_nats = SharedNatsPublisher(
    os.getenv("NATS_SHARED_URL", ""),
    os.getenv("NATS_SHARED_TOKEN", "aqencia-token"),
    "data-documents-service",
)
await shared_nats.initialize()  # Safe even if unavailable

# In route handlers
@app.post("/v1/documents")
async def create_document(...):
    doc = db.save(...)
    await shared_nats.publish_document_ingested(org_id, doc.id, doc.title, ...)
    return {...}
```

#### Go (Control Plane, Ingestion Plane – Quarry)
```go
// nats/shared_publisher.go
type SharedPublisher struct {
    conn *nats.Conn
    js   jetstream.JetStream
}

func NewSharedPublisher(sharedURL, token, clientName string) (*SharedPublisher, error) {
    if sharedURL == "" {
        return nil, nil  // Nil, not error — graceful disabling
    }
    nc, _ := nats.Connect(sharedURL, nats.Token(token))
    return &SharedPublisher{conn: nc, js: ...}, nil
}

func (sp *SharedPublisher) PublishOrgCreated(ctx, orgID, name, ...) {
    if sp == nil { return }
    sp.js.PublishAsync("aqencia.controlplane.org.created", {...})
}

// In main.go
sp, _ := nats.NewSharedPublisher(cfg.NATSSharedURL, cfg.NATSSharedToken, "quarry-api")
if sp != nil { defer sp.Close() }
// Pass sp to services or handlers
```

#### TypeScript/Node (Application Plane)
```typescript
// shared-nats.ts
class SharedNatsPublisher {
    nc: NatsConnection | null = null;
    
    async initialize() {
        if (!process.env.NATS_SHARED_URL) return false;
        try {
            this.nc = await connect({
                servers: [process.env.NATS_SHARED_URL],
                token: process.env.NATS_SHARED_TOKEN,
            });
            return true;
        } catch (e) {
            logger.warn("Shared NATS unavailable:", e);
            return false;
        }
    }
    
    publish(subject: string, payload: any) {
        if (!this.nc) return;  // Silently skip if unavailable
        this.nc.publish(subject, JSON.stringify(payload));
    }
}
```

## Planes & Implementation Status

### ✅ Control Plane (DONE)
- **Services**: auth-core (NestJS), user-core (Go), org-core (Go), billing-core (Go)
- **Status**: All connected and publishing to verevon-nats
  - `auth-core`: 6 event types (user registration, org creation, etc.)
  - `user-core`: user.registered published to shared NATS
  - `org-core`: org.created/updated/deleted via SharedPublisher interface
  - `billing-core`: billing.account_updated, invoice_created

### 🔄 Data Plane (IN PROGRESS)
- **Services**: documents, embedding-worker, knowledge-index, retrieval
- **Docker**: ✅ NATS_SHARED env vars added
- **Module**: ✅ `shared_nats.py` created
- **Wiring**: ⏳ Need to integrate into each service's main.py and handlers

### ⏳ Ingestion Plane (NEXT)
- **Services**: integration-core (Python), imports-core (Python), quarry-api (Go)
- **Docker**: ⏳ Add NATS_SHARED env vars
- **Module**: ⏳ Create `shared_nats.py` for Python services, `shared_publisher.go` for Quarry
- **Wiring**: ⏳ Wire into each service

### ⏳ Reasoning Plane (AFTER)
- **Services**: ai-core (Python)
- **Docker**: ⏳ Add NATS_SHARED env vars
- **Module**: ⏳ Create `shared_nats.py`
- **Wiring**: ⏳ Wire queries/insights publishing

### ⏳ Application Plane (AFTER)
- **Services**: convex-core (TypeScript, Convex runtime), convex-subscriber (already exists)
- **Docker**: ⏳ Ensure triodelab-net connectivity
- **Module**: ⏳ Create `shared-nats.ts` or enhance nats-subscriber.js
- **Wiring**: ⏳ Publish conversation/org events from Convex mutations

### ⏳ Frontend (OPTIONAL)
- **Services**: Next.js frontend app
- **Purpose**: Publish workspace/permission events
- **Status**: Can wait until late phase

## Remaining Work

1. **Data Plane**: Wire shared_nats.py into 4 services, rebuild containers
2. **Ingestion Plane**: 
   - Add NATS_SHARED to docker-compose.yml (3 services)
   - Create shared modules for integration-core, imports-core, quarry-api
   - Wire into event handlers
3. **Reasoning Plane**:
   - Add NATS_SHARED to docker-compose.yml
   - Create shared_nats.py
   - Wire into ai-core query/insight handlers
4. **Application Plane**:
   - Ensure convex-core has triodelab-net access
   - Create shared-nats.ts for TypeScript Convex services
   - Wire into conversation/org mutations
5. **Cross-Plane Subscriptions**: Set up listeners on each plane for relevant events
6. **End-to-End Testing**: Verify event flow across all planes

## Testing Strategy

### Phase 1: Control Plane
✅ Already verified — auth-core, user-core, org-core, billing-core all connected

### Phase 2: Data Plane
- [ ] Documents service ingest → publishes aqencia.data.document.ingested
- [ ] Embedding worker → publishes aqencia.data.document.embedded
- [ ] Knowledge index → publishes aqencia.data.document.indexed
- [ ] Retrieval → publishes aqencia.data.search.executed

### Phase 3: Ingestion Plane
- [ ] Quarry crawl completion → publishes aqencia.ingestion.crawl.completed
- [ ] Integration core M365 connection → publishes aqencia.ingestion.m365.connected

### Phase 4: Reasoning Plane
- [ ] AI-Core query → publishes aqencia.reasoning.query.executed
- [ ] AI-Core insight → publishes aqencia.reasoning.insight.generated

### Phase 5: Application Plane
- [ ] Convex user adds document → publishes aqencia.application.document.added

## Consumption Patterns (Future)

Each plane will subscribe to events relevant to its domain:

### Data Plane Listens To
- `aqencia.controlplane.org.created` → ensure org isolation setup
- `aqencia.ingestion.crawl.completed` → trigger knowledge indexing

### Reasoning Plane Listens To
- `aqencia.data.document.indexed` → load indexed docs for queries
- `aqencia.controlplane.user.signed_in` → establish user context

### Application Plane Listens To
- `aqencia.reasoning.insight.generated` → add to conversation
- `aqencia.controlplane.*.created` → sync to Convex

### Control Plane Listens To
- `aqencia.ingestion.m365.connected` → record provider link
- `aqencia.data.search.executed` → billing/usage tracking

---

**Note**: This document establishes the canonical pattern for cross-plane integration. All future services should follow this model.
