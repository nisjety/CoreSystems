# APPLICATION PLANE Overview

**Last Updated:** February 19, 2026  
**Purpose:** Product-facing experience layer and real-time UI orchestration

## Placement In The Pyramid

The Application Plane sits **above Control and Data consumers** and **below the Frontend Plane**.

It is an **optional layer**, not a mandatory backend foundation:

- Control Plane remains the authority root.
- Data Plane remains the canonical product data layer.
- Ingestion Plane and Model Plane v2 can operate without Application Plane.
- Frontend applications may compose Application Plane services when a product needs collaboration, notifications, projections, or app-specific workflows.

---

## Core Principle

> **"Product experience and real-time UI orchestration"**

The APPLICATION PLANE provides **product-facing experiences and real-time UI orchestration** - it is never the source of truth, but always a reactive composition of backend state tailored for frontend consumption.

### Key Characteristics

✅ **Reactive, not authoritative**  
✅ **Validates every request with Control Plane**  
✅ **Aggregates from multiple backend planes**  
✅ **Optimized for frontend UX (low latency, real-time)**  
✅ **Ephemeral state only (caches, sessions, projections)**  
❌ **NEVER stores canonical business data**  
❌ **NEVER makes policy or business logic decisions**  
❌ **NEVER trusts orgId from frontend without validation**  

---

## Services in APPLICATION PLANE

### 1. convex-core ✅
**Purpose:** Real-time state synchronization and conversation backbone

**Owns:**
- Chat messages and typing indicators
- UI state projections (reactive mirrors)
- Live progress updates (crawls, imports)
- Session-scoped caches

**Use Cases:**
- Real-time chat and collaboration
- Live crawl/import progress bars
- Typing indicators and presence
- Multi-tab UI state sync

**Technology:** Self-hosted Convex (WebSocket, reactive queries)

---

### 2. product-bff (Backend for Frontend) ✅
**Purpose:** API aggregation and transformation for frontend clients

**Owns:**
- Product-specific API composition logic
- UI-optimized response transformations
- Client-specific caching

**Use Cases:**
- Single API call aggregates 5 backend services
- Transform backend responses to UI-friendly formats
- Mobile, web, desktop have different BFF endpoints
- Pagination, filtering, sorting optimized for each client

**Technology:** Node.js/TypeScript (GraphQL or REST)

---

### 3. session-orchestrator ✅
**Purpose:** Active session lifecycle management

**Owns:**
- Active session state (in-memory/Redis)
- Multi-tab coordination
- Session-scoped feature flags

**Use Cases:**
- Manage user session lifecycle (create, refresh, invalidate)
- Coordinate state across browser tabs
- Implement session timeout and renewal
- Track session health and heartbeat

**Technology:** Redis + Node.js

---

### 4. feature-composer ✅
**Purpose:** Feature flag evaluation and A/B test assignment

**Owns:**
- Feature flag evaluation logic
- A/B test variant assignment
- Client-side configuration cache

**Use Cases:**
- Evaluate feature flags per user/org
- Assign A/B test variants consistently
- Compose feature sets based on entitlements
- Provide `/api/features` endpoint for frontend

**Technology:** LaunchDarkly SDK or custom (Redis cache)

---

### 5. realtime-streaming-layer ✅
**Purpose:** WebSocket multiplexing and stream management

**Owns:**
- WebSocket connection pool
- Stream multiplexing logic
- Server-Sent Events (SSE) channels

**Use Cases:**
- Stream LLM responses from ai-core to frontend
- Multiplex multiple backend streams to single client
- Handle connection lifecycle and reconnection
- Implement backpressure and flow control

**Technology:** Socket.IO or native WebSocket (Node.js)

---

### 6. notification-service ✅
**Purpose:** Multi-channel notification delivery orchestration

**Owns:**
- Notification delivery state (sent/pending/failed)
- User notification preferences
- Notification templates

**Use Cases:**
- Route notifications to email, push, in-app
- Batch and deduplicate notifications
- Respect user notification preferences (opt-out)
- Track delivery status (sent, read, dismissed)

**Technology:** Bull (job queue) + SendGrid/Twilio/Firebase

---

### 7. usage-projections ✅
**Purpose:** Real-time usage tracking and quota forecasting

**Owns:**
- Real-time usage counters (Redis)
- Projected quota consumption trends
- Usage dashboard cache

**Use Cases:**
- Display live usage dashboards in UI
- Alert users when approaching quota limits
- Project cost estimates based on current usage
- Provide `/api/usage/current` and `/api/usage/forecast`

**Technology:** Redis (counters) + TimescaleDB (historical trends)

---

## Architecture Boundaries

### What APPLICATION PLANE DOES ✅

1. **Aggregates** backend service responses for frontend
2. **Transforms** data to UI-friendly formats
3. **Caches** aggregated responses (short TTL)
4. **Validates** JWT and org membership on every request
5. **Subscribes** to backend events for real-time updates
6. **Provides** WebSocket/SSE streams to frontend
7. **Evaluates** feature flags and A/B tests
8. **Orchestrates** session lifecycle
9. **Delivers** notifications to multiple channels
10. **Projects** usage trends and quota forecasts

### What APPLICATION PLANE DOES NOT ❌

1. ❌ Store canonical business data (Data Plane owns)
2. ❌ Make business logic decisions (backend planes decide)
3. ❌ Define quotas or entitlements (Control Plane owns)
4. ❌ Execute AI/RAG operations (Reasoning/Data Planes)
5. ❌ Bypass authentication/authorization (always validates)
6. ❌ Trust orgId from frontend without validation
7. ❌ Own permanent state (ephemeral only)

---

## Security Model

### Every APPLICATION PLANE service MUST:

✅ **Validate JWT** on ALL incoming requests  
✅ **Check org membership** via auth-service  
✅ **Verify capabilities** via org-core  
✅ **Respect quota limits** from org-service  
✅ **Log sensitive operations** to audit-service  

### Every APPLICATION PLANE service MUST NOT:

❌ **Trust orgId from frontend** without validation  
❌ **Bypass backend validation** for performance  
❌ **Cache security decisions** longer than TTL  
❌ **Expose internal service URLs** to frontend  
❌ **Return unfiltered backend errors** to clients  

---

## Data Flow

### Typical Request Flow

```
Frontend (user clicks "Search")
    ↓ (JWT in Authorization header)
product-bff (validates JWT, aggregates)
    ↓
    ├─→ auth-service (verify JWT, get user context)
    ├─→ org-service (check quotas, entitlements)
    ├─→ rag-service (execute semantic search)
    ├─→ synthesis-service (generate answer)
    └─→ usage-projections (increment counter)
    ↓
product-bff (transforms response, caches)
    ↓ (JSON response)
Frontend (displays results)
    ↓
convex-core (stores query in chat history)
    ↓ (WebSocket event)
Frontend (updates chat UI in real-time)
```

### Real-Time Update Flow

```
Backend (crawler finishes page)
    ↓ (publishes crawl.page_crawled event)
convex-core (receives event, updates state)
    ↓ (reactive query triggers)
Frontend (progress bar updates automatically)
```

---

## Technology Stack

| Service | Primary Tech | State Storage | Protocol |
|---------|-------------|---------------|----------|
| convex-core | Self-hosted Convex | SQLite/Postgres | WebSocket |
| product-bff | Node.js/TypeScript | Redis (cache) | REST/GraphQL |
| session-orchestrator | Node.js + Redis | Redis | HTTP |
| feature-composer | Node.js | Redis (cache) | HTTP |
| realtime-streaming-layer | Socket.IO | In-memory | WebSocket/SSE |
| notification-service | Node.js + Bull | Postgres | NATS (events) |
| usage-projections | Node.js + Redis | Redis + TimescaleDB | HTTP |

---

## Deployment Strategy

### Development
```bash
# All APPLICATION PLANE services run locally
cd apps/Application\ Plane/convex-core && npm run dev
cd apps/Application\ Plane/product-bff && npm run dev
cd apps/Application\ Plane/session-orchestrator && npm run dev
# ... etc
```

### Production

**Horizontal Scaling:**
- `product-bff`: 3+ instances (load balanced)
- `realtime-streaming-layer`: 2+ instances (sticky sessions)
- `notification-service`: 2+ background workers
- `convex-core`: Self-hosted (stateful, managed replicas)

**Caching:**
- Redis cluster for distributed caching
- TTL: 1-5 minutes for aggregated responses
- Invalidate on backend events

**Rate Limiting:**
- Per user: 100 req/min
- Per org: 1000 req/min
- Per API key: 500 req/min

---

## Monitoring & Observability

### Key Metrics

1. **Latency:**
   - P95 response time < 100ms (cache hit)
   - P95 response time < 500ms (cache miss)

2. **Throughput:**
   - Requests per second per service
   - WebSocket connections (active)

3. **Cache Hit Rate:**
   - Target: >80% for product-bff

4. **Event Processing:**
   - convex-core event lag < 100ms
   - notification-service delivery rate

5. **Error Rates:**
   - 4xx errors < 1%
   - 5xx errors < 0.1%

### Health Checks

Each service exposes:
- `GET /health` - Liveness probe
- `GET /ready` - Readiness probe (checks dependencies)
- `GET /metrics` - Prometheus metrics

---

## Future Enhancements

### Planned (Phase 3)
- [ ] GraphQL Federation (product-bff becomes GraphQL gateway)
- [ ] Edge caching with Cloudflare Workers
- [ ] Offline-first support with local-first sync
- [ ] Advanced A/B testing with Statsig

### Considered
- [ ] Mobile-specific BFF for iOS/Android
- [ ] Desktop app BFF for Electron
- [ ] WebAssembly for client-side feature evaluation
- [ ] AI-powered notification batching and prioritization

---

## Related Documentation

- [BOUNDARY_VALIDATION.md](./BOUNDARY_VALIDATION.md) - Full multi-plane architecture
- [CONVEX_CORE_MIGRATION.md](./CONVEX_CORE_MIGRATION.md) - Convex Core setup
- [ARCHITECTURE.md](./ARCHITECTURE.md) - System architecture overview
- [CONTROL_REASONING_PLANES_QUICK_REF.md](./CONTROL_REASONING_PLANES_QUICK_REF.md) - Other planes

---

**Status:** ✅ **Specification Complete**  
**Implementation Status:** 🔨 **In Progress** (convex-core done, others planned)  
**Next Steps:** Implement product-bff and session-orchestrator  
**Owner:** Platform Team
