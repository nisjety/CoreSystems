# Convex Integration: The Real-time Application State Plane

> **Verified 2026-07-11 (Application Plane audit, Phase 5).** The conceptual guidance below (the Golden Rule, allowed/forbidden domains, event-flow mental model) is still accurate and authoritative. Several concrete operational specifics are stale — corrections are inlined and consolidated in the *2026-07-11 Verification addendum* at the end. Highlights: the `onOrganizationMemberRemoved` handler is **referenced but not defined** (would throw); the real host ports are `convex-backend :3210/:3211`, `convex-gateway :3006`, `convex-dashboard :6791` (there is **no** `localhost:3000` listener); and the `aquatiq-*` / `AI_CORE_URL=ai-core:8000` names in the examples are legacy.

## Overview & The Golden Rule

> **Convex stores INTERACTION, not KNOWLEDGE.**

Convex is the **LIVE EXPERIENCE** layer for CoreSystem. It exists to solve one problem only: **humans collaborating on AI work in real time.**

Convex is **NOT**:
- A source of truth
- A backend logic owner
- A RAG system
- An identity system
- A workflow engine

Convex turns backend events (from NATS) into **instant collaborative UX**. It strictly mirrors state, but it never owns authoritative core domain data.

## ✅ CONVEX USAGE DIRECTIVE

### Responsibilities
Convex SHALL:
1. Maintain realtime collaborative workspace state (chat, presence, live cursors).
2. Mirror backend domain events received via NATS (`document.indexed`, `org.member.added`).
3. Provide sub-100ms reactive updates to frontend clients.
4. Store conversational and interaction data only.
5. Enable multi-user collaboration across organizations.

### Allowed Convex Domains ✅
- Conversations
- Messages
- Comments & annotations
- Live Job progress (Scraping, chunking)
- Real-time Notifications
- Presence (who is viewing what)
- Workspace state / UI filters
- Draft prompts

### Forbidden Domains ❌
- Authentication (Control Plane owns this)
- RAG storage (Data Plane owns this)
- Vector data (Qdrant owns this)
- Documents & Files (Data Plane owns this)
- Billing data (Control Plane owns this)
- Core Organization metadata authority (Control Plane owns this)

## Architecture Mental Model

```text
Postgres  → Truth
Qdrant    → Knowledge
AI-Core   → Thinking
Temporal  → Execution
NATS      → Events
Convex    → LIVE EXPERIENCE
```

### Event Flow

Convex operates entirely asynchronously regarding authoritative data. It never queries heavy backend services. It simply listens.

```text
CONTROL PLANE / DATA PLANE
        ↓ (Domain Events)
      NATS
        ↓ (realtime sync)
APPLICATION PLANE (Convex Subscriber)
        ↓ (mutation)
    Convex Database
        ↓ (subscription)
    Frontend UI
```

## Integration Points

### 1. The Interaction Layer: Conversations & Messages (Owned by Convex)

Convex is the authoritative store for **real-time chat and collaboration**.
- Users create conversations within their Organization.
- Conversations provide multi-player isolation per organization.
- Subscriptions (`useQuery("chat:getMessages")`) provide instant updates for all team members (Slack-level collaboration automatically).

### 2. State Mirrors: Organizations & Users (Strictly Read-Only)

Synced from auth-core when `organization.created` event is published:
```
External (auth-core) → Convex
├── orgId (auth-core ID) → externalOrgId
├── name → name
├── slug → slug
└── createdAt → createdAt
```

**Index**: by `externalOrgId` for fast lookups

### 2. Users

Synced from org-core when `organization.member.added` event is published:
```
External (auth-core/org-core) → Convex
├── userId (auth-core ID) → externalAuthId
├── email → email
├── name → name
├── orgId (external) → (lookup to find convexOrgId)
├── role → role
└── createdAt → createdAt
```

**Indexes**: 
- by `externalAuthId`
- by `externalAuthId + orgId` (for membership verification)

### 3. Conversations

Created by users in Convex:
- User creates conversation in their organization
- Conversation references both Convex user ID and org ID
- Conversations are isolated per organization (multi-tenant)

### 4. Messages

Messages in a conversation:
- Reference the conversation
- Reference the user (both Convex user ID and external auth ID optional for audit)
- Support streaming, attachments, and metadata

## NATS Event Subscriptions

### Subscribed Topics

```
organization.created      → onOrganizationCreated(orgId, name, slug, createdAt)
organization.updated      → onOrganizationUpdated(orgId, name, slug, settings)
organization.deleted      → onOrganizationDeleted(orgId)
organization.member.added → onOrganizationMemberAdded(orgId, userId, email, role)
organization.member.removed → onOrganizationMemberRemoved(orgId, userId)
```

> **Stale (verified 2026-07-11):** `onOrganizationMemberRemoved` is dispatched from `convex/http.ts` (`internal.nats.onOrganizationMemberRemoved`) but **has no matching export in `convex/nats.ts`** (only `onOrganizationCreated`, `onOrganizationUpdated`, `onOrganizationDeleted`, and `onOrganizationMemberAdded` exist). An `organization.member.removed` event would throw at runtime. Treat this topic as unimplemented until the handler is added.

### Subscription Durability

Each topic has a durable subscriber named `convex-<topic>`. This ensures:
- Messages aren't lost if Convex is temporarily down
- Redelivery of missed events when Convex restarts
- At-least-once delivery semantics

## Running Convex with NATS Integration

### Docker Compose

Add to your `docker-compose.yml`:
```yaml
convex-gateway:
  build:
    context: ./convex-gateway
    dockerfile: Dockerfile
  container_name: convex-gateway
  env_file:
    - ./convex-gateway/.env.local
  ports:
    - "3000:3000"   # Convex HTTP API
  networks:
    - aquatiq-local
  depends_on:
    aquatiq-nats-local:
      condition: service_healthy
    auth-core:
      condition: service_healthy
    org-core:
      condition: service_healthy
```

### Environment Variables

**Required** in `.env.local`:
```
NATS_URL=nats://aquatiq-nats-local:4222
NATS_TOKEN=nats
NATS_SERVICE_NAME=convex-gateway
```

**Optional** (for NATS subscriber service):
```
CONVEX_BACKEND_URL=http://convex-gateway:3000
CONVEX_API_KEY=dev-key
```

### Starting

```bash
# Build and start all services
docker compose up -d

# Check Convex is running (verified 2026-07-11 — real host ports, NOT :3000)
curl http://localhost:3210/version   # convex-backend API
# convex-gateway is published on host :3006 (→ container :3000)

# View Convex dashboard
# Open http://localhost:6791 in browser
```

> **Stale (verified 2026-07-11):** the `3000:3000` / `curl http://localhost:3000` guidance above and in the compose snippet does not match the live stack. Real published host ports: `convex-backend :3210` (API) + `:3211` (HTTP actions), `convex-gateway :3006`, `convex-dashboard :6791`, `convex-subscriber` (no published port). The example service/network names (`aquatiq-nats-local`, network `aquatiq-local`) are legacy; the live compose uses network `app-net`.

## Data Isolation

### Clear Boundaries

Organizations and users are **read-only** in Convex:
- Cannot be created/edited directly in Convex
- Only synced via NATS events
- Always reference external IDs for audit

Conversations and messages are **writable** in Convex:
- Created by authenticated users via Convex API
- Belong to a specific organization
- Permanently retain references to user/org external IDs

### Query Examples

**List organizations**:
```typescript
const orgs = await ctx.db.query("organizations").collect();
```

**Get users in organization**:
```typescript
const users = await ctx.db
  .query("users")
  .filter((q) => q.eq(q.field("orgId"), orgId))
  .collect();
```

**List conversations for user**:
```typescript
const conversations = await ctx.db
  .query("conversations")
  .filter((q) => q.eq(q.field("userId"), userId))
  .collect();
```

## Security

### Service-to-Service Auth

Convex authenticates with other services via:
1. **NATS Token**: Configured in env for subscribing to events
2. **Internal Service ID**: For gRPC calls to auth-core (if needed)
3. **JWTs**: For user/session verification

### User Context

Users accessing Convex should:
1. Authenticate with auth-core first
2. Receive a JWT/session token
3. Pass token to Convex API calls
4. Convex validates user belongs to their organization

### Cross-Organization Isolation

Queries automatically scope to user's organization:
```typescript
if (user.orgId !== conversationOrgId) {
  throw new Error("Unauthorized");
}
```

## Common Operations

### Syncing a New Organization

1. User creates org in auth-core: `POST /api/v2/auth/organization/create`
2. auth-core publishes: `NATS: organization.created event`
3. Convex NATS subscriber receives event
4. `onOrganizationCreated` mutation is called
5. Organization appears in Convex within seconds

### Adding User to Organization

1. Org admin invites user: `POST /auth/organization/invite-member`
2. User accepts invitation
3. org-core publishes: `NATS: organization.member.added event`
4. Convex NATS subscriber receives event
5. `onOrganizationMemberAdded` mutation is called
6. User can now create conversations in Convex

### Querying with External IDs

For audit or verification, you may need to find Convex records by external IDs:

```typescript
// Get organization by auth-core ID
const org = await ctx.db
  .query("organizations")
  .filter((q) => q.eq(q.field("externalOrgId"), authOrgId))
  .first();

// Get user by auth-core user ID
const user = await ctx.db
  .query("users")
  .filter((q) => q.eq(q.field("externalAuthId"), authUserId))
  .first();
```

## Troubleshooting

### Convex not syncing orgs/users

1. Check NATS connection: `docker logs aquatiq-nats-local`
2. Check Convex logs: `docker logs convex-gateway`
3. Verify NATS_URL and NATS_TOKEN in `.env.local`
4. Restart Convex: `docker compose restart convex-gateway`

### Users can't see conversations

1. Verify user was added to organization (check Convex dashboard)
2. Check user's `orgId` matches conversation's `orgId`
3. Query user record: 
   ```typescript
   const user = await ctx.db.get(userId);
   // Should have orgId and syncStatus: "synced"
   ```

### Missing organizations after startup

1. Convex uses durable NATS subscriptions
2. Should automatically catch up on startup
3. If not, manually create org again to trigger event
4. Check durable subscriber status: `nats stream info CONTROL_PLANE_EVENTS`

## Development

### Local Testing

```bash
# Start just Convex dev mode
npm run dev

# Start with NATS subscriber
npm run dev &
node nats-subscriber.js
```

### Schema Changes

After updating `convex/schema.ts`:
```bash
# Regenerate types
npm run generate

# Convex handles migrations automatically
```

### Testing NATS Integration

Send test event via NATS CLI:
```bash
nats pub organization.created --json '{
  "id": "test-org-1",
  "name": "Test Org",
  "slug": "test-org",
  "createdAt": 1234567890
}'
```

Then check Convex dashboard for synced organization.

## Production Considerations

- Use Convex Cloud instead of local SQLite
- Implement retry logic with exponential backoff
- Add monitoring/alerting for sync failures
- Use stronger NATS authentication (mTLS)
- Implement rate limiting for API calls
- Regular backups of Convex database
- Document data retention policies

## Related Documentation

- [NATS Integration Guide](./NATS_INTEGRATION.md)
- [Auth Core Documentation](../auth-core/docs/auth-plan.md)
- [Organization Core Documentation](../org-core/README.md)
- [Convex Official Docs](https://convex.dev)

---

## 2026-05-20 — Verevon Build Runtime Audit

Source: `apps/Application Plane/docker-compose.yml` + `build-verevon-services.sh`.

### Observed services & ports

| Container | Host port → Container | Role |
|---|---|---|
| `convex-backend` | 3210 → 3210 (API), 3211 → 3211 (HTTP actions) | Rust + SQLite; real-time sync |
| `convex-dashboard` | 6791 → 6791 | UI (app-net only — **NOT on inter-plane-bus**) |
| `convex-gateway` | 3005 → 3000 | Node.js gateway, runs `npx convex dev` (app-net only) |
| `convex-subscriber` | — | NATS subscriber — dual-connects `verevon-nats:4222` (shared) + `model-plane-nats-1:4222` (model isolated) |
| `affine-runtime` | 47810 → 3010 | self-hosted AFFiNE |
| `affine-runtime-migration` | one-shot | DB migration, gated `service_completed_successfully` |
| `notification-core` | 3140 → 3140 | first-party Novu-compatible boundary |
| `application-postgres` | 9540 → 5432 | notifications + AFFiNE DB |
| `application-redis` | 6480 → 6379 | 256 MB cap |

### Bootstrap one-shots
- `affine-runtime-migration` — removed post-exit-0.

### Build script post-hook
- `deploy_convex_functions` (index 4) — runs `npx convex deploy` against `convex-backend:3210`. Idempotent. Belt-and-suspenders with `convex-gateway`'s startup.sh which also runs `convex deploy` on every cold start (mitigation for §10 SQLite-registry drift).

### Cross-plane wiring — stale defaults still present
| Compose env (in `convex-backend`) | Value | Real container |
|---|---|---|
| `ORG_CORE_URL` | `http://org-core-service:8080` | should be `http://org-core:8080` |
| `AUTH_SERVER_URL` | `http://auth-service:3011` | should be `http://auth-core:3011` |
| `AI_CORE_URL` | `http://ai-core:8000` | Model Plane gateway is `model-gateway:8080` on `model-plane-network` (NOT inter-plane-bus); cross-plane name resolution fails |

### Convex function deploy verification
- `CONVEX_SELF_HOSTED_ADMIN_KEY` read from `apps/Application Plane/convex-core/.env.local` by the post-hook.
- Build did not reach Application Plane in the 2026-05-20 run (blocked by Model Plane Go build failures upstream); SQLite registry consistency not yet verified.

### Remediation
1. Fix the env defaults inside `convex-backend` in `apps/Application Plane/docker-compose.yml` (rename `auth-service` → `auth-core`, `org-core-service` → `org-core`).
2. Add `convex-dashboard` and `convex-gateway` to `inter-plane-bus` if verevon server-side routes need to talk to them.
3. Address Model Plane network isolation so `AI_CORE_URL` resolves.

---

## 2026-07-11 — Verification addendum (Application Plane audit, Phase 5)

Re-verified against current `convex/` source, `apps/Application Plane/convex-core/docker-compose.yml`, live host curl, and `docker ps`/`docker inspect`. Docker exec/build/logs were unavailable this pass (containerd content store corrupted), so container internals are graded `[inspect]`/`[source-only]`; published-port reachability is `[live-curl]`.

- **Cross-plane env defaults — partially remediated.** Remediation #1 from the 2026-05-20 section is **done**: `ORG_CORE_URL` now defaults to `http://org-core:8080` and `AUTH_SERVER_URL` to `http://auth-core:3011`. **Still stale:** `AI_CORE_URL` defaults to `http://ai-core:8000`; the live Model Plane reasoning entry is `model-gateway:8080` (on `model-plane-network`, not the Application bus), so the "AI-Core → Thinking" mental model and any `ai-core` reference should read `model-gateway`. [source-only]
- **Gateway host port drift.** The 2026-05-20 table said `convex-gateway 3005 → 3000`; live `docker ps` shows **`3006 → 3000`**. `convex-backend` publishes `3210`/`3211`, `convex-dashboard` `6791`. All four containers show `(unhealthy)` due to exec-based healthchecks failing under the corrupted Docker runtime, not because the services are down — `:3210/version` returns 200 live. [live-curl] [inspect]
- **`onOrganizationMemberRemoved` undefined.** Confirmed: `convex/http.ts` dispatches it but `convex/nats.ts` never exports it. The `organization.member.removed` NATS subscription is effectively non-functional (throws). [source-only]
- **Internal service-key hardcoded default — removed.** A prior note claimed the `X-Service-Key` validator fell back to a hardcoded `"change-me-internal-service-secret"`. That is **no longer true**: `convex/authz.ts`, `convex/ingest.ts`, and `convex/controlSessions.ts` validate against `CONVEX_INTERNAL_SERVICE_KEY || INTERNAL_API_KEY` and **throw if unset** (fail-closed). Outbound callers (`convex/ai.ts`, `convex/nats.ts`) fall back to `""`, which the receiving validator rejects. [source-only]
- **Broken `api.jobs.*` webhooks (out of scope for this doc, noted for accuracy).** `convex/http.ts` `ragComplete`/`jobProgress` call `api.jobs.getByExternalId/updateStatus/updateProgress`, but no `convex/jobs.ts` module exists — those webhook paths are dead. `verifyWebhookSignature()` also returns `true` when `WEBHOOK_SECRET` is unset and only checks a `sha256=` prefix. See `apps/Application Plane/docs/core-research/convex-core.md`. [source-only]
- **Source of truth.** For current runtime shape, prefer `apps/Application Plane/docs/core-research/convex-core.md` and `apps/Application Plane/APPLICATION_PLANE_DEEP_DIVE.md` over this integration guide's operational specifics.

