# Velion — Phase 2 Execution Progress & DoD Evidence

> Companion to `docs/PHASE_2_PLAN.md` (authoritative spec). One section per PR with
> DoD evidence (commands + result) and the no-new-fakeness proof. Base: `main` @ Phase 1
> merged (`15704f00`). Execution started 2026-06-20.

## Gate status (PR-0 prerequisite)
- **Phase 0** (CI gate, IDOR fix, de-fake): merged to `main` (`ef1e57af`, `ff2a3f59`, `b269d09a`).
- **Phase 1** (HITL, GDPR, insights, change-monitoring): merged to `main` (`15704f00`); in-flight
  cc-go Track-B work committed (not clobbered) via `5d633954`/`15704f00`.
- **Phase 1 green verification** (PR-0): see PR-0 below.

## W2 day-15 go/no-go (recorded per PR-0)
- **Decision date: 2026-07-05** (day 15 after the 2026-06-20 Phase-1 merge / Phase-2 start).
- **Criterion:** PR-4 (W2 recurring monitoring) proceeds **only if** Phase-1 Track C
  (C1 edge rebuild on `--features postgres-queue` + `QUARRY_EDGE__DATABASE_URL`, C2 gateway
  `monitoring.rs`, C3 SPA Monitoring tab) is **merged-and-green** AND a live
  `POST /v1/change/check` returns **200, not 501**. Otherwise W2 defers wholesale to Phase 3 —
  nothing half-built ships.
- **Cut order if the window compresses:** W1 → W3 → W2. **Never** cut W4 or E5.

---

## PR-0 — Reconcile Phase-1-B, confirm Phase 1 green, re-baseline + IDOR CI lint
**Status:** in progress.

DoD checklist:
- [x] In-flight cc-go Track-B work committed (not lost) — on `main` via `15704f00` (`internal/conversation`
      `repository.go`/`service.go`/`types.go`/`handlers.go` + `repository_test.go`/`service_test.go`).
- [x] No-client-header-org **CI lint** strengthened in `.github/workflows/velionv3-ci.yml`
      `fabrication-guard`: bans `fn org_id_from_headers`, asserts `x-org-id` + `x-velion-org-id`
      stay `STRIPPED_HEADERS` entries, and bans any `domains/` read of a client org header
      (`get("x-(velion-)?org-id")`). **Proven red on a deliberate violation** (probe domain reading
      `x-velion-org-id` + reintroduced helper → exit 1; STRIPPED_HEADERS removal → fail). Clean tree passes.
- [x] Phase 1 merged-and-green — verified green across all four stacks (evidence below).
- [x] W2 day-15 go/no-go date on record (2026-07-05, above).

Green evidence (commands run on `main`):
- cc-go: `go build ./...` + `go test -race ./...` → green (`internal/conversation` ok; 0 FAIL/panic).
- user-core: `go build ./...` + `go test ./...` → green (grpc/http/users ok; 0 FAIL).
- gateway: `cargo fmt --check` ok; `cargo test` → 116 passed; `cargo clippy --all-targets -- -D warnings`
  was **red** on `browser.rs:549` (`clippy::unnecessary_lazy_evaluations`) → fixed
  (`unwrap_or_else(|| <const struct>)` → `unwrap_or(<const struct>)`); re-run **green**.
- SPA: `pnpm verify` (typecheck → lint → test → build) → green (41 files / 156 tests; built in 5.51s)
  after isolating the dev-bypass test.

---

## PR-1 — W4 ticket.classification executor (THE SPINE)
**Status:** complete + live-verified.

What shipped:
- `internal/consumers/`: a reusable `DurableConsumer` scaffold (QueueSubscribe + Durable +
  ManualAck + AckWait + MaxAckPending(1)) — PR-3 mirrors this — and `AIActionExecutor`, an
  in-process durable JetStream consumer on `velion.application.conversation.ai_action.reviewed`.
- On `decision=approved` + `kind=ticket.classification`: load the action (`GetAIAction`), find the
  suggested ticket, **claim** it atomically (`MarkAIActionExecuted`: `status approved→executed`,
  the idempotency gate), promote suggested→open + apply routing via `Service.UpdateTicket`, emit
  `ai_action.executed`. Rolls the claim back on a promote failure so JetStream can redeliver.
- Repo additions: `GetAIAction`, `MarkAIActionExecuted`, `UnmarkAIActionExecuted` (org-scoped).
- Loop reachability fixes in `AiActionReviewPanel.tsx` discovered during live verification: the
  panel filtered `status:'suggested'` but the producer writes the pending status as
  `suggest_ticket`, and `summarize()` read `proposed_ticket` but the payload uses
  `suggested_fields` — both fixed so pending actions and their details actually render.
- B3 copy flipped **"Decision recorded" → "Applied — the suggested ticket was promoted and routed"**
  for approve (reject stays "Decision recorded — suggestion dismissed"); the false "not
  auto-executed yet" note removed. Flipped **only after** live verification.

Plane rule honored: executor lives in conversation-core (Application Plane), never Model Plane.

DoD evidence:
- `go build ./... && go vet ./... && go test -race ./...` → green (`internal/consumers` +
  `internal/conversation` ok; 0 fail/panic). Tests cover: promote+route on approve, idempotent
  duplicate delivery (sequential + 16-goroutine concurrent → exactly one promote + one emit),
  rejected/wrong-kind/foreign-org no-op, transient-error retry, promote-failure rollback, malformed-ack.
- **Live smoke through the running stack** (cc-go rebuilt + restarted; executor logged
  "subscribed to velion.application.conversation.ai_action.reviewed"): ingest → classify
  (suggested ticket) → approve → ticket promoted **suggested→open** + `category=billing` routing
  applied → ai_action **executed**; re-approve left it **open** (no double-apply). RESULT: PASS.

No-new-fakeness proof: "Applied" copy ships only because a real approve→promote round-trip was
verified end-to-end against running NATS+Postgres+cc-go. The review panel now renders real pending
actions (not an empty list) and their real suggested fields.

---

## PR-2 — E5 audit tool-NAME fix (+ findings on per-connection & admin erasure)
**Status:** E5 backend tool-name fix complete; per-connection UI + admin erasure scoped below.

### Done — E5 backend: audit `details.tool` is the real tool NAME (was the call-id)
Root cause: execution-core `tool_step_id` suffixes the step with the provider **call id** when present
(`agent.rs:385`), and session-core derived `details.tool` from that suffix — so the GDPR `tool_action`
audit row recorded an opaque call id, not the tool name.

Fix (no proto churn — `mp-contracts/src/gen` is stale, so reuse the existing execution-core→session-core
metadata side channel, the `CompleteStepRequest` output prefix):
- execution-core `record_tool_step`: extend the prefix `[data_category=… zdr=…]` → adds `tool=<name>`
  (it already has `tool_name`). Tool identifiers have no spaces, so the space-delimited prefix stays parseable.
- session-core `audit_publisher`: `ToolActionDetail` gains `tool: Option<String>`; `parse_tool_action_detail`
  reads `tool=`; new `resolve_tool_name(detail, step_id)` prefers the prefix name and falls back to the
  step_id-derived id only for pre-E5 steps. `grpc.rs` caller uses `resolve_tool_name`.
- Evidence: `cargo check -p execution-core` → ok; `cargo test -p session-core audit_publisher` → 6 passed / 0 failed;
  tests assert the prefix `tool=` parse + the prefer-prefix-over-step_id resolution + pre-E5 fallback.
  (Full live verification — rebuild execution-core+session-core, drive an agentic tool run, inspect the
  audit row — is the recommended next confirmation; the unit tests pin the parse/resolve against the exact
  prefix execution-core now writes.)

### Finding — per-connection "Used by AI?" is NOT honestly buildable from the current toolset
The builtin agent tools (knowledge_search, company_lookup→Brreg, web_search, …) are not bound to per-org
**connections**, so a per-connection Yes/No column would fabricate attribution (violating the absolute rule).
The honest, real signal is **tool-level + data-category** attribution — which `audit-client.ts` already
aggregates and which Phase-1 E1 deliberately scoped to. The tool-NAME fix makes that view truthful; the
per-connection column stays honestly "Attribution unavailable" until connection-bound tools exist. **Not
fabricating it.** (Path to real per-connection: connection-bound MCP tools + a `source`/connection field on
the audit detail — deferred, not faked.)

### Done — admin Control-Plane erasure: backend confirmed + disclosure pinned
On inspection the **admin erasure backend already ships** (Phase-1 D, `user-core/internal/http/gdpr_handlers.go`):
`hardEraseUser`/`anonymizeUser`/`dsarExport` are gated by `resolveErasureActor` = **admin OR self**, so an
admin can erase another user; hard-erase requires `confirm:true`; the `ErasureAvailable()` 503 gate fires
when AUTH_DATABASE_URL is unset; every op emits a CP audit event + (on erase) the `velion.gdpr.erasure.requested`
cross-plane fan-out. Added this increment:
- **DSAR disclosure pinned verbatim**: extracted the Art. 15 CP-only scope notice to
  `users.DSARControlPlaneDisclosure` (used by `BuildDSARExport`) + `TestDSARDisclosureVerbatim` pins the exact
  three lines — guards "Control-Plane data only; Model/Data via fan-out", never "erased/exported everywhere".
- **Positive admin-authz test**: `TestHardEraseAdminCanTargetAnotherUser` proves an admin passes the gate to
  erase a *different* user (reaches the 503 capability check, not the 403 a non-admin non-self gets).
- Evidence: `go build ./...` ok; `go test ./internal/users ./internal/http` → ok (incl. both new tests).

### Remaining (PR-2) — admin erasure PRODUCT surface (scoped, security-sensitive)
What is NOT yet built is the admin-erases-another-user **product path** through the gateway + SPA. The gateway
`privacy.rs` is deliberately **self-scoped** (no client-supplied id → IDOR-safe). An admin route must:
(1) verify the caller is an **org admin**, (2) **org-scope the target** (confirm the target user is a member of
the admin's org before forwarding — else it reintroduces the cross-tenant IDOR Phase 0 closed),
(3) enforce **step-up re-auth**, (4) forward to user-core `/users/:id/gdpr/erase` with honest CP-only UI copy.
Also reconcile the CP `.env` DB_PASSWORD drift before the erase path runs in a live deploy (never change the
live hex). Deferred deliberately rather than rushed — a cross-org admin route is exactly the surface that must
be org-scoped + negative-tested before shipping.

---

## PR-3 — W3 insight-core Postgres + subscriber + gateway briefs.rs (Preview-gated)
**Status:** durable metric store (foundation) done + verified; subscriber + briefs.rs scoped next.

### Done — insight-core durable metric store (safe, renders nothing)
Phase-1 A shipped insight-core registry-only with an in-memory metric repo (`replicas: 1`). This increment
adds OPTIONAL Postgres persistence (mirrors social-core), gated so Phase-1 behaviour is unchanged until a DB
is provisioned:
- `internal/database/{database.go,migrate.go}` + `migrations/001_insight_core.{up,down}.sql` — embedded
  migration runner + `insight_metric_events` table (id PK, org_id, surface, metric, value, unit, source,
  connector_type, dimensions jsonb, occurred_at) + org/surface/time index.
- `internal/insights/pg_repository.go` — `PGRepository` implements the `Repository` contract:
  `RecordMetricEvent` (idempotent `ON CONFLICT (id) DO NOTHING` — duplicate JetStream delivery never
  double-counts), `ListMetricEvents` (org + `surface = ANY` + window), `ListConnectorSlots` (static registry).
- `config.go` + `main.go`: `DATABASE_URL` empty → in-memory (unchanged Phase-1); set → connect + migrate +
  `PGRepository`. `NATS_URL`/`NATS_TOKEN` added (for the upcoming subscriber). Compose should drop `replicas:1`
  when a DB is wired.
- Evidence: `go build ./...` ok; `go test ./...` green; **live SQL smoke** against `application-postgres`:
  migration applies, inserting the same id twice yields one row (idempotency), the `ANY(text[])` + window
  query round-trips.

### Done — insight-core metric subscriber (the real producer)
- `internal/nats/client.go` + `internal/consumers/{consumer.go,metric_subscriber.go}` — a `DurableConsumer`
  scaffold mirroring PR-1 (insight-core is a separate module, so mirrored not imported) + `MetricSubscriber`
  on `velion.application.>`. Maps conversation-core `LifecycleEvent`s by `type` →
  (surface=inbox, metric): `ai_action.executed`/`reviewed`, `ticket.created`/`suggested`/`resolved`,
  `conversation.created`, `message.received`/`sent`. Unknown types + missing-org are skipped (no fabricated
  metric). Idempotent: the metric id is derived from the source event id (`ins_evt_<id>_<metric>`), so a
  duplicate delivery hits the repo's `ON CONFLICT DO NOTHING`.
- `main.go`: starts the subscriber only when `NATS_URL` is set (no-op otherwise — Phase-1 empty-state preserved).
- Evidence: `go build` + `go vet` + `go test ./internal/consumers ./internal/insights` green — covers
  known-event→metric mapping, stable-id-on-duplicate-delivery, unknown-type skip, missing-org skip, recorder-error retry.

### Remaining (PR-3) — gateway briefs.rs ⛔ BLOCKED on concurrent gateway edits
> Blocker: `apps/Frontend Plane/velionv3/apps/gateway/src/main.rs` (router registration) is uncommitted-dirty
> with the parallel Ownership-phase work. A new gateway domain must register its router in `main.rs`, and a
> clean commit can't stage that without sweeping in the concurrent edits. Resolve by committing/settling the
> gateway edits first; then `briefs.rs` (+ PR-5 `/leads`) can be added cleanly. (Same blocker for any new
> gateway domain.)

- Gateway `briefs.rs` (`authorized_org_id` + `proxy_json`) fan-in over insight rollups + Quarry change +
  model-gateway summary with citations; **refuses-or-labels "Preview" below a real-event threshold** — never
  trends over empty data; empty-org test proves it refuses/labels. (Deferred to keep this increment coherent +
  verified; the foundation above renders nothing, so no fakeness is introduced by landing it first.)
