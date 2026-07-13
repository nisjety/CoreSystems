# conversation-core-go

> **2026-07-13 superseding update.** Postgres is healthy in the current runtime; every July 11 corruption/unhealthy/“only blocker” statement below is historical. Changed source now uses exact Auth Core membership through User Core, signed tenant/user/role delegation, role gates, fail-closed ingest, and `/notes` as the only store-only path. External replies require a stable idempotency key, claim a content-free outbound intent, and issue a 30-second Ed25519 proof bound to the durable human intent or approved AI action, verified actor, tenant, connection, provider operation, exact payload digest, and idempotency key. Integration receipts add a second single-use claim and reject opaque `approvalId`/legacy writes. Provably pre-provider failures use retryable/pending phases, accept a fresh valid JTI only for the identical pending effect, and `sending`/`executing`/`unknown` never blindly retransmit. Conversation has 293 tests; changed critical functions measure 83.3–100%, while broader legacy packages remain below the plane-wide gate. None of the new images, keys, credentials, or SQL migrations is deployed. Release blockers are provisioning/deployment, real-Postgres migration execution, stale-state reconciliation, provider delivery callbacks, durable approval-event publication, authoritative ZDR, HA replay state, sandbox identities, and Model health.

_Audit refresh: 2026-07-11. Supersedes the 2026-07-02 draft, which listed a much
smaller route surface and predates the Inbox provider-messaging build._

Evidence grades: **[live-curl]** = observed against the running container on
`:3160`; **[source-only]** = read from source/config on disk; **[inspect]** =
`docker inspect` / `git` / build tooling. Docker `exec`/`build`/`logs` are
unavailable this pass (host containerd content store is corrupted), so nothing
here relies on them.

## What it is

`conversation-core-go` is THE first-party Inbox / support-conversation backend of
the Application Plane (Zammad is legacy foundation, not the runtime). Velion v3's
Inbox proxies to it through the gateway; if it is down the gateway 502s. It owns
conversations, messages, tickets and the ticketing workbench (views, macros,
automation rules, SLA policies, checklists), plus the HITL AI-action review queue.

It is a live, honest service — **no runtime stubs, mocks, placeholders, TODOs, or
`panic()`s exist in the Go source** (the only `fake` match is a doc comment about
a test-injected pool). [source-only]

- Main: `conversation-core-go/cmd/server/main.go`
- Routes: `conversation-core-go/internal/http/server.go`
- Service: `conversation-core-go/internal/conversation/service.go`
- Repository (org-scoped SQL): `conversation-core-go/internal/conversation/repository.go` (2141 lines)
- Outbound client to integration-corev2: `conversation-core-go/internal/integration/client.go`
- NATS consumers: `conversation-core-go/internal/consumers/{ai_action_executor,model_action_proposed_consumer,webhook_received_consumer}.go`

## Live health & runtime state

### Historical July 11 evidence — not current state

- `GET /health` → **200** `{"service":"conversation-core-go","status":"ok"}`. `/healthz` → 404 (only `/health` + `/ready` exist). [live-curl]
- Container `conversation-core-go` and `conversation-ingest-rs` both report `Up 2 days (unhealthy)`. "unhealthy" = the exec-based healthcheck cannot run under the containerd corruption, NOT the service — HTTP health is 200. [inspect]
- Runtime env: `INTEGRATION_BASE_URL=http://integration-api:3026`, `INTERNAL_API_KEY` set (64-hex, not the `change-me` template default), `VELION_NATS_URL=nats://velion-nats:4222`, `DATABASE_URL=…@application-postgres:5432/application_plane`. On `app-net` + `inter-plane-bus`. [inspect]
- Because `INTEGRATION_BASE_URL` + key are both set, `DraftReplySendEnabled()` is TRUE → outbound-send (human replies AND the draft.reply act-leg) is ENABLED at runtime. [inspect]

### CRITICAL (live, environmental): datastore is down — every DB-backed read 500s

- Auth + input validation work: `/api/v1/*` with no key → **401**; with key but no org → **400 `missing_org_id`**. [live-curl]
- But every DB-backed read — `/api/v1/inboxes`, `/conversations`, `/tickets`, `/ai-actions`, `/sla-policies` — returns **500 `internal_error`** with a valid key + org. [live-curl]
- Root cause (confirmed via a throwaway pgx probe against `:9540`, since removed): `application-postgres` is failing with `FATAL: could not open file "global/pg_filenode.map": I/O error (SQLSTATE 58030)`. The same host storage/containerd corruption has reached the Postgres data directory. [live-curl/probe]
- Interpretation: conversation-core's **code is healthy** (it booted, ran migrations, serves `/health`, enforces auth), but its datastore is currently unreadable, so the Inbox cannot serve real conversations/tickets right now despite `/health=200`. This is an infra outage, not a service defect. It also blocks live end-to-end verification of the reply-send path (no conversation/thread ref can be created).

## Exposed surface (changed source uses signed delegation and role scope) [source-only]

Health: `GET /health`, `GET /ready`.

Conversations: `GET /api/v1/inboxes`, `GET /inboxes/:id/queue`, `GET /conversations`,
`GET /conversations/:id`, `POST /conversations/search`,
`POST /conversations/:id/ticket-classifications`, `POST /conversations/:id/messages`,
`POST /conversations/:id/notes`, `PATCH /conversations/:id/status`,
`PATCH /conversations/:id/assignment`, `POST /conversations/:id/tags`,
`DELETE /conversations/:id/tags/:tag`.

Ticketing workbench: `GET/POST /tickets`, `GET/PATCH /tickets/:id`,
`POST /tickets/:id/links`, `POST /tickets/:id/macros/:macro_id/run`,
`POST /tickets/:id/checklists`, `PATCH /tickets/:id/checklists/:cid/items/:iid`,
`GET/POST /ticket-views`, `PATCH /ticket-views/:id`, `GET/POST /ticket-macros`,
`PATCH /ticket-macros/:id`, `GET/POST /ticket-automation-rules`,
`PATCH /ticket-automation-rules/:id`, `GET/POST /sla-policies`, `PATCH /sla-policies/:id`.

AI actions (HITL): `GET/POST /ai-actions`, `POST /ai-actions/:id/{review,approve,reject}`.

Internal: `POST /internal/conversation-events` (ingest), `GET /internal/conversations/:id/projection`,
and internal mirrors of the ai-actions routes.

Auth: changed source verifies HMAC-v2 delegation bound to service, audience, method, URI,
body, user, organization, role, timestamp, and nonce. `velion-gateway` may use `/api/v1`
with reader/agent/admin role gates; `conversation-ingest` may use only its internal event
route. Scope comes from the verified principal, never query parameters. Body cap is 2 MiB.

## Re-verified prior findings

- **PROD BUG "WhatsApp/Messenger Inbox replies silently never send" — FIXED** (commit `f007642b`). [source-only]
  `Service.AddMessage` now **sends first, then persists**: for a non-internal outbound reply to a
  channel with a real send op, `deliverReply` resolves the channel thread ref, calls
  integration-corev2 `POST /api/v1/connections/{id}/actions`, and only on success stores the row.
  A send failure returns `ErrSendFailed`, which the handler maps to **HTTP 502 `send_failed`** — no
  phantom "Reply sent", no stored-but-undelivered row. Real send ops exist for whatsapp / messenger /
  instagram / slack / google (gmail) / microsoft (graph); discord is honestly rejected as unsupported;
  store-only channels (plain email, no thread ref) stay store-only without a false claim.
  (Live provider send NOT verifiable this pass — datastore down.)
- **conversation-ingest-rs `/internal/ingest/email` fail-open — FIXED.** [source-only + cargo check]
  `require_internal_key` runs as middleware BEFORE body deserialization; an empty configured key
  rejects everything unless `ALLOW_INSECURE_DEV_DEFAULTS=1`, and `main.rs` refuses to boot keyless
  otherwise. Constant-time compare. Unit tests cover 401 (missing/wrong key) and 422 (valid key →
  reaches validation). Runtime key matches conversation-core's. `cargo check` exit 0.
- **HITL is REAL, not decorative.** [source-only] `AIActionExecutor` consumes
  `ai_action.reviewed`; only `decision=="approved"` executes; an atomic approved→executed claim
  (`MarkAIActionExecuted`, keyed by action id) makes redelivery idempotent (no double-send /
  double-promote). `draft.reply` sends via integration with terminal-vs-transient classification
  (terminal → ack + `ai_action.send_failed`; transient → unclaim + retry). `ticket.classification`
  promotes the suggested ticket + applies routing. `CreateAIAction` enforces a kind allowlist
  (only `draft.reply`). Migration `003` adds a `UNIQUE(org_id, ai_action_id)` send-audit table as a
  second-layer dedup.
- **ZDR tripwire present.** [source-only] `model_action_proposed_consumer` fails closed on any
  restrictive ZDR marker (`zdr`/`zdr_mode`/`zdr_classification`/`ephemeral_only`; unknown values
  treated as restrictive) because conversation-core has no ZDR propagation yet. Honest guard
  (commit `a40aacf1`).
- **IDOR-clean.** [source-only] Every repository query is `WHERE org_id = $1`; org comes from the
  `x-org-id` header only (never request body). The v3 gateway `inbox.rs` sets that header from the
  **authenticated session's** org (`authorized_org_id`), not a client-supplied header/query/body —
  closing the `x-velion-org-id` cross-tenant IDOR flagged in the AI-First audit. A foreign action/
  conversation id simply misses `org_id = $auth AND id = $id` → 404.

## Gateway wiring & the Zammad question [source-only]

`apps/Frontend Plane/velionv3/apps/gateway/src/domains/inbox.rs` is a thin proxy: `/api/v1/inbox/*`
→ conversation-core `/api/v1/conversations|inboxes|ai-actions`, forwarding internal key + session org
+ actor headers via `proxy_json`. **There is NO `ConversationSummary`→`ZammadTicket` mapping in v3** —
that was a velionv2-era concept; v3's Inbox is native conversation-core. The only Zammad in the v3
gateway is a separate optional "support actions" helper (`domains/agents.rs`: agents/groups/macros)
that returns "not configured" unless `ZAMMAD_API_TOKEN` is set. `zammad-foundation` is legacy.

## Inbound bridges [source-only]

- `conversation-ingest-rs` (Rust, `:3161`): Gmail/Outlook (and any normalized email) → `POST /internal/ingest/email` → normalizes → conversation-core `/internal/conversation-events`.
- `webhook_received_consumer` (Go): integration-corev2 `velion.ingestion.integration.webhook_received` → fetches full payload → normalizes WhatsApp / Messenger / Instagram / Slack inbound messages into stored conversations. Delivery/read/status callbacks and bot echoes are skipped. Composite thread refs (`businessId:recipientId`) carry the reply address for the outbound send op.

## Storage & migrations [source-only]

Shared `application-postgres` (`application_plane` DB, `appuser`), no cross-plane DB crossing.
Migrations are `//go:embed`-ed, applied transactionally at boot, and tracked in
`conversation_core_schema_migrations`: `001_conversation_core` (inboxes/conversations/contacts/
messages/idempotency/tags/audit), `002_conversation_ticketing` (tickets/views/macros/automation/
SLA/checklists/classifications), `003_ai_action_send_audit` (legacy send audit), and
`004_outbound_intents` (tenant-scoped content-free outbound state/idempotency), and
`005_outbound_authorization_binding` (authorization kind, durable actor/action IDs,
operation, payload digest, and retryable pre-provider phase). Migrations 004/005 have
query/contract tests but have not been applied to a real database in this audit.

## Build & WIP status

- `go build ./...` exit **0**; `go vet ./...` clean. `cargo check` (ingest-rs) exit **0**. Toolchain: go 1.26.2, cargo 1.94.1. [inspect]
- The monorepo is intentionally dirty with extensive pre-existing/user work plus this audit's
  source/test changes. No commit, cleanup, reset, or deployment was performed.

## Bottom line

Conversation-core is not release-ready. The current Postgres and old service container are healthy,
but their revision does not prove these changes are live. Source now contains honest send failures,
signed scope, production bearer, durable effect-bound approval attestation, idempotency, phase-aware
pre-provider retry, unknown outcomes, and enforced HITL regressions. MVP still requires key/credential
provisioning, a coordinated Conversation/Integration cutover, authoritative ZDR, real-DB migrations,
stale-state reconciliation, delivery callbacks, durable approval-event dispatch, authorized provider
E2E, and safe paired rollback.
