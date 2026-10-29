# Convex Core Service

> **Verified 2026-07-11 (Application Plane audit).** This file was written as a pre-build *proposal* ("Status: Ready for Implementation", "2-3 weeks", "replaces Gateway Service"). convex-core is now **live and deployed**: `convex-backend` answers on `:3210` (HTTP 200), containers have been up for days, and 20+ Convex function modules ship in `convex/`. Read the sections below as historical design intent; the corrections here plus the linked sources are authoritative for current runtime.
> - **Frontend is Verevon v3 (SolidJS/Vite), not Next.js/React.** The `convex/react`, `ConvexReactClient`, `NEXT_PUBLIC_CONVEX_URL`, and `useQuery`/`useMutation` examples reflect an abandoned Next.js plan.
> - **The "Functions" inventory is wrong and incomplete.** `ai:generateResponse` exists, but there is **no `convex/jobs.ts` and no `convex/rag.ts`** — so the `jobs:*` / `rag:indexDocuments` functions and the `/webhooks/rag/*` + `/webhooks/job/*` HTTP actions in `http.ts` call a missing `api.jobs.*` module and are broken/dead. The actually-live surfaces are undocumented here: HTTP routes `/ingest/session`, `/ingest/session/message`, `/ingest/control-session`, and `/api/webhook/nats/{handler}`, plus modules `controlSessions`, `agentRuns`, `conversationProjection`, `plannerDocuments`, `knowledgeQnA`, `organizations`, `users`, and `nats`.
> - **Service hostnames in "Environment Variables" are stale.** The deployed plane compose points Convex at `model-gateway:8080` (`AI_CORE_URL`/`MODEL_GATEWAY_URL`) and `org-core:8080`; `ai-core:8000`, `org-core-service:8080`, and `auth-service:3001` are legacy aliases being phased out (see `verevon-gap.md` G7).
> - Current sources of truth: `apps/Application Plane/docs/core-research/convex-core.md` and `apps/Application Plane/APPLICATION_PLANE_DEEP_DIVE.md`.

**Purpose**: Reactive UI state layer for CoreSystem (Application Plane)

Convex Core (self-hosted) provides real-time synchronization between backend services and frontend, acting as a **reactive mirror** of canonical backend state:
- Real-time WebSocket gateway for UI updates (subscriptions, live progress, presence)
- Conversation state (chat messages, typing indicators)
- Live crawl/job progress updates
- UI projections (non-authoritative, temporary caches)
- Session-scoped application state

⚠️ **IMPORTANT**: Convex is NOT the source of truth. Backend services (Postgres, etc.) are canonical.

## Architecture Overview

```
User
  ↓
Frontend (Next.js)
  ↓ (JWT from Auth Service - VALIDATED)
Convex Core (Application Plane)  ← Reactive UI state mirror + real-time sync
  ├─→ Validates with auth-service (JWT, org membership)
  ├─→ Validates with org-core (capabilities)
  └─→ Subscribes to backend events
  ↓
Backend Services (Source of Truth)
  ├─→ AI Core (reasoning, LLM orchestration)
  ├─→ Org Core (org metadata, quotas, capabilities)
  ├─→ Document Service (canonical document storage)
  └─→ Crawler Service (canonical crawl state)
  ↓
Canonical Data Stores (Postgres, S3, Qdrant)
```

## Why Convex Core?

### Real-Time UI Synchronization
Instead of polling or complex WebSocket management, Convex provides:

✅ **Realtime Subscriptions** - Built-in reactive queries that auto-update clients  
✅ **State Management** - TypeScript functions with transactional guarantees  
✅ **HTTP Actions** - Inbound webhooks and external API calls  
✅ **Orchestration** - Actions that call AI Core / Org Core with retry logic  
✅ **File Storage** - Built-in file uploads  
✅ **Auth Integration** - JWT verification built-in  

### Key Features

1. **Reactive Database**
   - TypeScript queries/mutations with strong typing
   - Automatic subscriptions - clients get live updates
   - Transactional consistency

2. **Real-time Everything**
   - Chat messages stream to all participants instantly
   - Job progress updates broadcast to frontend
   - Presence indicators (who's typing, online status)

3. **Orchestration Actions**
   - Server-side functions that call external services
   - Built-in retry logic and error handling
   - Can call AI Core HTTP endpoints

4. **HTTP Actions**
   - Webhooks from Org Core when RAG jobs complete
   - Callbacks from AI Core with streaming chunks
   - External integrations (Slack, Discord, etc.)

## Deployment Options

### Option 1: Docker Compose (Development)
```bash
cd backend/convex-gateway
docker compose up -d
```

### Option 2: Fly.io (Production)
```bash
fly launch --config fly.toml
fly deploy
```

### Option 3: Railway (Production)
One-click deploy with Railway template

### Option 4: Self-Hosted (Production)
Deploy to your own infrastructure with PostgreSQL backend

## Data Flow

### 1. Chat Message Flow
```
User types message in Next.js frontend
  ↓
Frontend calls convex.mutation("messages:send", { content })
  ↓
Convex stores message in messages table
  ↓
Convex triggers action("ai:generateResponse")
  ↓
Action calls AI Core: POST /chat with orgID + message
  ↓
AI Core calls Org Core: gRPC GetContext(orgID, query)
  ↓
Org Core retrieves from Postgres + Qdrant → returns facts
  ↓
AI Core generates answer, streams chunks back to Convex
  ↓
Convex persists chunks, broadcasts to all subscribers
  ↓
Frontend receives live updates via reactive query
```

### 2. RAG Indexing Job Flow
```
User uploads documents in frontend
  ↓
Frontend calls convex.action("jobs:createRAGJob", { documents })
  ↓
Convex creates job record with status="pending"
  ↓
Action calls Org Core: POST /api/v1/rag/documents/async
  ↓
Org Core creates Asynq job, returns job_id
  ↓
Convex stores job_id, polls for updates
  ↓
Org Core Worker processes job, sends webhook to Convex
  ↓
Convex HTTP Action receives webhook, updates job status
  ↓
Frontend gets live update via reactive query
  ↓
Shows progress bar updating in real-time
```

## Schema

Convex uses TypeScript schema definitions in `convex/schema.ts`:

```typescript
// conversations, messages, sessions, jobs, etc.
```

## Functions

### Queries (read data, automatically subscribe)
- `conversations:list` - Get all conversations for org
- `messages:get` - Get messages for conversation
- `jobs:status` - Get job status with progress

### Mutations (write data, transactional)
- `conversations:create` - Create new conversation
- `messages:send` - Send message to conversation
- `jobs:updateStatus` - Update job progress

### Actions (call external services)
- `ai:generateResponse` - Call AI Core for answer
- `rag:indexDocuments` - Call Org Core RAG indexing
- `jobs:pollStatus` - Poll Org Core for job updates

### HTTP Actions (inbound webhooks)
- `webhooks:ragComplete` - Receive RAG job completion
- `webhooks:jobProgress` - Receive job progress updates
- `webhooks:aiCallback` - Receive AI streaming chunks

## Environment Variables

```bash
# Convex Self-Hosted
CONVEX_SELF_HOSTED_URL=http://localhost:3210
CONVEX_SELF_HOSTED_ADMIN_KEY=<generated-key>

# Auth Integration
AUTH_SERVER_URL=http://auth-service:3001
JWT_SECRET=<your-jwt-secret>

# AI Core Integration
AI_CORE_URL=http://ai-core:8000
AI_CORE_API_KEY=<internal-service-key>

# Org Core Integration
ORG_CORE_URL=http://org-core-service:8080
ORG_CORE_API_KEY=<internal-service-key>

# Database (for production)
DATABASE_URL=postgresql://user:pass@host:5432/convex
```

## Frontend Integration

### Next.js Setup
```typescript
// lib/convex.ts
import { ConvexProvider, ConvexReactClient } from "convex/react";

const convex = new ConvexReactClient(process.env.NEXT_PUBLIC_CONVEX_URL!);

export function ConvexClientProvider({ children }: { children: React.ReactNode }) {
  return <ConvexProvider client={convex}>{children}</ConvexProvider>;
}
```

### Using Queries
```typescript
// components/ChatMessages.tsx
import { useQuery } from "convex/react";
import { api } from "@/convex/_generated/api";

export function ChatMessages({ conversationId }: { conversationId: string }) {
  // Automatically subscribes to updates!
  const messages = useQuery(api.messages.get, { conversationId });
  
  return (
    <div>
      {messages?.map(msg => (
        <div key={msg._id}>
          <strong>{msg.role}:</strong> {msg.content}
        </div>
      ))}
    </div>
  );
}
```

### Using Mutations
```typescript
// components/ChatInput.tsx
import { useMutation } from "convex/react";
import { api } from "@/convex/_generated/api";

export function ChatInput({ conversationId }: { conversationId: string }) {
  const sendMessage = useMutation(api.messages.send);
  
  const handleSend = async (content: string) => {
    await sendMessage({ conversationId, content });
    // UI updates automatically!
  };
  
  return <input onSubmit={handleSend} />;
}
```

## Benefits Over Custom Gateway

### Development Speed
- ✅ No need to build WebSocket hub from scratch
- ✅ No manual subscription management
- ✅ No state synchronization logic
- ✅ Built-in TypeScript types

### Reliability
- ✅ Transactional consistency out of the box
- ✅ Automatic retries for actions
- ✅ Connection recovery handled automatically
- ✅ Battle-tested by thousands of apps

### Performance
- ✅ Optimized for realtime updates
- ✅ Efficient delta updates (only changes sent)
- ✅ Built-in caching
- ✅ Scales horizontally

### Developer Experience
- ✅ Write backend in TypeScript
- ✅ Automatic API generation
- ✅ Type-safe frontend/backend
- ✅ Hot reload in development

## Migration Path

### Phase 1: Parallel Deployment (Week 1)
- Deploy Convex alongside existing WebSocket hub
- Implement chat messages in Convex
- Frontend uses Convex for new chats
- Old chats still use WebSocket

### Phase 2: Feature Parity (Week 2-3)
- Migrate job status to Convex
- Migrate session management to Convex
- Implement all webhooks in Convex HTTP Actions
- Test both systems in parallel

### Phase 3: Full Migration (Week 4)
- Switch all frontend traffic to Convex
- Deprecate old WebSocket hub
- Remove custom state management code
- Monitor and optimize

## Monitoring

### Convex Dashboard
- Real-time function logs
- Query performance metrics
- Database size and growth
- Error tracking

### External Monitoring
- Prometheus metrics export
- Grafana dashboards
- Alert on slow queries
- Track subscription count

## Cost Estimate

### Self-Hosted (Free)
- $0 - OSS software
- Infrastructure: ~$50-100/month (compute + storage)
- PostgreSQL: ~$20-50/month (optional, can use SQLite)

### Cloud-Hosted (if you prefer managed)
- Free tier: 500K function calls/month
- Pro: $25/month for 5M calls
- Enterprise: Custom pricing

**Recommendation**: Start with self-hosted Docker, migrate to Fly.io for production.

## Next Steps

1. ✅ Review this README
2. [ ] Set up local Convex with Docker Compose
3. [ ] Define schema for conversations, messages, jobs
4. [ ] Implement queries/mutations for chat
5. [ ] Create actions to call AI Core / Org Core
6. [ ] Set up HTTP Actions for webhooks
7. [ ] Integrate with Next.js frontend
8. [ ] Test end-to-end chat flow
9. [ ] Deploy to Fly.io for production

## References

- [Convex Documentation](https://docs.convex.dev/)
- [Self-Hosting Guide](https://github.com/get-convex/convex-backend/blob/main/self-hosted/README.md)
- [Convex Auth](https://labs.convex.dev/auth)
- [Discord Support](https://discord.gg/convex) - #self-hosted channel

---

**Status**: Live and deployed (this proposal-era doc predates the build — see the Verified 2026-07-11 banner at top)
**Priority**: 🔥 Critical
**Team**: 1-2 developers
