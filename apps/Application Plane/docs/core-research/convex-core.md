# convex-core

> **2026-07-13 superseding update.** The July 11 missing-function findings are fixed in source, not live: `onOrganizationMemberRemoved` now tombstones/revokes all duplicate memberships and matching Control Sessions; `imports.recordCompleted` exists; dead `api.jobs.*` HTTP callers were removed; called-function contract tests, membership authorization, durable JetStream consumption, redacted DLQ, and removal/demotion-only reconciliation were added. The dedicated reconciliation endpoint uses HMAC, timestamp, nonce replay claims, tenant scope, and exact apply confirmation. Tests/typecheck/lint are green (28 tests). The running bundle/subscriber is still old and no production reconciliation was applied. Control events still lack signed issuer identity, immutable event ID/revision, transactional publication, and narrow NATS ACLs, so deployment remains blocked. Treat the rest of this file as July 11 historical evidence where it conflicts with this paragraph.

_Audited 2026-07-11. Evidence grades: **[live-curl]** = verified against a running port; **[source-only]** = read from disk; **[inspect]** = `docker ps`/`docker inspect` config/state. Docker `exec`/`build`/`logs` are unavailable this pass (containerd content-store corruption), so container-internal behavior is graded [source-only]/[inspect]._

## Current State

`convex-core` is the Application Plane's reactive projection / subscription layer. It is **non-authoritative** — it mirrors Control/Model/Ingestion Plane state so the Verevon v3 frontend can subscribe reactively. It is **live** on the backend + HTTP-actions surface; the separate `convex-gateway` container's own HTTP port is dead but that does not break projections (see below).

Four containers:

| Container | Host port | State [inspect] | Live check |
|---|---|---|---|
| `convex-backend` | 3210 (API), 3211 (HTTP actions) | Up 2d, `(unhealthy)` | :3210 `/`=200, `/version`=200; :3211 `/webhooks/health`=200 **[live-curl]** |
| `convex-gateway` | 3006 → :3000 | Up 2d, `(unhealthy)` | every path = empty reply / HTTP 000 **[live-curl]** |
| `convex-subscriber` | (none) | Up 2d, `(unhealthy)` | `node nats-subscriber.js`; not host-reachable |
| `convex-dashboard` | 6791 | Up 2d, no healthcheck | :6791 `/`=500 **[live-curl]** |

The `(unhealthy)` flags are the **exec-based healthchecks failing** (containerd corruption + the backend image lacking `curl` for its `curl -f .../version` probe), not dead services — the backend answers fine over host curl. [live-curl]+[inspect]

### The `:3006` gateway is a deployer sidecar, not a request surface
`convex-gateway` builds from `./Dockerfile` and runs `startup.sh` → `npx convex dev`, bind-mounting `./convex` so function edits hot-push into `convex-backend` without an image rebuild (docker-compose.yml:122-131). Its container port 3000 (host 3006) does not serve usable HTTP — **all paths return empty-reply** (`/`, `/version`, `/webhooks/health`, `/api` all HTTP 000). [live-curl] Its job is pushing functions to the backend; the functional surface the frontend actually uses is **`convex-backend` :3210 (queries/WS) and :3211 (HTTP actions)**. So `:3006` having no health path is expected — there is no real path to discover.

## Entry Points

- Compose: `apps/Application Plane/convex-core/docker-compose.yml`
- Functions: `apps/Application Plane/convex-core/convex/*.ts`
- HTTP actions router: `convex/http.ts` (served on backend :3211)
- Standalone NATS subscriber: `convex-core/nats-subscriber.js` (the real subscriber process)
- Deploy sidecar: `Dockerfile` + `startup.sh` (`npx convex dev`)

## Exposed Surface (HTTP actions, backend :3211)

- `GET /webhooks/health` — 200 `{"status":"healthy","service":"convex-gateway"}` **[live-curl]**
- `POST /ingest/session`, `POST /ingest/session/message` — session-core mirror; `X-Service-Key` fail-closed (ingest.ts)
- `POST /ingest/control-session` — session-core Refresh snapshot mirror
- `POST /api/webhook/nats/{handler}` — dispatcher the subscriber posts to (see nats map below)
- `POST /webhooks/rag/complete`, `POST /webhooks/job/progress`, `POST /webhooks/ai/stream` — **broken/legacy** (see finding 2)

## NATS projection map (nats-subscriber.js → :3211 /api/webhook/nats/…)

Working [source-only]: `org.{created,updated,deleted}`, `org.member.added`, `crawl.{started,progress,completed,failed,indexed}` (direct `ingestJobs` writes), `mp.v1.run.*.event` → `onAgentRunEvent` (needs `MODEL_PLANE_NATS_URL` for the 2nd connection, else silently skipped), `onConversationEvent` → `conversationProjection.applyEvent`.

Broken: `org.member.removed` (finding 1), `import.completed` (finding 3).

## Stub, Mock, Placeholder, and Breakage Audit

1. **`onOrganizationMemberRemoved` is called but never defined — CONFIRMED [live-curl].**
   `http.ts:276` (`case "onOrganizationMemberRemoved"` → `ctx.runAction(internal.nats.onOrganizationMemberRemoved,…)`) and `nats-subscriber.js:308` both invoke it; `verevon.controlplane.org.member.removed` is actively subscribed (`nats-subscriber.js:121`). No `export const onOrganizationMemberRemoved` exists anywhere in `convex/` (nats.ts defines up to `onOrganizationMemberAdded`/`onImportCompleted` only). Live probe:
   `POST :3211/api/webhook/nats/onOrganizationMemberRemoved` → **HTTP 500 `{"error":"Couldn't resolve api.nats.onOrganizationMemberRemoved"}`**.
   Impact: member removals in the Control Plane never propagate to the Convex projection (subscriber catches the error and continues) → stale membership mirror. The prior Phase-1 finding is **still open**.

2. **`api.jobs.*` references a missing module — CONFIRMED [source-only].**
   `http.ts` webhooks `ragComplete`/`jobProgress` call `api.jobs.getByExternalId`, `updateStatus`, `updateProgress`. No `convex/jobs.ts` exists (a `jobs` table is defined in `schema.ts:125` but is orphaned — no function reads it). Because `_generated/api.js` is `anyApi`, the reference resolves at ref-time but fails at call-time exactly like finding 1. `/webhooks/rag/complete` and `/webhooks/job/progress` are dead legacy (Org-Core/AI-Core RAG-job webhooks); not wired to any current caller.

3. **`api.imports.recordCompleted` references a missing module — NEW/CONFIRMED [source-only].**
   `nats.ts:264` (`onImportCompleted`, subject `verevon.ingestion.import.completed`) calls `api.imports.recordCompleted`. No `convex/imports.ts` module and no `imports` table. For an org that exists the handler fails at that call (for a missing org it returns `org_not_found` first). Crawl-lifecycle projection works (uses `ingestJobs`); the discrete `import.completed` event does not.

4. **`verifyWebhookSignature` is a stub — [source-only].**
   `http.ts:172-183`: returns `true` when `WEBHOOK_SECRET` is unset; when set, only checks `signature.startsWith("sha256=")` — no HMAC. Genuine security stub. Low live impact (the webhooks it guards are already broken via finding 2), but real if those are ever restored. Distinct from the `X-Service-Key` ingest path, which is properly fail-closed.

5. **`startSubscriber` placeholder — dead code [source-only].**
   `nats.ts:31` `internalAction`, comment "placeholder for the NATS subscriber logic", never referenced. Overlaps the real `nats-subscriber.js`. Harmless.

6. **Type safety is defeated for the above — [source-only].**
   `tsc --noEmit` passes clean (exit 0). `tsconfig.json` has `strict:false`, `noImplicitAny:false`, `skipLibCheck:true`, plus `convex-stubs.d.ts` typing `convex/*` as `any` and `_generated/api.js`=`anyApi`. So the compiler does **not** catch the missing-module / missing-function references in findings 1-3 — they only surface at runtime.

## Fixed since prior audits

7. **Hardcoded `"change-me-internal-service-secret"` default — REMEDIATED [source-only].**
   grep of the whole service finds it nowhere. `authz.ts:getExpectedServiceKey()` and `ingest.ts:getServiceKey()` both **fail closed** (throw if `CONVEX_INTERNAL_SERVICE_KEY`/`INTERNAL_API_KEY` unset). `nats.ts`/`ai.ts` use an empty-string fallback (rejected by the receiving validator). Landed in commit `49dc5720` "fix(security): harden internal-auth — kill weak defaults + close fail-open holes" (2026-07-07). The prior key-split finding is stale.

## Drift

8. **`AI_CORE_URL=http://ai-core:8000` is stale but dead — [source-only].**
   `ai.ts:58,178` (`generateResponse`, `queryWithContext`) read `process.env.AI_CORE_URL` and POST to `/stream/chat`/`/chat`. Default `ai-core:8000` appears in `docker-compose.yml:37`, `.env.local:26`, `startup.sh:73`, `DEPLOYMENT.md:76`, `README.md:179` (real target is `model-gateway:8080`). **However `ai.ts`'s actions are not referenced anywhere in the convex tree** — Convex is a projection layer and v3 chat runs directly through model-gateway, so `ai.ts` is legacy Convex-as-chat-backend code and inert. The stale URL is misleading, not load-bearing.

9. **`DEPLOYMENT.md` stale service names — [source-only].** Lines 76-77 point at `ai-core:8000` and `org-core-service:8080` (renamed to `org-core` per the `.env.local` comment).

10. **Orphaned schema tables — [source-only].** `jobs` (schema.ts:125) and `webhooks` (schema.ts:199) tables have no owning function module.

## Uncommitted WIP

None. `git status --porcelain` and `git diff --stat` for `apps/Application Plane/convex-core/` are both empty. Last commit touching the dir: `49dc5720 fix(security): harden internal-auth` (2026-07-07). [source-only]

## Build / toolchain

- `node v25.8.2`; `node_modules` present. `node_modules/.bin/tsc --noEmit` → exit 0, clean. [source-only]
- No Go/Rust here — pure Convex/TypeScript + a Node subscriber. Convex codegen/`convex dev` needs the convex binary + backend (via the gateway sidecar), not run this pass.

## Notes

The service is genuinely live and broader than the Jun-07 doc claimed, and the headline security bug (hardcoded internal-service secret) is fixed. Two projection paths remain broken at runtime — `org.member.removed` (finding 1, live-confirmed 500) and `import.completed` (finding 3) — plus the legacy job/RAG webhooks (finding 2). All three are hidden from the type-checker by the loose tsconfig + `anyApi` (finding 6), so they will keep failing silently until the missing modules/functions (`jobs.ts`, `imports.ts`, `nats.onOrganizationMemberRemoved`) are added. The `:3006` gateway is a `convex dev` deployer sidecar, not a request endpoint — its dead HTTP port is by design and not the projection surface.
