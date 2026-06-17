# Application Plane Deep Dive

## Executive Summary

The Application Plane is the app-layer coordination and projection boundary that sits above the lower operational planes. Its current runtime is not just a single collaborative workspace backend. It now includes:

1. `convex-core` as a reactive projection and UI-state mirror.
2. `conversation-core` as a first-party support/conversation API.
3. `conversation-ingest-rs` as a thin ingest shim into conversation-core.
4. `information-core` as an internal information API for weather/traffic/news.
5. `notification-core` as a notification and inbox/feed service.
6. Optional or partial AFFiNE surfaces in compose.
7. `zammad-foundation` as a support-stack foundation package with its own separate compose/runtime path.

This plane currently has two notable characteristics:

- It is more service-rich than the main architecture doc admits.
- Several docs and optional runtime paths still reflect older assumptions, incomplete migrations, or partial integrations.

## Current Runtime Topology

Primary compose file: `apps/Application Plane/docker-compose.yml`

### Active/default compose services

| Service | Host ports | Role |
|---|---:|---|
| `convex-backend` | `3210`, `3211` | Convex backend runtime with mounted app functions and sqlite storage |
| `convex-dashboard` | `6791` | Convex admin UI |
| `convex-gateway` | `3006 -> 3000` | Convex dev gateway / schema push path |
| `convex-subscriber` | worker | NATS subscriber mirroring cross-plane events into Convex |
| `application-postgres` | `9540` | Shared Application Plane Postgres |
| `application-dragonfly` | `6480` | Shared Application Plane Dragonfly cache |
| `conversation-core-go` | `3160` | Support/conversation API |
| `conversation-ingest-rs` | `3161` | Conversation ingest adapter |
| `information-core` | `3190` | Internal weather/traffic/news API |
| `notification-core` | `3140` | Notification boundary service |
| `nats` / `app-nats` | internal | Plane-local NATS bus |

### Optional/profile-gated compose surfaces

| Service | Status |
|---|---|
| `affine-core` | compose profile `affine`, but build context directory is missing on disk |
| `affine-runtime` | compose profile `affine`, image-based runtime |
| `affine-runtime-migration` | compose profile `affine`, one-shot migration helper |

### Separate but adjacent support runtime

| Path | Role |
|---|---|
| `docker-compose.zammad.yml` | Separate Zammad stack |
| `zammad-foundation/` | bootstrap scripts, config examples, checklists, architecture guidance |

## Plane Boundary and Ownership

The Application Plane should own app-facing projections, support workflows, notifications, and interactive workspace state. It should not become the source of truth for foundational plane data.

It should own:

- Reactive app-state mirrors and projections for frontend workflows.
- App-facing conversation/support workflow APIs.
- Notification request, feed, preference, and subscriber management.
- Internal app-support information surfaces used by higher layers.
- Optional app/workspace runtimes such as Convex or AFFiNE when actually wired.

It should not own:

- Identity, org, auth, billing, and canonical session truth. Control Plane owns those.
- Document, embedding, retrieval, graph, or wiki stores. Data Plane owns those.
- Ingestion pipelines and connector acquisition. Ingestion Plane owns those.
- Reasoning, execution, agent orchestration, and model routing. Model Plane owns those.

## Relationship Map

```mermaid
flowchart TD
    FE["Frontend Plane / internal clients"] --> CX["convex-backend"]
    FE --> CC["conversation-core-go"]
    FE --> NC["notification-core"]
    FE --> IC["information-core"]

    CS["convex-subscriber"] --> NATS1["velion-nats / model-plane-nats / app-nats"]
    CS --> CX

    CX --> CP["Control Plane"]
    CX --> MP["Model Plane"]
    CX --> IP["Ingestion Plane"]

    CIR["conversation-ingest-rs"] --> CCG["conversation-core-go"]
    CCG --> PG["application-postgres"]
    CCG --> NATS1

    NC --> PG
    NC --> Dragonfly["application-dragonfly"]
    NC --> Novu["Novu runtime or local stub"]
    NC --> NATS1

    ZF["zammad-foundation / zammad compose"] --> CC
    ZF --> NC
```

## Current Service-by-Service Understanding

## convex-core

`apps/Application Plane/convex-core`

This is the reactive UI-state mirror for the system. It is explicitly non-authoritative.

### What it includes

- Convex schema and TypeScript functions under `convex/`.
- Official `convex-backend` container image with mounted function code.
- `convex-subscriber.js` as an external NATS-to-Convex sync service.
- Projections for organizations, users, conversations, messages, planner documents, control sessions, agent runs, Q&A, and conversation projections.

### What it owns

- UI projections and reactive subscriptions.
- Frontend-facing app/session/workspace state caches.
- Mirrored control-session snapshots.
- Mirrored agent-run lifecycle rows.
- Mirrored conversation inbox/message/AI-action projections.

### What it does not own

- Canonical org/user/session truth.
- Canonical run/execution truth.
- Canonical document truth.

### Important runtime facts

- `agentRuns.ts` mirrors Model Plane run events into Convex for UI subscriptions.
- `controlSessions.ts` mirrors Control Plane aggregated session snapshots into Convex.
- `conversationProjection.ts` stores projected conversation inbox/message/action state from external events.
- `knowledgeQnA.ts` is a first-class org-scoped Q&A entity in Convex, not just transient UI state.

## conversation-core-go

`apps/Application Plane/conversation-core/conversation-core-go`

This is a real first-party support/conversation API, not just an auxiliary library.

### What it includes

- Internal-key-gated HTTP API.
- Postgres-backed repository and migrations.
- Optional NATS publisher.
- Support/conversation service with inbox, queue, messages, notes, assignment, tags, and AI-action review.

### Exposed surface

- `/health`, `/ready`
- `/api/v1/inboxes`
- `/api/v1/inboxes/:id/queue`
- `/api/v1/conversations`
- `/api/v1/conversations/:id`
- `/api/v1/conversations/search`
- `/api/v1/conversations/:id/messages`
- `/api/v1/conversations/:id/notes`
- `/api/v1/conversations/:id/status`
- `/api/v1/conversations/:id/assignment`
- `/api/v1/conversations/:id/tags`
- `/api/v1/ai-actions/:id/{review,approve,reject}`
- internal ingest/projection paths

### Current position

- This service is active in compose.
- It is missing from the main architecture doc, which makes that doc incomplete.

## conversation-ingest-rs

`apps/Application Plane/conversation-core/conversation-ingest-rs`

This is a thin Rust adapter into `conversation-core-go`.

### Responsibilities

- Accept ingress on `3161`.
- Forward into `conversation-core-go`.
- Hold internal API key and outbound HTTP client configuration.

### Current position

- Active in compose.
- Small but real.
- Also omitted from the main architecture doc.

## information-core

`apps/Application Plane/information-core`

This is a small Go service exposing internal information endpoints.

### Responsibilities

- Weather lookups.
- Traffic lookups.
- News lookups.
- Internal-key-gated access.

### Current position

- Active in compose on `3190`.
- Uses in-memory cache plus outbound HTTP clients.
- Not represented in the main Application Plane architecture doc.

This service reads more like a utility/internal-support API than a central workspace core, but it is live and should be documented honestly.

## notification-core

`apps/Application Plane/notification-core`

This is the first-party notification boundary.

### What it includes

- Postgres-backed notification request storage.
- Feed/inbox surface.
- Preferences API.
- Channel configuration API.
- Recipient/subscriber upsert surface.
- Novu adapter.
- NATS publication and shared-bus consumers.

### Exposed surface

- `/health`
- `/api/v1/notification-requests`
- `/notifications`
- `/notifications/unread/count`
- `/notifications/unseen/count`
- `/notifications/:id/read`
- `/notifications/:id/seen`
- `/notifications/mark-all-read`
- `/notifications/mark-all-seen`
- `/notifications/:id`
- `/preferences`
- `/channels/config`
- `/internal/recipients/upsert`

### Current position

- Much broader than the README's V0 summary.
- Subscribes to shared-bus control-session and identity sync events.
- Uses a real Novu adapter when configured and local stub mode when not.

## AFFiNE surfaces

`affine-core`, `affine-runtime`, `affine-runtime-migration`

### Current position

- Compose still contains an AFFiNE profile.
- `affine-runtime` and migration image paths exist in compose.
- `./affine-core` build context is missing from disk in the current workspace.

That makes the AFFiNE story partial or stale today. The runtime image path may still be usable, but the first-party `affine-core` build surface is absent.

## zammad-foundation

`apps/Application Plane/zammad-foundation`

This is not the main app compose runtime. It is a support-stack foundation package plus a separate Zammad compose path.

### What it includes

- Detailed Zammad architecture and operating model documentation.
- Bootstrap script package.
- Example config JSON.
- Admin/test checklists.
- Separate `docker-compose.zammad.yml`.

### Current position

- More foundation/bootstrap/documentation than integrated app runtime.
- Should be treated as a support-domain package inside the Application Plane, not confused with the always-on main compose runtime.

## Shared Infrastructure and Data Boundaries

Current Application Plane infrastructure:

- `application-postgres`
- `application-dragonfly`
- plane-local `app-nats`
- access to `inter-plane-bus`

### Important current boundary fact

The main architecture doc claims `application-postgres` is exclusively the notification DB. That is not true anymore in the current compose:

- `conversation-core-go` also points at `application-postgres`.
- AFFiNE runtime paths also point at `application-postgres`.
- This means the plane's database boundary is shared across multiple app services today.

## Stub, Mock, Placeholder, and TODO Audit

This section excludes normal test-only mocks and focuses on runtime-relevant or documentation-relevant partial surfaces.

## convex-core

1. `convex/http.ts`
   - `verifyWebhookSignature` is explicitly placeholder-grade.
   - When `WEBHOOK_SECRET` is unset it skips verification.
   - Even when set, current logic is just a prefix/equality placeholder instead of real HMAC verification.

2. `convex/http.ts`
   - Webhook handlers call `api.jobs.getByExternalId`, `api.jobs.updateStatus`, and `api.jobs.updateProgress`.
   - There is no `convex/jobs.ts` source module in the current tree.
   - That makes those webhook paths a stale or broken surface unless generated artifacts are masking removed source.

3. `convex/nats.ts`
   - `startSubscriber` is explicitly a placeholder.
   - The actual running subscriber logic lives in `nats-subscriber.js`, not in the Convex function itself.

4. `convex/ai.ts`
   - Still uses older `AI_CORE_URL` naming and older `/stream/chat` and `/chat` path assumptions.
   - Current compose points `AI_CORE_URL`/`MODEL_GATEWAY_URL` to `model-gateway:8080`, so these calls need to be treated as potentially stale integration assumptions until verified against the gateway surface.

5. `convex/messages.ts`
   - Assistant placeholder message creation for streaming is intentional and not itself a problem.

6. Compose defaults
   - Several `change-me` or dev-secret defaults remain in Convex-related envs.

## notification-core

1. `internal/runtime/client.go`
   - Stub mode is active whenever `NOVU_SECRET_KEY` is unset.
   - In stub mode dispatch returns synthetic transaction IDs and no real notifications are sent.

2. `internal/subscribers/service.go`
   - Explicitly inserts stub subscriber rows for sparse identity states that will later be merged.

## AFFiNE

1. `docker-compose.yml`
   - `affine-core` is declared, but the `./affine-core` directory is missing.
   - This is a concrete stale/broken runtime surface.

## Documentation/runtime mismatch

1. `APPLICATION_PLANE_ARCHITECTURE.md`
   - Omits `conversation-core-go`, `conversation-ingest-rs`, and `information-core`.
   - Still frames Model Plane as `Model Plane v2`.
   - Still claims `application-postgres` is only notification storage.

2. `convex-core/README.md` and `CONVEX_INTEGRATION.md`
   - Still use older `auth-service`, `org-core-service`, and `ai-core` narratives and URLs in parts.

## Relationship Coverage

The main relationships are mapped well enough for broader system analysis:

- Convex as reactive projection of Control/Model/Ingestion-derived state.
- Conversation and notification services as first-party app/support APIs.
- Information-core as internal app-support utility.
- Zammad foundation as adjacent support stack.
- Shared app Postgres/Dragonfly/NATS inside the plane.

What remains partially converged:

- AFFiNE runtime story.
- Convex webhook/job integration correctness.
- Whether `information-core` is still strategically used or just present.
- How tightly `zammad-foundation` is actually wired into `conversation-core` and `notification-core`.

## Likely Legacy, Unused, or Transitional Surfaces

### AFFiNE first-party core

Because `./affine-core` is missing, the first-party AFFiNE integration path looks transitional or partially removed.

### Old Convex integration assumptions

Some Convex docs and actions still reference older service names and older AI endpoint shapes.

### `docker-compose.ui.yml`

This overlay duplicates at least part of the main compose's Convex dashboard story and adds only a small AFFiNE port overlay. It may now be redundant or at least in need of a truth pass.

## Stale-Doc Candidates

These are candidates only. Do not delete until the cross-plane stale-doc register is complete.

1. `apps/Application Plane/APPLICATION_PLANE_ARCHITECTURE.md`
   - High-value but stale.
   - Omits active services and contains outdated Model Plane and DB-boundary assumptions.

2. `apps/Application Plane/convex-core/README.md`
   - Useful orientation doc, but integration examples still point at older service names and older AI-core framing.

3. `apps/Application Plane/convex-core/CONVEX_INTEGRATION.md`
   - Needs verification against the current compose and current Model Plane gateway contract.

4. `apps/Application Plane/docker-compose.ui.yml`
   - Candidate for consolidation or deletion if it no longer adds meaningful separation.

5. AFFiNE-related doc sections in runtime docs
   - Need review because the first-party `affine-core` build context is absent.

## Operational Notes

- The Application Plane is currently a broader app-support layer than the main architecture doc suggests.
- Convex is still intentionally non-authoritative, but it now mirrors more control/model/application state than earlier docs emphasize.
- `conversation-core-go` and `notification-core` are meaningful first-party APIs, not side notes.
- `information-core` is live but strategically narrower than the other application services.
- Zammad belongs here as a support foundation package, but not as part of the always-on main compose runtime.

## Recommended Follow-Up Checks

1. Verify whether any current clients still depend on AFFiNE paths before removing or rewriting those docs.
2. Verify whether `convex/ai.ts` still works against current `model-gateway` endpoints or is now dead/stale code.
3. Verify whether missing `jobs.ts` means Convex webhook job paths are broken or generated from an untracked source.
4. Trace real consumers of `information-core` before classifying it as keep/update/delete.
5. During the stale-doc register pass, treat `APPLICATION_PLANE_ARCHITECTURE.md` as a likely update target, not an immediate delete candidate.
