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

### Done — gateway briefs.rs (Preview-gated)
(Unblocked: the concurrent gateway `main.rs`/`middleware.rs` edits were committed to main as a WIP snapshot
`8c2940fe`, so the router registration could be added cleanly.)
- `domains/briefs.rs` — `GET /api/v1/briefs`, org resolved via `authorized_org_id` (never a client header),
  fans in insight-core's `/insights/overview` via `proxy_json`. **Preview gate**: sums the real per-surface
  `total_events`; `>= BRIEF_MIN_EVENTS (5)` → `state:"live"`, else `state:"preview"` + a disclosure. Surfaces /
  scorecards pass through verbatim — **never a fabricated trend**; empty-org → preview over an empty overview.
- Registered in `domains/mod.rs` + `main.rs` router.
- Evidence: gateway `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` + `cargo test briefs`
  all green — 3 tests pin the gate: below-threshold→preview+disclosure, at-threshold→live (no disclosure),
  empty-overview→preview & never fabricates (surfaces/scorecards empty).
- Scope note: the Quarry-change + model-gateway-summary (citations) legs are additive enrichment on the same
  envelope (follow-up); the Preview gate already protects the honesty invariant. A thin SPA brief surface
  consuming `/api/v1/briefs` is the remaining UI piece.

---

## PR-6 — W6 design-partner slice + mandatory de-fakes (the JOIN, last)
**Status:** mandatory honesty de-fakes done; the full vertical slice + i18n centralization scoped.

### Done — mandatory de-fakes (honesty-critical, unblocked, shipped first)
- **US-region fabrication removed**: `AccountSettingsPage.tsx` dropped the synthetic
  `{ value: 'US', label: 'US region default' }` timezone option (it conflated residency with a timezone and
  `'US'` is not a valid IANA zone). Real IANA zones (Europe/Oslo, UTC, America/New_York, Europe/London) remain.
- **Grep guard added**: `velionv3-ci.yml` fabrication-guard now fails on any `region default` string in the SPA.
  Verified: passes on the cleaned tree; **red on a deliberate re-introduction**.
- **No orphaned nav**: confirmed the SPA nav (`app/shell/navigation.ts`, `features/core/lib/sidebar-navigation.ts`)
  does not point at the unbuilt engines (leads/briefs/monitoring) — nothing to remove.
- Evidence: `pnpm verify` green (156 tests, build ok); guard red-on-violation verified.

### Remaining (PR-6) — the vertical-slice JOIN + i18n
- The thinnest coherent slice scoped to what shipped (Brreg resolve → AI proposes → human approves →
  executes via W4 → audited in-region via E5, + brief from W3) run end-to-end for one real Norwegian org —
  depends on PR-5 (leads) + the live stack; the "join" is finalized last by design.
- Centralize Norwegian i18n — a cross-cutting consistency refactor (not a fakeness fix); scoped to avoid a
  rushed sweep.

---

## PR-5 — W1 Brreg lead-builder (metered, company-data-only) — built last
**Status:** filtered Brreg client (the novel core) done + verified; the rest of leads-core scoped.

### Done — NEW filtered Brreg client (the "real net-new work")
NEW Application-Plane module `apps/Application Plane/leads-core` (separate Go module, social-core template) with
`internal/brreg/client.go` — a filtered Enhetsregisteret client distinct from org-core's navn-only size-20 client
(which is left untouched):
- Filters: `naeringskode`, `kommunenummer`, `organisasjonsform`, `fra/tilAntallAnsatte`,
  `fra/tilRegistreringsdatoEnhetsregisteret`, page+size.
- **Real API contract verified against the live Brreg docs** (and it corrects the plan): `/enheter` uses
  **page+size with a hard 10 000 window** (`(page+1)*size > 10000` → HTTP 400) — there is **no `searchAfter`**
  for this endpoint. Guarded with a typed `ErrDeepPagingLimit`.
- **1–4 employee band**: Enhetsregisteret v2 rejects `fra/tilAntallAnsatte` values 1–4 (HTTP 400; `antallAnsatte`
  is null for 0–4 employees). Guarded client-side with a typed `ErrEmployeeBandUnsupported` so the caller never
  hits an opaque 400.
- **Company data ONLY**: calls only `/enheter`, never `/roller`. The `Company` record carries no person/role/
  contact/birth-number field.
- Evidence: `go build` + `go vet` + `go test` green — 6 tests: facet query building, empty-facet omission,
  **1–4 band → typed error**, **10k cap → typed error**, fixture parse maps company fields only, and TWO no-PII
  assertions (a serialization-leak test feeding a fixture with `roller`/`fodselsnummer`/`epostadresse` proves
  none leak into the serialized `Company`, plus a structural field-name test).

### Remaining (PR-5, scoped) — the rest of leads-core (substantial)
leads-core service scaffold (Postgres + migration + repo + handlers + internal-key, social-core template) for
saved lead lists; gateway `/leads` domain (`authorized_org_id` + `proxy_json`); v3 `features/leads`
(search → save named list → table → CSV export); metering via a billing-core entitlement; per-export audit
event. The filtered client above is the load-bearing novel piece; the rest is service plumbing on proven templates.

---

## PR-5 — COMPLETE + live-verified (W1 end to end)
The leads-core service was built, deployed, and live-verified against the REAL Brreg API:
- **Real Brreg search** (leads-core container): `naeringskode=10.20` → 594 companies (1814SALMON AS, …);
  `kommunenummer=4601` → 57,720 (Bergen). HTTP 200, real Norwegian company data.
- **Full real-data flow**: search → save named list ("Norske lakseprodusenter", 3 real companies) → CSV export
  → header is the 12-column company-only schema, contains the real company, **0 PII tokens**.
- **1–4 employee band**: leads-core → 422 (typed `invalid_filter`); confirmed the live Brreg API itself
  returns HTTP 400 for `fraAntallAnsatte=2` (the client guards it before calling).
- **Cross-org isolation**: a foreign org GETting another org's list → 404.
- **Metered**: gateway `/leads/export.csv` gated on the billing-core `leads` entitlement (entitlement_allowed
  unit test). **Per-export audit**: leads-core logged "per-export audit enabled" (NATS connected); emits
  `velion.audit.v1.application.lead_export` (count metadata only).
- Deployed: `docker compose up -d --build leads-core` → healthy; migrations ran; serving :3164.
- Environment note: the only thing NOT exercised from inside the container in the first pass was a
  network-egress edge — re-tested directly, the container DOES reach data.brreg.no (real results above).

## PR-6 — de-fakes + i18n + join
- **US-region fabrication removed + grep-guarded** (test-enforced, red-on-violation) — done.
- **No orphaned nav** — confirmed; the new "Leads" nav item points at a BUILT engine.
- **Norwegian i18n centralized** — `src/shared/i18n/no.ts` is the single source; the leads feature is wired
  through it (Norwegian copy). Convention established; full app-wide migration is incremental follow-up.
- **Vertical slice (the JOIN)** — all components are built + verified: Brreg resolve (W1, live), AI proposes →
  human approves → executes (W4, live-verified), audited in-region (E5, tool-name fix), brief (W3, Preview-gated).
  Gateway domains (`leads.rs`, `briefs.rs`) are registered + clippy/test-green; the gateway was rebuilt so they
  are served. The full design-partner run for one real org through the gateway needs a real session + the
  `leads` billing entitlement configured — the cores + gateway domains are verified; that final operational
  run is the remaining step.

## PR-4 — W2 recurring monitoring — CORRECTLY DEFERRED
The day-15 go/no-go is **2026-07-05** (not yet reached as of 2026-06-20). Per the plan, W2 proceeds only if
Phase-1 Track C is merged-and-green at that gate; otherwise it defers wholesale to Phase 3. Building it now
would violate the conditional gate, so it is intentionally NOT built (never half-shipped).

Pre-flight signal (read-only, recorded for the 2026-07-05 decision): quarry-edge `POST /v1/change/check`
returns **401 (mounted, auth-gated) — NOT 501**, so Track C's change endpoint appears deployed (the plan's
"`/v1/change` returns 501 → cannot build" pre-flight is NOT triggered). The remaining gate is purely the dated
go/no-go + the formal Track-C-merged-and-green review (C1 edge `--features postgres-queue` + C2 monitoring.rs +
C3 SPA tab). If the team chooses to override the date gate given this positive signal, that is a deliberate
scope decision — the plan as written defers to 2026-07-05.

### W2 recon (date gate overridden by request 2026-06-20) — root cause + remaining build
Pinned the decode mismatch:
- `store.Schedule` (quarry-control `internal/store/store.go:122`) = `{ID, Cron, TargetKind(scrape|crawl|batch),
  TargetRef, Enabled, CreatedAt}` — **no `org_id`, no `Workflow`/`Args`**.
- `schedules.ScheduleSpec` (quarry-orchestrator `internal/schedules/schedules.go:34`) = `{ID, Name, Cron,
  Workflow, Args, Paused}`.
- The orchestrator decodes Control-Plane Schedules into ScheduleSpec, but the JSON keys don't line up
  (`target_kind`/`target_ref` vs `workflow`/`args`), so `Workflow=""` → the Temporal reconciler creates a
  schedule with no workflow. AND **`ChangeMonitorWF` does not exist** (only 3 workflows are registered in
  `cmd/orchestrator/main.go`). The change STORE (`quarry-runtime/postgres_baseline_store.rs`:
  save_baseline/compare_snapshot/create_diff_record) and the on-demand `/v1/change/check` DO exist (Phase-1 C).

Remaining W2 build (the recurring layer on top of Phase-1 C's on-demand check), spanning 3 services:
1. quarry-control: add `org_id` to `store.Schedule` + a `TargetKind→(Workflow,Args)` mapping so a
   `change`-kind schedule serializes to `Workflow="ChangeMonitorWF", Args=[org_id, url]`.
2. quarry-orchestrator: create **ChangeMonitorWF** + an activity that runs the change-check for a source on
   the preset, register it in `cmd/orchestrator/main.go`, and make the reconciler map the decoded spec; confirm
   orphan-reaping is org-scoped (two coexisting orgs test).
3. `quarry_sources` durable CRUD (org_id) replacing the `/v1/sources` stub.
4. change → run-event-log → existing `fanoutWebhooks` + one in-product notification; gateway monitoring read
   surface + minimal v3 tab (fixed hourly/daily/weekly presets only).

**Status: NOT built.** Per the plan's absolute rule ("never half-ship W2 — fully persists+alerts with cross-org
isolation, OR cleanly deferred"), this multi-service Temporal engine + its live DoD (a real source on a daily
preset persists a baseline + writes a diff on change + fires webhook + notification, cross-org isolated) require
a focused build + a live Temporal/Quarry-stack verification — not safely completable as a no-half-ship unit in
this session's tail. The recon above is the precise build plan for that focused session.

## No-new-fakeness audit (final)
Every rendered value traces to real per-org data today or is explicitly labelled, and several fabrications were
*removed* or *refused* rather than added: W4 "Applied" only after a live approve→execute round-trip; W3 briefs
Preview-gated (never trends over empty); E5 audit shows the real tool name (call-id fixed); per-connection
"Used by AI?" NOT fabricated (builtin tools aren't connection-bound); leads are company-data-only (no PII) with
the real Brreg page+size/10k + 1–4-band contract (the plan's "searchAfter" corrected to the real API); the
US-region residency fabrication removed + guarded.

---

## PR-4 / W2 — recurring change-monitoring (BUILT this session; live-verify in progress)

W2 was executed under the Ultracode blueprint (`tasks/w80y4vlr0.output`) — 8 of 9 steps built, all unit-green
across four services. The honest MVP loop is end-to-end in code:

`schedule (org_id + created_by + preset)` → orchestrator reconciler maps it to **ChangeMonitorWF** → workflow
mints a per-fire run_id from its Temporal execution → **CheckChange** activity (runtime `/v1/internal/run_page`
for the fresh fingerprint → edge `/v1/internal/change/record` for compare→save_baseline→[on change]diff) →
emits **change_detected** → quarry-control fans out the **webhook** (existing) **+ one in-product notification**
to the schedule's creator via notification-core.

### What shipped
- **quarry-control (Go)**: `store.Schedule.OrgID`+`CreatedBy`+`Preset`; `Validate` maps the change_monitor
  preset (hourly=`0 * * * *`/daily=`0 9 * * *`/weekly=`0 9 * * 1`) to a literal 5-field cron (cron.Validate
  rejects `@daily` aliases) and requires org_id; migrations `008_schedules_org_id.sql`+`009_schedules_created_by.sql`;
  pg Create/Get/List carry org_id+created_by; NEW `schedule_wire.go` projects each schedule to the
  orchestrator contract `{workflow,args,paused}` — **change_monitor only** maps a workflow (`ChangeMonitorWF`
  + `[{org_id,url,created_by}]`); scrape/crawl/batch emit empty workflow so the reconciler skips them (no
  fake "scheduled scrape"); `MountSchedules` rewritten to wire-shaped list/get/create; NEW `internal/notify`
  HTTP sink + `notifyOnChange` (best-effort, idempotent on event_id, recipient = `payload.created_by`).
- **quarry-orchestrator (Go)**: `ScheduleSpec` decode now binds (control emits workflow/args) — the empty-Workflow
  root cause is fixed; defensive guard skips empty-workflow specs (reaper stays whole-set-safe, org_id rides in
  Args only — the latent multi-tenant orphan-reap bug is documented & deliberately left untouched); NEW
  `ChangeMonitorWF` + `CheckChange` activity (+ `EdgeBaseURL`/`EdgeAuthToken` config, defaulting EdgeBaseURL to
  RUNTIME_BASE_URL since the edge serves both); registered as a 4th workflow.
- **quarry-edge (Rust)**: NEW `POST /v1/internal/change/record` OUTSIDE `require_auth` (shared-token auth, org_id
  from the trusted body — the orchestrator calls for many tenants; org was verified at create time) running the
  full compare→save_baseline→(Changed)put-diff-artifact+create_diff_record sequence in-process; `create_schedule`
  detects a change_monitor by `preset`, stamps org_id+created_by from the **verified JWT** (never the client body),
  and forwards a `store.Schedule`-shaped body to control. Reused the existing `PostgresBaselineStore` (no
  re-implementation). Compiles with + without `postgres-queue`; clippy-clean on changed files.
- Incidental: fixed a pre-existing newer-clippy lint in `quarry-core/pagination.rs` (derivable_impls) that blocked
  the workspace `-D warnings` gate.

### Gates (unit)
- quarry-control: `go build/vet/test ./...` green (store/resources/cron incl. new wire + preset + org_id tests).
- quarry-orchestrator: `go build/vet`, `go test ./internal/workflows ./internal/schedules` green incl. 4 new
  ChangeMonitorWF tests (changed/unchanged/scheduled-run-mints-run_id/activity-failure); `-race` green.
- quarry-edge: `cargo check` (default + `--features postgres-queue`) green; 7 change_routes + 4 schedule_routes
  unit tests pass; no clippy warnings in changed files.

### Deliberately deferred (no silent cut)
- **Step 7 (quarry_sources durable CRUD)**: blueprint's "only time-boxable deferral" — a source row is optional
  scaffolding; the schedule's `target_ref=url` already drives monitoring. The DoD does not require it.
- **Gateway monitor-CREATE + SPA form**: the gateway already surfaces change history (`monitoring.rs` →
  `/api/v1/monitoring/{check,latest,history}`); the recurring-monitor *create* path (preset/target_ref) is the
  one product seam still to add. The W2 DoD is the backend engine, verified directly.

### Live DoD verification — see below (rebuild of quarry-{edge,control,orchestrator} + Temporal fire).

### LIVE DoD VERIFICATION — ✅ COMPLETE (rebuilt quarry-{edge,control,orchestrator}, real Quarry+Temporal stack)
Stack: ingestion-plane compose (quarry-edge :8082 built `--features grpc,http3,postgres-queue` + dedicated
`quarry_edge` Postgres; quarry-control :8081 → `quarry_v2` Postgres + NOTIFICATION_CORE_URL=notification-core:3140
over inter-plane-bus; quarry-orchestrator → Temporal; notification-core :3140). Migrations 008/009 auto-applied
on control boot (schedules now carry org_id + created_by).

- **Persistence (direct `/v1/internal/change/record`, deterministic fast-fire)**: org_a fp v1 → `{status:new, baseline_id:bln_…}`;
  same fp → `{status:unchanged}` (no new baseline); fp v2 → `{status:changed, prev_fingerprint:v1, baseline_id, diff_id}`.
  DB: org_a = 2 baselines + 1 diff; v2 baseline chains to v1 (prev_baseline_id set).
- **Cross-org isolation (data)**: org_b posting the SAME url returned `status:new` (its `load_latest` saw none of
  org_a's baselines) → DB org_a=2/org_b=1 baselines, org_a=1/org_b=0 diffs.
- **Schedule → Temporal (the "daily preset")**: POST change_monitor (preset=daily) → control persists row +
  list/get emit `workflow=ChangeMonitorWF`, `args=[{org_id,url,created_by}]`, cron `0 9 * * *`, paused=false. The
  orchestrator reconciler (≤30s) materialized the Temporal schedule `{"Workflow":"ChangeMonitorWF"}`, NextRunTime
  = next 09:00 UTC.
- **Workflow → full chain**: `temporal workflow start --type ChangeMonitorWF` (org_placeholder, example.com,
  created_by=user_w2_verify) against a pre-seeded bogus baseline → COMPLETED, run_started→change_detected→run_completed
  in the event log; CheckChange fetched example.com (real blake3 fp) and persisted baseline+diff via the edge.
- **Webhook leg**: a webhook subscribed to change_detected got a pending WebhookDelivery whose payload contains
  `change_detected` (fanoutWebhooks fired on the workflow's event).
- **ONE in-product notification**: notification-core `/notifications` (x-user-id=user_w2_verify) returned EXACTLY
  one entry — event_type=change_detected, channel=in_app, delivery_status=delivered, payload carrying the real
  fingerprint + diff_id + url. Recipient = the schedule's creator (created_by threaded edge JWT → schedule →
  workflow args → event payload → notifyOnChange).
- **Cross-org reconcile safety**: adding org_b's schedule did NOT reap org_a's (both Temporal schedules coexisted)
  — the global-desiredSet reaper stayed whole-set-safe.
- **Lifecycle**: DELETE /v1/schedules/{id} → 204 → control rows = 0 → reconciler reaped both Temporal schedules.

**Two bugs caught BY live verification (not unit tests — both are Postgres/runtime-only paths):**
1. `schedulesStore.List` SELECT/Scan column mismatch — the `created_by` column was added to the Scan (8 targets)
   but the List SELECT string (single-line) wasn't, so List Scanned 8 against 7 columns → silent error → empty
   list → the reconciler saw no schedules. Unit tests use the in-memory store and couldn't surface it. Fixed.
2. `MountSchedules` rewrite dropped the `DELETE /v1/schedules/{id}` route the old generic mount provided (edge +
   gateway both forward it) → schedules couldn't be deleted/reaped. Restored `deleteScheduleHandler`.

Both fixes re-verified live (list returns the schedule; DELETE → 204 → reaped). Test data cleaned up.

### Gateway BFF surface (product reachability) — built + static-green
Added the recurring-monitor CREATE/LIST/DELETE seam the monitoring domain's own doc flagged as "Phase 2 (C-FULL)":
- `velionv3/apps/gateway/src/domains/monitoring.rs`: `POST /api/v1/monitoring/schedules` (preset+SSRF-guarded url →
  edge `/v1/schedules`; edge stamps org_id+created_by from the session token), `GET /api/v1/monitoring/schedules`
  (lists change_monitor schedules only), `DELETE /api/v1/monitoring/schedules/:id`. Mirrors the proven
  check/latest/history proxy pattern (quarry audience token, never a client org header). `cargo check` +
  `cargo clippy -D warnings` green.
- The change-history READ surface (`/api/v1/monitoring/{check,latest,history}`) was already wired pre-W2.
- Remaining (documented, not a silent gap): the SPA monitoring-tab CREATE **form** + a live gateway-session
  run through `/api/v1/monitoring/schedules`. The backend engine + edge create path are live-verified directly;
  the gateway routes are thin proxies over that verified path.
