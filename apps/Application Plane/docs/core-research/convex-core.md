# convex-core

## Current State

`convex-core` is the Application Plane's reactive projection and subscription surface. It is live, non-authoritative, and wired into the default plane compose through:

- `convex-backend`
- `convex-dashboard`
- `convex-gateway`
- `convex-subscriber`

It stores frontend-facing projection state for organizations, users, control sessions, agent runs, conversations, planner documents, and knowledge Q&A.

## Entry Points

- Compose runtime: `apps/Application Plane/docker-compose.yml`
- Convex functions: `apps/Application Plane/convex-core/convex/`
- External event subscriber: `apps/Application Plane/convex-core/nats-subscriber.js`
- HTTP actions: `apps/Application Plane/convex-core/convex/http.ts`

## Relationships

- Mirrors Control Plane session snapshots into Convex through `/ingest/control-session`.
- Mirrors Model Plane run lifecycle into Convex for frontend subscriptions.
- Mirrors conversation events into Convex through `/ingest/session` and `/ingest/session/message`.
- Exposed directly to frontend BFF code through `convex-backend:3210`.

## Stub, Mock, Placeholder, and Breakage Audit

1. `convex/http.ts`
   - `verifyWebhookSignature()` skips verification entirely when `WEBHOOK_SECRET` is unset.
   - When set, it still only checks for a `sha256=` prefix rather than validating an HMAC.

2. `convex/http.ts`
   - Webhook handlers call `api.jobs.getByExternalId`, `api.jobs.updateStatus`, and `api.jobs.updateProgress`.
   - No `convex/jobs.ts` module is present in the current tree.
   - Those webhook paths are therefore stale, broken, or dependent on missing/generated artifacts not present in-repo.

3. `convex/nats.ts`
   - `startSubscriber` is explicitly placeholder logic.
   - Actual subscriber runtime behavior lives in `nats-subscriber.js`, which means there are two overlapping subscriber stories and only one is live.

4. Service naming drift
   - `README.md`, `DEPLOYMENT.md`, `docker-compose.yml`, and `startup.sh` still refer to `AI_CORE_URL=http://ai-core:8000` in places.
   - Main plane compose now points Convex traffic at `model-gateway:8080`.

## Redundancy and Drift

- `docker-compose.ui.yml` duplicates the dashboard exposure already in the main compose.
- The docs still describe older service names and older Model Plane assumptions.
- The placeholder subscriber action overlaps with the real `nats-subscriber.js` process.

## Notes

`convex-core` should be kept as a projection layer, but its webhook and integration docs need cleanup before it can be treated as trustworthy operational documentation.
