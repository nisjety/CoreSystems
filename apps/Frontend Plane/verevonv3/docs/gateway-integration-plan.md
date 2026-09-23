# verevonv3 Gateway Integration Plan

**Architecture decision (FIXED):** verevonv3 is a SolidJS SPA with no separate application
server. It talks only to the same-origin
`verevon-gateway-rs` (Frontend Plane, `apps/Frontend Plane/verevonv3/apps/gateway/`, HTTP :3185).
The gateway authenticates every request against the Control Plane session, enforces org scoping,
and proxies/fans out to plane boundary services. No other origin is ever called from the browser,
with one structural exception: the integration OAuth redirect (`GET /oauth/callback/:provider`)
which the provider drives, not our JS.

Companion document: `endpoint-map.md` (full upstream catalog + per-surface consumption view).

## Ownership hardening update — 2026-08-11

The gateway remains a session-aware browser boundary. It may validate a small
allowlist, mint a scoped audience token, attach the active organization derived
from the Control Plane session, and adapt an owner service's wire format. It
must not manufacture data, make a durable business decision, or become a
second repository for another plane.

The following first-pass corrections are implemented and covered by focused
tests:

| Surface | Durable authority | Gateway responsibility now |
|---|---|---|
| Insights | `insight-core` | Proxies its raw connector/overview contract after resolving the active org; `organization_required` is explicit instead of a fabricated empty overview. |
| Knowledge | Data Plane v2 plus Quarry/integration owners | Preserves scoped reads and returns `organization_required` rather than a synthetic workspace. The current workspace composite is explicitly a temporary, stateless adapter until an owner-plane read-model contract exists. |
| Ingestion | Data Plane v2, Integration Core, and Quarry-v2 | Preserves lifecycle and evidence contracts. The source inventory requires an active org rather than returning an unscoped partial list; its current source-card composite is temporary, stateless response shaping. |
| Navbar | `user-core` / `notification-core` | Reads and writes User Core appearance state and submits support requests to User Core; it does not acknowledge a change that the owner did not save. |
| Billing gate | `billing-core` | Leads Core enforces the `leads` entitlement at its own build/export boundary; the gateway paywall is only early UX feedback. `GET /billing/account` now forwards Billing Core failures instead of returning a fabricated local account. |
| Support AI / recurrence | `conversation-core` with `org-core` policy | Conversation Core fails closed on ZDR, AI mode, and recurrence capability decisions. Gateway checks are early feedback, not the sole authority. |
| Onboarding plan recommendation | Model Plane | The gateway relays the Model Plane recommendation or returns `recommendation_unavailable`; Billing Core remains the owner of plans, entitlements, and checkout, not model-generated recommendation. |

Two response-shaping projections remain temporarily in the Frontend Plane: the
Knowledge workspace card view and the Ingestion source-card view. They combine
already-authoritative, scoped reads and own no records, quotas, policy
decisions, or durable state. They are not yet a complete deduplication result:
both need an explicit owner-plane read-model contract before the gateway can
be reduced to thin normalization.

### Reconciliation result — 2026-08-11

The product/ownership documents were re-read against the current source. When
older research or a historical plan differs, the master ownership matrix and
the roadmap execution ledger take precedence.

| Area | Current source evidence | Reconciled state |
|---|---|---|
| Insights, Navbar, billing entitlement, and support policy | Gateway handlers proxy owner APIs; `leads-core` checks Billing Core itself; `conversation-core` checks Org Core policy before retaining AI proposals or returning recurrence candidates. | Aligned with the authority rules. |
| Billing account | The previous debug/development fallback returned a made-up successful account on a Billing Core server error. | Corrected here: owner errors now pass through unchanged. |
| Onboarding recommendation | `onboarding/lookup/plan.rs` requests `model_recommend_url`; Billing Core has no plan-recommendation endpoint. | Model Plane owns recommendation; Billing Core owns the selected plan and entitlement decision. |
| Knowledge and Ingestion composites | `knowledge/workspace.rs` and `ingestions/sources.rs` fan out to scoped Data Plane, Integration, and Quarry reads and calculate SPA cards in-process. | No duplicated durable data, but too much read-model composition remains in the BFF. Define a projection contract under the correct owner before treating this work as complete. |
| Chat history | `chat/history.rs` now lists the Session Core thread projection and relays title, preview, pin, single archive, and archive-all to Model Gateway. It cleans a legacy index only after the owner acknowledges the archive. | Partially aligned: presentation and visibility state have moved to the owner, with ZDR-aware writes and an audit event. The bounded rich-transcript/task-step cache remains temporary until canonical messages, orchestration todos, and run/artifact evidence are projected in one owner-backed read model. |
| Browser run metadata | Quarry owns the actor-bound browser-session projection, step receipts, current live observation, profile scope, and compact append-only timeline events. The projection, timeline, and control hand-off all require the verified `org_id` **and** signed initiating actor; legacy evidence without an actor fails closed. | Aligned at runtime. The BFF forwards owner reads and mutations, then applies a pure same-origin presentation adapter. `BrowserRunStore`/replay metadata remains available only to legacy unit fixtures and is not part of the gateway binary. |
| Studio | Existing Studio work is intentionally deferred by the roadmap sequence. | Do not migrate until the preceding owner contracts are complete. |

### Browser-run owner migration — next implementation pass

Quarry is the browser-execution authority. Its current agent-run lane already
owns live Chromium sessions, verified tenant claims, action execution,
artifact-backed observations, and immutable action receipts. Its procedure
replay endpoint compares a proposed procedure with receipt evidence; it is not
yet the browser-session projection consumed by the Verevon UI.

#### Owner-contract migration — 2026-08-11

The first safe migration increment is implemented:

- Quarry's existing checkpoint is extended with the signed initiating
  `actor_id` and the historical `lease_id`. Both fields default on decode, so
  old JSON checkpoint rows remain readable by the store but cannot authorize a
  user-scoped browser-session projection.
- `GET /v1/agent/runs/{run_id}/browser-session` returns a live, tab-aware
  projection when Chromium is present, or a durable non-ZDR checkpoint
  projection after restart. It includes owner-held profile scope and the latest
  live observation when one exists. It derives both tenant and actor only from
  verified claims and returns `404` for a foreign/legacy record.
- Quarry's live step, frame, tab, DevTools, receipt, and close paths now use
  that same tenant-and-actor check. A different member of the same organization
  cannot control a guessed active run identifier.
- Forward-only migrations `0008_step_receipt_actor_scope.sql` and
  `0009_browser_timeline_events.sql` bind all new step receipts to their
  initiating actor and create Quarry's append-only browser owner-event stream.
  No legacy receipt is backfilled or guessed: actor-scoped receipt and
  procedure reads filter it out.
- `POST /v1/agent/runs/{run_id}/browser-session/control` persists a compact
  control hand-off; start, tabs, DevTools summaries, and close append compact
  owner events. The cursor-paginated
  `GET /v1/agent/runs/{run_id}/browser-session/timeline` merges these events
  with actor-bound action outcomes. It returns no page body, frame data, raw
  DevTools payload, credential, or browser-storage value.
- `zdr=true` now skips durable action receipts and timeline events as well as
  checkpoints and persistent profiles. It may still expose transient live
  browser state while the run remains active.
- Display URLs are stripped of user info, query, and fragment before leaving
  Quarry. Live frames remain transient and are not included.
- `GET /api/v1/browser/sessions/{session_id}` and the new
  `GET /api/v1/browser/sessions/{session_id}/timeline` forward only Quarry
  owner resources. `POST /api/v1/browser/sessions/{session_id}/control`
  forwards the control transfer before returning the owner projection.

The runtime migration is complete: browser creation, actions, control, tabs,
suggestions, AI-run launch, artifacts, frames, SSE, DevTools, and WebSocket
authorization read Quarry's owner projection or the separately owner-scoped
Model Plane orchestration record. The gateway retains no browser run cache in
the production binary. Its response adapter is pure and only supplies
same-origin route URLs and safe client defaults; it neither stores nor
authorizes browser state.

| Quarry owner resource | Required behavior | BFF behavior after migration |
|---|---|---|
| `GET /v1/agent/runs/{run_id}/browser-session` | Return the current run projection: run and lease identifiers, verified tenant and actor scope, profile scope, viewport, status, ZDR, control mode, and current tab summary. Never return raw credentials or frame payloads. | Proxy `{ data }`; no local reconstruction or default values. |
| `POST /v1/agent/runs/{run_id}/browser-session/control` | Validate an explicit human/agent control-mode transition against the run owner; append an immutable receipt/event with actor and timestamp. | Forward the requested transition and use the owner response. |
| `GET /v1/agent/runs/{run_id}/browser-session/timeline?cursor=` | Return ordered, immutable browser UI events backed by run receipts/observations: action result, control transition, tab change, compact DevTools summary, and safe artifact reference. Frames remain live/transient and are never persisted as image payloads. | Proxy cursor/meta unchanged; do not create a replay timeline in memory. |
| Start, step, tab, DevTools, and close operations | Each changes the same owner projection and records a sequence-stable event or receipt. A completed browser call is still only an observed browser result, not proof of a business effect. | Forward to Quarry, then render the returned owner projection/timeline. |

The owner record must bind both dimensions available in Quarry's verified
claims: `org_id` **and** `actor_id`/`user_id` for a user-started run. A
service principal may act only through an explicit delegated/automation rule
and must retain its signed service actor in the receipt. The existing
org-only live-run check is insufficient for user-scoped history. No route may
accept a caller-supplied organization or owner field.

Persistence and privacy requirements:

- Quarry may retain only its own execution evidence through its owner-store
  contract; no other plane reads or writes that store directly.
- The timeline contains compact metadata and artifact references, not raw CDP,
  cookies, credentials, or frame/image content. `zdr=true` forbids durable
  observation/timeline payloads and durable browser profile use; it may expose
  an ephemeral live status while the session exists.
- The receipt stream remains append-only. Closing or restarting a live
  session must not turn a missing in-memory entry into authorization success;
  the durable owner record decides whether a scoped read is allowed.
- Model Plane can propose actions and receives evidence through its contracts;
  Quarry alone executes, records, or rejects browser work.

Implementation order and exit criteria:

1. Add Docker-backed integration coverage for cross-org, cross-user, service
   delegation, ZDR, close/restart, migration application, and timeline
   ordering.
2. **Completed 2026-08-11:** the browser dashboard hydrates the paginated
   actor-scoped Quarry timeline after session creation, restore, and each
   mutation. It displays compact activity separately from evidence and clears
   the old local replay list once the canonical read succeeds; an unavailable
   canonical read is surfaced explicitly rather than shown as empty history.
3. Remove the now test-only `BrowserRunStore` fixture harness after its
   browser-domain tests are rewritten around mock Quarry projections.

The owner contract and its dashboard consumption are now implemented. Remaining
work is Docker-backed integration coverage and test-fixture cleanup, not a
second browser runtime in the gateway.

Response convention for every gateway route (normalizing the differing upstream envelopes —
integration-api `{success,data|error}`, finspo `{success,data,error}`, raw plane JSON):

```jsonc
// success
{ "data": { ... }, "meta": { "cursor": "..." }, "links": { "next": "..." } } // meta/links only for paginated lists
// failure
{ "error": { "code": "ORG_FORBIDDEN", "message": "Not a member of this organization", "details": { } } }
```

SSE routes stream upstream events verbatim (additive/versioned event types only) and carry `id:`
fields so `Last-Event-ID` resume works.

---

## A. Target gateway route surface

Status: **EXISTS** = registered today in `apps/gateway/src/` (20 routes: `/health` + current
onboarding routes wired from `main.rs` and its supporting modules). **ADD** = must be implemented
(144 route rows below). Target surface: **164 route rows**.

### A.1 Health + session (existing + Phase 1)

| Gateway route | Upstream | Status | Phase |
|---|---|---|---|
| `GET /health` | — local | EXISTS | — |
| `GET /api/v1/session/bootstrap` | — header echo (rework: derive from validated session, not raw headers) | EXISTS (rework) | 1 |
| `GET /api/v1/session/current` | session-core `GET /api/v1/sessions/current` | ADD | 1 |
| `POST /api/v1/session/refresh` | session-core `POST /api/v1/sessions/refresh` | ADD | 1 |

### A.2 Auth domain — `domains/auth.rs` (Phase 1, all ADD)

| Gateway route | Upstream |
|---|---|
| `POST /api/v1/auth/sign-up` | auth-core `POST /api/v2/auth/signUp` (Set-Cookie passthrough) |
| `POST /api/v1/auth/sign-in` | auth-core `POST /api/v2/auth/signIn` (Set-Cookie passthrough) |
| `POST /api/v1/auth/sign-out` | auth-core `POST /api/v2/auth/signOut` |
| `GET /api/v1/auth/session` | auth-core `POST /api/v2/auth/getSession` |
| `POST /api/v1/auth/email-verification/send` | auth-core `POST /api/v2/auth/sendEmailVerification` |
| `POST /api/v1/auth/email-verification/verify` | auth-core `POST /api/v2/auth/verifyEmail` |
| `POST /api/v1/auth/password/check-strength` | auth-core `POST /api/v2/auth/password/check-strength` |
| `POST /api/v1/auth/password/send-reset` | auth-core `POST /api/v2/auth/sendPasswordReset` |
| `POST /api/v1/auth/password/reset` | auth-core `POST /api/v2/auth/resetPassword` |
| `POST /api/v1/auth/oauth/initiate` | auth-core `POST /api/v2/auth/oauth/initiate` |
| `GET /api/v1/me` | user-core `GET /api/v1/users/me` |
| `PATCH /api/v1/me` | user-core `PATCH /api/v1/users/me` |
| `GET /api/v1/me/session-context` | user-core `GET /api/v1/me/session-context` |
| `GET /api/v1/billing/account` | billing-core `GET /api/v1/billing/orgs/{orgId}/account` (orgId from validated session) |
| `PUT /api/v1/billing/account` | billing-core `PUT /api/v1/billing/orgs/{orgId}/account` (trial start) |

(13 auth/me routes + 2 billing-account routes + 2 session routes from A.1 = **17 ADD in Phase 1**.)

### A.3 Onboarding domain — current onboarding routes (all EXISTS; hardened in Phase 1)

All 18 remaining onboarding routes from `endpoint-map.md` §4.1 stay as-is:
`GET /api/v1/onboarding/status`, `GET /api/v1/onboarding/brreg/search`,
`GET /api/v1/onboarding/graph-preview`, `POST /api/v1/onboarding/crawl-preview` (SSE),
`POST /api/v1/onboarding/recommend-plan`, `GET|PUT /api/v1/onboarding/state`,
`PUT /api/v1/onboarding/theme`, `POST /api/v1/onboarding/complete`, and the nine
`POST /api/v1/onboarding/actions/{create-organization, set-plan, start-checkout,
start-website-ingest, start-connect-session, discover-source, cleanup-source,
warm-sharepoint-discovery, start-integration-sync}`.

Phase 1 hardening (no new routes): put them behind the auth middleware, stop masking upstream
failures as `200 + success:false` (return the typed `{ error }` envelope with real status), and
verify `discover-source`/`cleanup-source` upstream paths against integration-api (the v2 BFF used
`GET /api/v1/connections` + finspo + documents-api here — confirm gateway parity or fix).

### A.4 Chat + AG-UI domains — `domains/chat.rs`, `domains/ag_ui.rs` (Phase 2, all ADD — 10 routes)

| Gateway route | Upstream (model-gateway :8080, Bearer = minted `model-plane` audience token) |
|---|---|
| `POST /api/v1/chat/stream` (SSE) | `POST /v1/invoke/stream` — re-stream verbatim, `profile:"chat"` preserved |
| `POST /api/v1/chat/invoke` | `POST /v1/invoke` |
| `GET /api/v1/chat/stream/resume/:request_id` (SSE) | `GET /v1/invoke/resume/:request_id` (forward `Last-Event-ID`) |
| `POST /api/v1/chat/invocations/:request_id/cancel` | `POST /v1/invoke/:request_id/cancel` |
| `GET\|DELETE /api/v1/chat/threads` | Model Gateway `GET\|DELETE /v1/threads`; Session Core owns the visible-thread projection and archive-all receipt. |
| `PUT\|DELETE /api/v1/chat/threads/:thread_id` | Model Gateway `POST /v1/threads/:thread_id/presentation` and `DELETE /v1/threads/:thread_id`; Session Core owns title, preview, pin, and archive receipts. The BFF cache is not an authority. |
| `GET /api/v1/chat/threads/:thread_id/transcript` | Read-through adapter over Model Gateway `/v1/threads/:thread_id/messages` (Session Core canonical conversation); task-step/run evidence is intentionally a separate owner-backed projection. |
| `GET /api/v1/chat/threads/:thread_id/messages` | `GET /v1/threads/:thread_id/messages` |
| `GET /api/v1/models` | `GET /v1/models` |
| `POST /api/v1/chat/documents` | `POST /v1/chat/documents` (multipart passthrough, ZDR header propagated) |
| `POST /api/v1/chat/feedback` | `POST /v1/feedback` |
| `GET /api/v1/runs/:run_id/events` (SSE) | `GET /v1/runs/:run_id/events` — also the real backing for the action-client's promised `/api/v1/runs/{runId}/events` |
| `POST /api/v1/ag-ui/stream` (SSE) | `POST /v1/invoke/stream` — accepts legacy chat bodies or TanStack/AG-UI `RunAgentInput`, normalizes to Model Plane invoke shape, propagates ZDR, and maps Model Plane SSE events into AG-UI `RUN_*`, `TEXT_MESSAGE_*`, `TOOL_CALL_*`, and `CUSTOM` payloads for Verevon agentic UI clients |

### A.4.1 Voice dictation and transcription domain — `domains/voice.rs` (Phase 2B, all ADD — 11 routes)

Companion product/architecture plan: `voice-dictation-and-transcription-plan.md`.
The gateway is the policy boundary. The SPA may request a mode, but the gateway
derives the effective voice mode from org/user policy and propagates `x-zdr` to
every content-carrying upstream call.

| Gateway route | Upstream |
|---|---|
| `POST /api/v1/voice/transcribe` | model-gateway `POST /v1/ai/speech` with `operation:"stt"`; short audio/chunked upload; no raw audio/transcript persistence under ZDR |
| `POST /api/v1/voice/transcribe/stream` | model-gateway streaming STT target (new `/v1/ai/speech/stream` or realtime session bridge); no replay buffer under ZDR |
| `POST /api/v1/voice/format` | model-gateway `POST /v1/invoke` with profile `voice_format`; prompt cache/session writes disabled under ZDR |
| `POST /api/v1/voice/commands/interpret` | model-gateway `POST /v1/invoke` with profile `voice_command`; returns command classification + preview payload |
| `GET /api/v1/voice/policy` | user-core/org-core effective voice policy: allowed modes, default mode, retention, cloud allowed, Teams import allowed |
| `GET /api/v1/voice/dictionary` | user-core settings; user/org-authored vocabulary only, not learned transcript content unless explicitly approved |
| `PUT /api/v1/voice/dictionary` | user-core settings; replace/update vocabulary entries |
| `GET /api/v1/voice/snippets` | user-core settings; user-authored snippets and custom voice prompts |
| `PUT /api/v1/voice/snippets` | user-core settings; replace/update snippets and custom voice prompts |
| `POST /api/v1/voice/teams/transcripts/import` | integration-corev2 Microsoft Graph transcript import + Model Plane ephemeral summarization; saves only approved summary/tasks by default |
| `GET /api/v1/voice/teams/imports/:job_id/events` (SSE) | integration-corev2 sync-job events normalized to progress metadata; no raw transcript event payloads under ZDR |

### A.5 Knowledge domain — `domains/knowledge.rs` (Phase 3, all ADD — 24 routes)

| Gateway route | Upstream |
|---|---|
| `GET /api/v1/knowledge/documents` | documents-api `GET /v1/documents` |
| `GET /api/v1/knowledge/documents/:id` | documents-api `GET /v1/documents/{id}` |
| `DELETE /api/v1/knowledge/documents/:id` | documents-api `DELETE /v1/documents/{id}` |
| `GET /api/v1/knowledge/sources` | documents-api `GET /v1/sources` |
| `POST /api/v1/knowledge/search` | retrieval-engine `POST /v1/knowledge/search` |
| `POST /api/v1/knowledge/graph-search` | retrieval-engine `POST /v1/knowledge/graph` |
| `POST /api/v1/knowledge/wiki-search` | retrieval-engine `POST /v1/knowledge/wiki` |
| `POST /api/v1/knowledge/citations/sources` | retrieval-engine `POST /v1/retrieve/sources` |
| `POST /api/v1/knowledge/citations/chunks` | retrieval-engine `POST /v1/retrieve/chunks` |
| `GET /api/v1/knowledge/retrieval-traces/:trace_id` | retrieval-engine `GET /v1/retrieval/{trace_id}` |
| `GET /api/v1/knowledge/wiki/pages` | wiki-store `GET /v1/wiki/pages` |
| `GET /api/v1/knowledge/wiki/pages/by-path` | wiki-store `GET /v1/wiki/pages/by-path` |
| `GET /api/v1/knowledge/wiki/pages/:page_id` | wiki-store `GET /v1/wiki/pages/{pageID}` |
| `GET /api/v1/knowledge/wiki/pages/:page_id/versions` | wiki-store versions |
| `GET /api/v1/knowledge/wiki/pages/:page_id/diff` | wiki-store diff |
| `GET /api/v1/knowledge/wiki/pages/:page_id/backlinks` | wiki-store backlinks |
| `GET /api/v1/knowledge/graph` | graph-index `GET /v1/graphs/{org_id}` (org from session; workspace twin of onboarding graph-preview) |
| `POST /api/v1/knowledge/imports/upload` | imports-core `POST /api/v1/import/jobs/upload` (multipart) |
| `POST /api/v1/knowledge/imports/source` | imports-core `POST /api/v1/import/jobs/source` |
| `GET /api/v1/knowledge/imports/:job_id` | imports-core `GET /api/v1/import/jobs/{job_id}` |
| `GET /api/v1/knowledge/imports/:job_id/events` | imports-core `GET /api/v1/import/jobs/{job_id}/events` |
| `POST /api/v1/knowledge/crawl` | quarry-edge `POST /v1/crawl` (minted `quarry` audience token) — BUILT |
| `GET /api/v1/knowledge/crawl/jobs` | quarry-edge `GET /v1/crawl/jobs` — BUILT |
| `GET /api/v1/knowledge/runs/:id/events` | quarry-edge `GET /v1/runs/:id/events` — BUILT |

### A.6 Actions domain — `domains/actions.rs` (Phase 3 scaffold, Phase 5 complete — 2 ADD)

| Gateway route | Behavior |
|---|---|
| `POST /api/v1/actions/:action_id/execute` | Validates payload against the descriptor contract (mirror of `src/shared/actions/action-registry.ts` Zod schemas), checks approval requirement, dispatches by owner plane: `knowledge.recrawl_source` → quarry-edge `POST /v1/crawl`; `security.check_url_reputation` → security domain `POST /api/v1/security/url-reputation-checks`; `security.investigate_url` → security domain `POST /api/v1/security/url-investigations`; `inbox.draft_reply` → conversation-core message + ai-action review; `workflows.toggle_policy` → user-core settings (interim store, see Phase 4); `agents.deploy_channel` → 501 `{error:{code:"CHANNEL_PLANE_NOT_AVAILABLE"}}`. Emits a real audit event (NATS `verevon.audit.v1.>` or audit-core `POST /v1/audit`) and returns real `run_id`/`audit_id`. |
| `GET /api/v1/action-runs/:run_id/events` (SSE) | Streams the owner plane's run events (model-gateway `/v1/runs/:id/events`, quarry-edge `/v1/runs/:id/events`, or integration-api `GET /api/v1/sync-jobs/:id/events` polled→SSE) under one normalized event shape. |

### A.7 Security connectors domain — `domains/security.rs` (ADD — 2 routes)

| Gateway route | Upstream |
|---|---|
| `POST /api/v1/security/url-reputation-checks` | Validate HTTP(S) URL + org policy; check local feeds/cache first; optionally call Google Web Risk Lookup API with `GOOGLE_WEB_RISK_API_KEY`; return normalized `{data}` verdict. |
| `POST /api/v1/security/url-investigations` | Validate HTTP(S) URL + explicit `allowExternalSubmission`; default urlscan visibility to `private`; call urlscan.io with `URLSCAN_API_KEY`; reject non-public data unless visibility is `private`; emit audit event. |

### A.8 Settings / Orgs / Billing / Notifications / Integrations — `domains/{settings,orgs,billing,notifications,integrations}.rs` (Phase 4, all ADD — 40 routes)

| Gateway route | Upstream |
|---|---|
| `GET /api/v1/settings/:section` | user-core `GET /api/v1/settings/{section}` (section ∈ appearance, language, privacy, notifications, security, accessibility, ai, storage — validate enum at the gateway) |
| `PUT /api/v1/settings/:section` | user-core `PUT /api/v1/settings/{section}` |
| `GET /api/v1/preferences` | user-core `GET /api/v1/preferences` |
| `PATCH /api/v1/preferences` | user-core `PATCH /api/v1/preferences` |
| `GET /api/v1/orgs` | org-core `GET /api/v1/organizations` |
| `GET /api/v1/orgs/:id` | org-core `GET /api/v1/organizations/:id` |
| `GET /api/v1/orgs/:id/entitlements` | org-core `GET /api/v1/organizations/:id/entitlements` |
| `GET /api/v1/orgs/:id/members` | org-core `GET /orgs/:id/members` (compat family — only place RBAC exists) |
| `POST /api/v1/orgs/:id/members/invite` | org-core `POST /orgs/:id/members/invite` |
| `DELETE /api/v1/orgs/:id/members/:user_id` | org-core `DELETE /orgs/:id/members/:userId` |
| `GET /api/v1/orgs/:id/roles` | org-core `GET /orgs/:id/roles` |
| `GET /api/v1/orgs/:id/roles/catalog` | org-core `GET /orgs/:id/roles/catalog` |
| `PATCH /api/v1/orgs/:id/members/:user_id/role` | org-core `PATCH /orgs/:id/members/:userId/role` |
| `GET /api/v1/billing/entitlements/:feature` | billing-core `GET /api/v1/billing/orgs/{orgId}/entitlements/{feature}` |
| `GET /api/v1/billing/quotas/:metric` | billing-core `GET /api/v1/billing/orgs/{orgId}/quotas/{metric}` |
| `POST /api/v1/billing/checkout-session` | billing-core `POST /api/v1/billing/orgs/{orgId}/checkout-session` |
| `GET /api/v1/notifications` | notification-core `GET /notifications` |
| `GET /api/v1/notifications/unread/count` | notification-core `GET /notifications/unread/count` |
| `GET /api/v1/notifications/unseen/count` | notification-core `GET /notifications/unseen/count` |
| `POST /api/v1/notifications/:id/read` | notification-core `POST /notifications/:id/read` |
| `POST /api/v1/notifications/:id/seen` | notification-core `POST /notifications/:id/seen` |
| `POST /api/v1/notifications/mark-all-read` | notification-core `POST /notifications/mark-all-read` |
| `POST /api/v1/notifications/mark-all-seen` | notification-core `POST /notifications/mark-all-seen` |
| `DELETE /api/v1/notifications/:id` | notification-core `DELETE /notifications/:id` |
| `GET /api/v1/notifications/preferences` | notification-core `GET /preferences` |
| `PUT /api/v1/notifications/preferences/:event_type/:channel` | notification-core `PUT /preferences/:eventType/:channel` |
| `GET /api/v1/integrations/providers` | integration-api `GET /api/v1/providers` |
| `GET /api/v1/integrations/connections` | integration-api `GET /api/v1/connections` |
| `GET /api/v1/integrations/connections/:id` | integration-api `GET /api/v1/connections/:id` |
| `GET /api/v1/integrations/connections/:id/status` | integration-api `GET /api/v1/connections/:id/status` |
| `PATCH /api/v1/integrations/connections/:id/capabilities` | integration-api `PATCH /api/v1/connections/:id/capabilities` |
| `DELETE /api/v1/integrations/connections/:id` | integration-api `DELETE /api/v1/connections/:id` |
| `POST /api/v1/integrations/connections/:id/sync` | integration-api `POST /api/v1/connections/:id/sync` |
| `GET /api/v1/integrations/sync-jobs` | integration-api `GET /api/v1/sync-jobs` |
| `GET /api/v1/integrations/sync-jobs/:id` | integration-api `GET /api/v1/sync-jobs/:id` |
| `GET /api/v1/integrations/sync-jobs/:id/events` | integration-api `GET /api/v1/sync-jobs/:id/events` |
| `POST /api/v1/integrations/sync-jobs/:id/cancel` | integration-api `POST /api/v1/sync-jobs/:id/cancel` |
| `POST /api/v1/integrations/sync-jobs/:id/retry` | integration-api `POST /api/v1/sync-jobs/:id/retry` |
| `GET /api/v1/integrations/connect-sessions/:id/status` | integration-api `GET /api/v1/connect-sessions/:id/status` |
| `GET /api/v1/integrations/profile` | integration-api `GET /api/v1/projections/integration-profile` |

### A.8 Inbox / Agents / Dashboard — `domains/{inbox,agents,dashboard}.rs` (Phase 5, all ADD — 40 routes)

| Gateway route | Upstream |
|---|---|
| `GET /api/v1/inbox/inboxes` | conversation-core `GET /api/v1/inboxes` |
| `GET /api/v1/inbox/inboxes/:id/queue` | conversation-core `GET /api/v1/inboxes/:id/queue` |
| `GET /api/v1/inbox/conversations` | conversation-core `GET /api/v1/conversations` |
| `GET /api/v1/inbox/conversations/:id` | conversation-core `GET /api/v1/conversations/:id` |
| `POST /api/v1/inbox/conversations/search` | conversation-core `POST /api/v1/conversations/search` |
| `POST /api/v1/inbox/conversations/:id/messages` | conversation-core `POST /api/v1/conversations/:id/messages` |
| `POST /api/v1/inbox/conversations/:id/notes` | conversation-core `POST /api/v1/conversations/:id/notes` |
| `PATCH /api/v1/inbox/conversations/:id/status` | conversation-core `PATCH /api/v1/conversations/:id/status` |
| `PATCH /api/v1/inbox/conversations/:id/assignment` | conversation-core `PATCH /api/v1/conversations/:id/assignment` |
| `POST /api/v1/inbox/conversations/:id/tags` | conversation-core `POST /api/v1/conversations/:id/tags` |
| `DELETE /api/v1/inbox/conversations/:id/tags/:tag` | conversation-core `DELETE /api/v1/conversations/:id/tags/:tag` |
| `POST /api/v1/inbox/ai-actions/:id/review` | conversation-core `POST /api/v1/ai-actions/:id/review` |
| `POST /api/v1/inbox/ai-actions/:id/approve` | conversation-core `POST /api/v1/ai-actions/:id/approve` |
| `POST /api/v1/inbox/ai-actions/:id/reject` | conversation-core `POST /api/v1/ai-actions/:id/reject` |
| `GET /api/v1/agents/capabilities` | model-gateway `GET /v1/capabilities` |
| `GET /api/v1/agents/capabilities/:id` | model-gateway `GET /v1/capabilities/:id` |
| `GET\|POST /api/v1/agents/tasks` | model-gateway `GET\|POST /v1/tasks` |
| `GET\|PATCH /api/v1/agents/tasks/:id` | model-gateway `GET\|PATCH /v1/tasks/:id` |
| `POST /api/v1/agents/tasks/:id/cancel` | model-gateway `POST /v1/tasks/:id/cancel` |
| `GET\|POST /api/v1/agents/cron` | model-gateway `GET\|POST /v1/cron` |
| `GET\|PATCH\|DELETE /api/v1/agents/cron/:id` | model-gateway `/v1/cron/:id` |
| `GET\|POST /api/v1/agents/memory` | model-gateway `GET\|POST /v1/memory` |
| `GET\|PATCH\|DELETE /api/v1/agents/memory/:id` | model-gateway `/v1/memory/:id` |
| `GET\|POST /api/v1/agents/skills` | model-gateway `GET\|POST /v1/skills` |
| `GET\|PATCH\|DELETE /api/v1/agents/skills/:id` | model-gateway `/v1/skills/:id` |
| `GET /api/v1/agents/runs/:run_id/plans` | model-gateway `GET /v1/orchestration/runs/:run_id/plans` |
| `GET /api/v1/agents/plans/:plan_id` | model-gateway `GET /v1/orchestration/plans/:plan_id` |
| `POST /api/v1/agents/plans/:plan_id/approve` | model-gateway `POST /v1/orchestration/plans/:plan_id/approve` |
| `POST /api/v1/agents/plans/:plan_id/reject` | model-gateway `POST /v1/orchestration/plans/:plan_id/reject` |
| `GET /api/v1/agents/threads/:thread_id/todos` | model-gateway `GET /v1/orchestration/threads/:thread_id/todos` |
| `POST /api/v1/agents/todos/:todo_id/status` | model-gateway `POST /v1/orchestration/todos/:todo_id/status` |
| `GET /api/v1/agents/runs/:run_id/approvals` | model-gateway `GET /v1/orchestration/runs/:run_id/approvals` |
| `POST /api/v1/agents/approvals/:approval_id/decide` | model-gateway `POST /v1/orchestration/approvals/:approval_id/decide` |
| `POST /api/v1/agents/runs/:run_id/cancel` | model-gateway `POST /v1/orchestration/runs/:run_id/cancel` |
| `POST /api/v1/agents/runs/:run_id/resume` | model-gateway `POST /v1/orchestration/runs/:run_id/resume` |
| `GET /api/v1/dashboard/usage-summary` | audit-core `GET /v1/usage/summary?org_id={validated}` |
| `GET /api/v1/dashboard/audit` | audit-core `GET /v1/audit?org_id={validated}` |
| `GET /api/v1/dashboard/info/weather` | information-core `GET /api/v1/weather` |
| `GET /api/v1/dashboard/info/traffic` | information-core `GET /api/v1/traffic` |
| `GET /api/v1/dashboard/info/news` | information-core `GET /api/v1/news` |

### A.9 Search domain — `domains/search.rs` (BUILT — 5 routes, additive)

Powers the home dashboard "Søk" surface; net-new relative to the original plan (no search surface was
enumerated above). All web-search providers (SearXNG, Tavily-replacement, Brave, local Tantivy) live
inside quarry-edge's SmartSearchRouter / AnswerPipeline — the gateway only proxies and normalizes. The
internal "i Verevon" arm reuses `GET /api/v1/navbar/search?scope=knowledge` (retrieval-engine). All
routes sit behind the session middleware.

| Gateway route | Upstream |
|---|---|
| `POST /api/v1/search/web` | quarry-edge `POST /v1/search` (keyword) or `POST /v1/scrape` (URL / bare-domain input); normalized to `{mode,results,answer,citations}` / `{mode:"fetch",url,title,description,excerpt}` |
| `POST /api/v1/search/images` | quarry-edge `POST /v1/search/images` → sanitized `{images:[{url,thumbnailUrl,imageUrl,title}]}` |
| `POST /api/v1/search/videos` | quarry-edge `POST /v1/search/videos` → sanitized embeddable `{videos:[{url,title,thumbnailUrl,embedUrl,author,length}]}` |
| `GET /api/v1/search/suggestions` | autocomplete-core `GET /v1/suggestions` (`AUTOCOMPLETE_CORE_URL` + `AUTOCOMPLETE_INTERNAL_TOKEN`; degrades to an empty list when unconfigured) |
| `POST /api/v1/search/answer/stream` (SSE) | quarry-edge `POST /v1/answer/stream` — re-streams `citations`/`delta`/`done`/`error` verbatim |

Config added to `config.rs`: `AUTOCOMPLETE_CORE_URL` (default `http://autocomplete-core:3219`),
`AUTOCOMPLETE_INTERNAL_TOKEN` (optional). This promotes `autocomplete-core` from §E "later
(deployment unverified)" to a live (graceful-degrade) upstream.

**`SEARXNG_URL` is no longer a gateway upstream (closed 2026-09-15).** Until then `videos` was the one
route in this table that skipped quarry-edge and called SearXNG itself — and, being the only handler in
`search.rs` with no `AuthenticatedUser` extension, it could mint no quarry token and so reached the
provider with no org scope, no cache, no host-diversity cap and no metered unit. It was authenticated
by the session guard but never attributed. It now proxies `POST /v1/search/videos` like its siblings;
`sanitize_videos` stays BFF-side because trimming results to embeddable fields is this layer's job.
The gateway keeps no SearXNG reachability of its own: `searxng_url` in `config.rs` is now unread — the
compiler says so (`field 'searxng_url' is never read`) — and is pending deletion together with the five
test-fixture `AppState` literals that still name it (`main.rs`, `domains/browser.rs`,
`domains/chat/shared.rs`, `domains/orchestration.rs`, `onboarding/crawl_preview/stream_e2e.rs`). Do not
reintroduce a direct provider call here, even as a fallback: an un-attributed path is the defect
whether or not the attributed one also runs.

### A.10 Ingestions domain — `domains/ingestions/` (BUILT — 8 routes, additive)

Backs the SPA's `/api/ingestions/*` surface (`src/shared/api/ingestions-client.ts`,
`VerevonIngestionsPage`): scrape/crawl/extract/batch runs, schedules + lifecycle, browser profiles,
web sources, and the per-run evidence timeline. Port of verevonv2's `app/api/ingestions/` BFF.

> **Upstream is quarry-EDGE (`QUARRY_EDGE_URL`, `:8082`), NOT quarry-control.** This is the one
> trap in this domain (it cost us a wrong first pass — see the ⚠ below). Edge is the JWT face of
> Quarry-v2: it owns the execution + profile endpoints and forwards the durable registry to
> quarry-control over HMAC internally. The gateway cannot sign HMAC and must never reach control
> directly — consistent with §B.2 ("quarry-control stays gateway-internal") and §E's `none` bucket
> ("quarry-control direct"). Auth is the minted `quarry` audience token; edge derives org from the
> token's `claims.org_id`, so this domain never reads `x-verevon-org-id`.
>
> *Known exception (do not copy):* the onboarding crawl handlers (`onboarding/crawl_preview`,
> `onboarding/actions/website`) post to control `/v1/jobs/` directly. That works only because control
> currently runs in HMAC **rollout mode** (unsigned = trusted on the private network); it has no org
> scoping and breaks under `QUARRY_INTERNAL_HMAC_REQUIRED=1`. New consumer features go through edge.

| Gateway route (SPA paths — no `/v1/`) | Upstream (quarry-edge, `quarry` bearer token) |
|---|---|
| `GET /api/ingestions/runs` | fan-out `GET /v1/{crawl,extract,search,agent,batch}/jobs` → `RunItem[]` |
| `POST /api/ingestions/runs` | by kind: crawl→`/v1/crawl`, batch→`/v1/batch`, extract→`/v1/extract`, else→`/v1/scrape` (each URL SSRF-normalized) → `RunCreateResult` |
| `GET /api/ingestions/schedules` | `GET /v1/schedules` → `ScheduleItem[]` |
| `POST /api/ingestions/schedules` | `POST /v1/schedules` → `ScheduleItem` |
| `POST /api/ingestions/actions` | `POST /v1/schedules/{id}/{pause,unpause,trigger,backfill}` / `DELETE /v1/schedules/{id}` |
| `GET /api/ingestions/sources` | `GET /v1/sources` (web) + integration-core connections (+discovery) + documents-api + graph-index → `SourcePayload` |
| `GET /api/ingestions/profiles` | `GET /v1/profiles` + per-id `POST /v1/profiles/{id}/restore_probe` → `ProfilePayload` |
| `GET /api/ingestions/evidence?runId=` | `GET /v1/runs/{id}/events` → `EvidenceTimeline` |

The `sources` cross-plane fan-out (integration-core / documents-api / graph-index) is org-scoped via
session-context (`authorized_org_id`, the billing.rs pattern), never a client header. No new
`config.rs` keys — reuses the existing `QUARRY_EDGE_URL`.

> ⚠ **Trap (cost us a wrong first pass):** quarry-control (`:8081`) reads like "the ingestion
> registry" and tempts a direct wire, but it is **HMAC-only, has no JWT/org context, and lacks the
> `/v1/scrape|crawl|extract|batch` execution + `/v1/profiles/{id}/restore_probe` endpoints**.
> Pointing at it 404s run-creation/profiles, silently drops org scoping, and 401s once
> `QUARRY_INTERNAL_HMAC_REQUIRED=1`. Always use `QUARRY_EDGE_URL`. Canonical role split:
> `apps/Ingestion Plane/Quarry-v2/docs/ARCHITECTURE.md` → "Access rule (who may call what)".

---

**Route count summary: 20 EXISTS (1 reworked) + 133 ADD = 153 total** *(+ §A.9 Search: 5 routes built
this session, additive; + §A.10 Ingestions: 8 routes built, additive — both outside the original 153).*
Per phase: P1 +17, P2 +10, P3 +26 (24 knowledge + 2 actions), P4 +40, P5 +40.

---

## B. Auth & session handling at the gateway

Current state (must change): no inbound validation; actor from `x-session-user-id/...` proxy
headers; dev headers behind `ALLOW_DEV_ACTOR_HEADERS`; silent fallback actor `verevon-v3-local-user`.

### B.1 Session model

1. **Credential = Better Auth session cookie**, issued by auth-core, surfaced to the browser via the
   gateway's `/api/v1/auth/sign-in|sign-up` (Set-Cookie passthrough, `HttpOnly`, `Secure`,
   `SameSite=Lax`; cookie domain shared between SPA origin and gateway, or gateway-scoped with
   `credentials: 'include'` + CORS). The SPA never sees a token.
2. **Validation middleware** (new `auth/middleware.rs`, applied to everything except `/health`,
   `/api/v1/auth/sign-up|sign-in|password/*|oauth/initiate`): validate the forwarded cookie via
   auth-core — start with HTTP `POST /api/v2/auth/getSession`, optimize to gRPC
   `auth.v1.TokenValidationService.ValidateToken` (:50011) later. Cache validations in-process with
   exp-based TTL (mirror of v2 `onboarding-proxy.ts` `mintAudienceToken()` caching).
3. **Org resolution**: user-core `GET /api/v1/me/session-context` → `active_org_id`; cache per
   session; invalidate on `organization/switch-active` and `session/refresh`. Every org-scoped
   upstream call gets the **gateway-derived** `x-org-id`. For `:id`-parameterized org routes, enforce
   membership (org list from auth-core `POST /api/v2/auth/organization/list` or org-core) before
   proxying — billing-core and audit-core trust caller-supplied org blindly, so the gateway is the
   only line of defense.
4. **Audience tokens**: for model-gateway and quarry-edge (both JWKS-validating), mint per-user
   plane tokens via auth-core `GET /api/{audience}/token` (audiences `model-plane`, `quarry`,
   `data-plane` for retrieval-engine JWT mode), cached until exp. Tokens live only in gateway memory.
5. **Dev mode**: keep `ALLOW_DEV_ACTOR_HEADERS` but make it fail-closed in release builds
   (`#[cfg(debug_assertions)]` or explicit `VEREVON_ENV=dev` check) and **remove the silent
   `verevon-v3-local-user` fallback** — unauthenticated requests get 401
   `{error:{code:"UNAUTHENTICATED"}}`, never a default actor.

### B.2 What must never reach the SPA

Per repo rule ("BFF routes must not leak upstream secrets or raw OAuth tokens"):

- `INTERNAL_API_KEY` / per-service keys (`FINSPO_API_KEY`, imports key) — outbound headers only.
- `QUARRY_INTERNAL_SECRET` / HMAC `X-Quarry-Sig*` material (quarry-control stays gateway-internal).
- `GOOGLE_WEB_RISK_API_KEY` and `URLSCAN_API_KEY` — server-side security connector secrets only.
- Minted audience JWTs (model-plane/quarry/data-plane) — never echoed in responses.
- Raw provider OAuth tokens from integration-api (only connect-session ids/status pass through).
- Upstream error bodies that embed internal hostnames/keys — map to the typed error envelope;
  log details server-side with a correlation id.

### B.3 ZDR + headers

- Accept `x-zdr: true` (or body flag) from the SPA on content-carrying routes (chat stream, chat
  documents, voice/audio/transcript routes, crawls, imports) and propagate to upstreams that persist
  content (quarry-edge scrape `zdr` flag, model-gateway invoke/speech, imports). Default comes from
  org entitlements; the gateway enforces, the SPA only requests.
- Strip ALL inbound `x-user-*`/`x-org-id`/`x-internal-*` headers from browser requests before
  building upstream headers (today a spoofed `x-org-id` would be forwarded on most Data Plane
  services — cross-org read). This is the single most important security fix in the program.
- CORS: `VEREVON_ALLOWED_ORIGINS` must list the real SPA origins per environment (dev
  `http://localhost:5173`, `http://127.0.0.1:5173`), `Access-Control-Allow-Credentials: true`,
  methods extended to include PATCH/DELETE (current default is GET/POST/PUT/OPTIONS only — PATCH
  routes in Phases 4–5 will fail CORS without this).

---

## C. v3 client-side changes

### C.1 Transport layer (fills the empty `src/shared/api/`)

| New file | Contents |
|---|---|
| `src/shared/api/config.ts` | `gatewayBaseUrl()` from `VITE_VEREVON_GATEWAY_URL` (fallback `http://127.0.0.1:3185`), env typing in `src/vite-env.d.ts`; single source — `src/features/onboarding/lib/api.ts` migrates onto it |
| `src/shared/api/http.ts` | `requestJson<T>()` with `credentials:'include'`, typed envelope parsing (`{data}` / `{error:{code,message,details}}`), error coalescing (`message || \`Request failed (${status})\``), retry/backoff for idempotent GETs |
| `src/shared/api/sse.ts` | fetch-based SSE reader (POST-capable — EventSource is GET-only and the chat/crawl streams are POST), multi-`data:`-line join per spec, per-event runtime guards, `Last-Event-ID` resume support, AbortController cancel |
| `src/shared/api/auth-client.ts` | sign-up/in/out, session, verification, password flows → `/api/v1/auth/*`, `/api/v1/me*`, `/api/v1/session/*` |
| `src/shared/api/chat-client.ts` | `streamChat()`, resume, cancel, thread history, models, upload, feedback |
| `src/shared/api/knowledge-client.ts` | documents/sources/search/wiki/imports/crawls |
| `src/shared/api/settings-client.ts` | settings sections, preferences, orgs, billing, integrations |
| `src/shared/api/security-client.ts` | URL reputation checks and URL investigations through gateway-owned Web Risk/urlscan connectors |
| `src/shared/api/inbox-client.ts` | inboxes/conversations/ai-actions |
| `src/shared/api/agents-client.ts` | capabilities/tasks/cron/memory/skills/orchestration |
| `src/shared/api/notifications-client.ts` | feed/counts/marks/preferences |

### C.2 Auth/session state + route guards

- `src/shared/session/session-store.ts`: Solid store holding `{user, activeOrg, entitlements, status}`,
  hydrated from `GET /api/v1/session/current` on boot; replaces BOTH identity stubs
  (`demoActor` `user_demo_operator`/`org_triodelab` in `src/shared/mocks/verevon-operating-model.ts`
  and the gateway dev actor in `src/features/onboarding/lib/api.ts`).
- Route guards in `src/app/App.tsx`: unauthenticated → `/login`; authenticated without org/onboarding
  → `/onboarding`; otherwise workspace. `src/features/auth/components/AuthPage.tsx` `completeAuth()`
  switches from blind `navigate('/onboarding')` to real `auth-client` calls + session-context routing.
- Dev actor headers: keep behind `import.meta.env.DEV && VITE_ALLOW_DEV_ACTOR_HEADERS`, delete the
  localStorage-actor path once cookie sessions work.

### C.3 Mock replacement map (`src/shared/mocks/verevon-operating-model.ts` shrinks per phase)

| Mock export | Replaced by | Phase |
|---|---|---|
| `conversationTurns` | chat-client SSE stream + thread history | 2 |
| `knowledgeSources`, `fetchKnowledgeGraphSnapshot()` | knowledge-client (documents/sources/wiki; graph stays mock until graph view ships) | 3 |
| `graphrest-client.ts` hardcoded nodes | gateway `GET /api/v1/knowledge/graph` | 3 |
| `workflowPolicies` | settings-client (+ AI settings section); model routing stays server-owned | 4 |
| `inboxItems` | inbox-client | 5 |
| `agentRoles` | agents-client (skills/capabilities) — pending owner decision | 5 |
| `operatingMetrics`, `liveRuns` | dashboard composites (runs SSE, sources, integration profile, usage-summary) | 5 |
| `getVisibleItemsForSurface()` context rail | live per-surface state as each surface lands | 2–5 |

### C.4 Action execution

`src/shared/actions/agent-tools.ts` is the AI-first bridge: the same `ActionDescriptor` entries used by
UI controls are converted into model-visible tool contracts with JSON-schema parameters, owner-plane,
risk, approval, and reversibility metadata. The chat AG-UI client sends those tools as TanStack
`RunAgentInput.tools`, with operational options in `forwardedProps`; the gateway sanitizes and maps
that body back to Model Plane `/v1/invoke/stream`.

`src/shared/actions/action-client.ts` stops synthesizing `run_*`/`audit_*` via `crypto.randomUUID()`
and POSTs to `/api/v1/actions/:action_id/execute`; the returned `eventStream` URL becomes the real
`/api/v1/action-runs/:run_id/events`. The `ActionDescriptor` contract (owner plane, risk, approval,
reversibility, Zod schemas in `src/shared/actions/action-registry.ts`) is kept verbatim — the
gateway mirrors the validation server-side.

---

## D. Phased rollout

### Phase 1 — Onboarding + Auth end-to-end

**Goal:** real signup → email verify → session → profile → org (BRREG) → website/connect →
plan/trial-or-checkout → theme → complete → workspace, with zero dev-actor reliance.

**Endpoint sequence (the contract to make green end-to-end):**
`POST /api/v1/auth/sign-up` → `POST /api/v1/auth/email-verification/send` → `.../verify` →
`GET /api/v1/auth/session` → `GET /api/v1/me/session-context` → `GET|PUT /api/v1/onboarding/state` →
`PATCH /api/v1/me` → `GET /api/v1/onboarding/brreg/search` →
`POST /api/v1/onboarding/actions/create-organization` → crawl-preview SSE →
`.../start-website-ingest` → `.../start-connect-session` → poll connect-session →
`.../discover-source` → `.../start-integration-sync` → `POST /api/v1/onboarding/recommend-plan` →
`.../set-plan` → trial: `PUT /api/v1/billing/account` | paid: `.../start-checkout` →
`PUT /api/v1/onboarding/theme` → `POST /api/v1/onboarding/complete` →
`POST /api/v1/session/refresh` → navigate `/dashboard`.

**Gateway tasks (Rust, `verevon-gateway-rs/`):**
- `src/auth/middleware.rs` (session validation, caching), `src/auth/audience_tokens.rs` (mint+cache),
  `src/domains/auth.rs` (15 routes), session routes (2), wire upstreams `AUTH_CORE_URL`,
  `SESSION_CORE_URL`, `BILLING_CORE_URL` into config.
- Harden onboarding domain: auth middleware on all routes; replace 200-masking with typed errors;
  strip inbound identity headers; remove default-actor fallback; CORS PATCH/DELETE.
- Billing trial: `PUT /api/v1/billing/account` invoked on trial selection (closes the pending
  14-day-trial lifecycle gap from the v2 port; billing-core's server-side expiry sweep handles the rest).

**v3 tasks:** `src/shared/api/{config,http,sse,auth-client}.ts`, `src/shared/session/session-store.ts`,
route guards in `src/app/App.tsx`, rewire `src/features/onboarding/lib/{api,actions,persistence,state,queries}.ts`
onto shared transport, real AuthPage submit.

**Audit fixes folded in (from the confirmed onboarding audit):**
- *Correctness (must-fix while touching the flow):* finishOnboarding no longer swallows completion
  failure / clears state only after success (`OnboardingPage.tsx`); inspect `allSettled` results in
  connectSource and surface per-connector failure; validate localStorage/server snapshots at the
  boundary (`state.ts`); runtime-guard SSE payloads + multi-line data join (`api.ts`); requestJson
  error-message coalescing; URL validation before crawl-preview; downgrade persisted in-flight crawl
  statuses on hydrate; guard duplicate org creation on back-then-continue; remove hardcoded demo
  credentials in `AuthPage.tsx`; checkout popup result handling (blocked-popup check, dedicated
  callback page, `history.replaceState`); step-dot navigation restricted to visited steps + error
  rendering on paywall (`role="alert"`); clear stale errors on step change.
- *Performance (cheap while in the files):* shallow-spread persistence snapshot + debounced
  serialization with `untrack()` (`persistence.ts`); `batch()` multi-setState handlers; stable
  plan-recommendation queryKey; `Promise.all` in commitPlan; fire-and-forget website ingest;
  skip first post-hydration PUT echo; cache parsed dev actor; `useNavigate('/dashboard')` instead of
  full reload; memoize `buildModelContextPack` in `AppShell.tsx`.
- *Deliberately deferred to a UI pass:* the remaining uiux/reuse items (focus management, contrast,
  reduced-motion, component extraction) — tracked, not blocking integration.
- Wire the currently-dead `fetchSessionBootstrap` on mount (or delete it with `fetchOnboardingStatus`/
  `cleanupSource` if superseded by `session-store`).

**Tests:** gateway `cargo test` — middleware unit tests (cookie→401/200, header stripping, org
membership denial), per-route handler tests with mocked upstreams (wiremock), envelope-normalization
tests. v3 Vitest — auth-client/session-store/sse parser units, onboarding controller flow with mocked
fetch, guard redirects. One Playwright smoke (`tests/e2e/onboarding.spec.ts`) against compose stacks.

**Risks:** auth-core enhanced routes have placeholder corners (consent, email mock in dev) — pin to
the routes the sequence needs; cookie domain/CORS mismatch in deployed envs (needs an env matrix);
gateway `discover-source`/`cleanup-source` upstream parity with v2 BFF unverified — verify first.

### Phase 2 — Chat / Model Plane streaming

**Gateway:** `domains/chat.rs` (9 routes); SSE re-stream verbatim (no buffering — axum
`Body::from_stream`); forward `Last-Event-ID`; model-plane audience token injection; ZDR flag pass-through.
**v3:** `chat-client.ts`; replace `conversationTurns` in `ChatPage`; streaming renderer keyed on the
current event set (`connected/message/done/error`) with additive handling for the target taxonomy
(`delta`, `citation`, `usage`, …) so backend upgrades need no client release; stop/cancel + resume UX;
thread history on mount; model picker.
**Tests:** cargo — SSE proxy integration test (mock upstream emitting event sequences incl.
disconnect/resume); Vitest — sse.ts parser (chunk splits, multi-line data, ids), chat store reducer.
**Risks:** model-gateway today emits only `connected/message/done/error` — do not promise richer UI;
usage/confidence must come from real `usage` events (no placeholders, per parity constraints);
persistence/resume gaps are Model Plane 🔴 items — chat history depends on `/v1/threads/:id/messages`
being durable (verify against Model Plane v1, the canonical target).

### Phase 2B — Voice dictation and transcription

**Gateway:** `domains/voice.rs` (11 routes); enforce effective voice mode from org/user policy;
propagate `x-zdr`; reject cloud providers when org policy requires `company_private`; emit
metadata-only audit. **v3:** replace the current browser-only voice modal in `DashboardComposer.tsx`
with `src/features/voice/*`; add mode badge, explicit discard/insert, command preview, and Teams
transcript import panel. **Model Plane:** reuse `/v1/ai/speech` for MVP STT, then add streaming STT,
`voice_format`, `voice_command`, and `company_private` provider routing. **Tests:** gateway policy
matrix, ZDR propagation, no raw audio/transcript logs, local-provider egress-blocked smoke, browser
dictate/cancel/insert E2E.

### Phase 3 — Knowledge / Data Plane

**Gateway:** `domains/knowledge.rs` (24 routes) + `domains/actions.rs` scaffold (2 routes,
`knowledge.recrawl_source` dispatch live); upstream config for `DOCUMENTS_API_URL`,
`RETRIEVAL_ENGINE_URL`, `WIKI_STORE_URL`, `IMPORTS_API_URL` (+ existing graph-index/quarry);
strict org-injection (wiki-store/graph-index trust `X-Org-ID` outright); multipart proxy for imports.
**v3:** `knowledge-client.ts`; replace `knowledgeSources` + `graphrest-client.ts`; knowledge page
search (knowledge/search + wiki/graph arms), wiki browser (sidebar list, page view, versions/diff/
backlinks), import flow with job-event progress, recrawl action through the real action executor.
**Tests:** cargo — org-injection tests (spoofed inbound header never forwarded), multipart proxy,
action dispatch + audit emission; Vitest — knowledge store, citation expansion, import progress.
**Risks:** Data Plane freshness gap (no `documents.updated` publisher) — add polling/refresh
affordances, do not promise live updates; retrieval-engine schemas not fully documented — verify
handler structs before typing the client; wiki RAG silently degrades if wiki-store NATS_URL unset.

### Phase 4 — Settings / Org / Billing / Notifications / Integrations

**Gateway:** `domains/{settings,orgs,billing,notifications,integrations}.rs` (40 routes); org-membership
enforcement on `:id` routes; integration-api envelope normalization (`{success,...}` → `{data}/{error}`);
entitlement-aware error mapping (pro-plan gate on connect-session → `{error:{code:"PLAN_REQUIRED"}}`).
**v3:** `settings-client.ts` + `notifications-client.ts`; replace `workflowPolicies`
(workflow policies → settings-backed interim store until a dedicated policy service
exists — `workflows.toggle_policy` action routes there). Model routing is configured
through the server-owned Router Policy surface, not a browser-side tier selector;
org/member/role
management UI; billing panel (account, entitlements, quotas, upgrade checkout); notification bell +
preferences; integrations management (connections, sync jobs, reconnect).
**Tests:** cargo — envelope normalization fixtures per upstream, membership-denial matrix; Vitest —
settings forms (optimistic update + rollback), notification badge polling.
**Risks:** no dedicated backend owner for workflow policies (interim = user-core settings; flagged as
tech debt); notification-core per-route auth granularity undocumented — verify in Go source first;
org-core RBAC only on compat `/orgs/*` family (watch for migration to `/api/v1`).

### Phase 5 — Inbox / Agents / Dashboard composites / Actions completion

**Gateway:** `domains/{inbox,agents,dashboard}.rs` (40 routes); finish `actions.rs` dispatch for
`inbox.draft_reply` (conversation-core ai-action loop) and `workflows.toggle_policy`;
`agents.deploy_channel` returns 501 until Channel Plane exists; normalized action-run event stream.
**v3:** `inbox-client.ts`, `agents-client.ts`; replace `inboxItems`, `agentRoles`, `operatingMetrics`,
`liveRuns`; dashboard composes runs SSE + sources + integration profile + usage-summary; AI context
rail (`AppShell`) fed by live per-surface state; action approval UX wired to real approval state
(model-gateway orchestration approvals + conversation-core ai-actions).
**Tests:** cargo — action fan-out per owner plane (mocked), SSE normalization across three run-event
sources; Vitest — dashboard composition, inbox flows, agents CRUD. Playwright smoke for
inbox→draft→approve.
**Risks:** conversation-core endpoints were doc-sourced, not source-verified — re-verify `server.go`
before codegen; no backend owner for agent-role config (decide: skills/capabilities vs. new store);
audit/usage numbers depend on NATS ingest being healthy (best-effort, drops malformed events);
cost-core spend numbers are in-memory/placeholder — exclude from dashboard until durable.

---

## E. Non-goals (explicit)

- **Channel Plane**: docs-only, no runtime — `agents.deploy_channel` stays 501; no gateway routes.
- **Everything tagged `later` in `endpoint-map.md`**: 2FA/OTP/passkey/api-keys (placeholder-backed),
  admin surfaces, multimodal `/v1/ai/*`, realtime (placeholder), fine-tune, graph-index workspace
  views beyond the snapshot, wiki authoring/proposals, document delete UX, finspo
  analytics/proposals, quarry schedules/change-tracking/team routes, autocomplete-core (deployment
  unverified), audit/usage dashboards beyond Phase 5 summary, cost-core spend, Convex realtime
  (WS-proxy decision deferred), session-core legacy `/v1/sessions/*`.
- **Everything tagged `none`**: inter-plane internals, quarry-control direct, documents-api writes,
  Model Plane sibling cores, conversation-ingest, convex `:3211` webhooks (stale/broken),
  zammad-foundation, defined-but-unhosted protos (`RunService`, `EventLog`, `BrowserAgentService`),
  affine-core (missing build context).
- No new embeddings/reranking outside Data Plane; no direct DB access; no SPA-held secrets — ever.
- No WebSocket surface (none exists upstream today); all streaming is SSE.
