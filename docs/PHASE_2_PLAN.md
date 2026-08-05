# Verevon — Phase 2 "Complete the Honest Loop + Monetize the Wedge" Execution Plan

> **Status:** Approved 2026-06-19 (recon-grounded incl. live Brreg API probes + hardened by a 6-persona honesty-first council). **Gated on Phase 1 being merged** (Phase 0/IDOR is ALREADY merged at HEAD `ff2a3f59`). Source: `Verevon-ai-first.md`, `docs/PHASE_0_PLAN.md`, `docs/PHASE_1_PLAN.md`.
>
> ## Headline spine
> Phase 2 **completes the honest agent-runner loop** — monitor → brief → approve → **ACT** → audit becomes real (approve stops being a no-op; every tool action is in-region auditable) — and rides a **thin, metered Brreg lead-builder** as the monetization companion. It is **NOT** a net-new lead-builder phase. Take **one** net-new engine (W1), not two.
>
> ## THE ABSOLUTE RULE — no new fakeness
> Every rendered number/label/status traces to real per-org data *today*, or carries an explicit **Preview / disclosure** label. No live-but-empty brief, no "Applied" copy before a working executor, no over-claimed DSAR/erasure, no fabricated region, no Brreg PII.
>
> ## Hard invariants (every PR)
> - **Re-baseline against HEAD `ff2a3f59`** (NOT the stale `c1b06ecd` "pre-Phase-0" ancestor). Phase 0/IDOR is merged with a green cross-tenant regression test.
> - Every new gateway domain (leads, briefs, monitoring) resolves org via `upstream::authorized_org_id` + `require_session` + `proxy_json` — **never a client header** — enforced by a CI lint.
> - Each PR is a focused, revertible commit on its own branch off `main`; never reset/clean the user's tree. Do not commit/push unless asked.
> - **Never change the live DB password hex** (`apps/Control Plane/.env`); reconcile drift before any erasure path runs.
> - Quality gates green per PR: gateway `cargo fmt --check` + `clippy -- -D warnings` + `cargo test`; Go `go test -race ./...`; SPA `pnpm typecheck && pnpm lint && pnpm test && pnpm build`; live smoke through the running gateway.

Paths: gateway = `apps/Frontend Plane/verevonv3/apps/gateway`; SPA = `apps/Frontend Plane/verevonv3/src` (quote the space).

---

## Scope — IN vs DEFERRED

**IN (Phase 2):**
- **W4-MVP** — ticket.classification HITL **executor** in conversation-core-go (THE SPINE KEYSTONE, build first).
- **E5** — light the already-built-and-dark per-connection "Used by AI?" UI (tool_name + source attribution) + admin Control-Plane erasure.
- **W3-thin** — insight-core on Postgres + one JetStream subscriber + gateway `briefs.rs` (Preview-gated). *First cut to Phase 3 if Phase-1 Track A slips.*
- **W1-thin** — NEW Application-Plane `leads-core`: filtered Enhetsregisteret search → save list → CSV, **company data only**, metered. Built **last**.
- **W2-conditional** — recurring monitoring honest MVP. **Gated on a day-15 go/no-go** that Phase-1 Track C is merged-and-green; else drops wholesale to Phase 3.
- **W6-thin** — the thinnest coherent design-partner vertical slice (scoped to what ships) + mandatory de-fakes.

**DEFERRED to Phase 3:**
- Cross-plane DSAR fan-out + erasure-propagation subscribers (event is emit-only; Data Plane has no per-subject ownership — a truthful Art.15 individual export isn't buildable yet).
- W4 `draft.reply` **send** executor (irreversible) + the XL Model→Application agent-proposed-actions producer.
- W1 enrichment waterfall / **roller/contacts (PII)** / agent-driven list-building / bulk-CSV >10k.
- W2 per-org quotas + team aggregates (and W2 wholesale if the day-15 gate fails).
- W3 as a separate `brief-core` (gateway `briefs.rs` is the correct seam).

**Cut order if the window compresses:** W1 first, then W3, then W2 — **NEVER cut W4 or E5** (the cheap trust keystones). W6 scope tracks only what actually ships.

---

## PR sequence

### `PR-0` — Reconcile Phase-1-B, confirm Phase 1 merged, re-baseline — deps: none (prerequisite)
- Land/commit the in-flight uncommitted cc-go Track-B work (~+301 lines across `repository.go/service.go/types.go/handlers.go/server.go/service_test.go`) so it isn't clobbered.
- Confirm Phase 1 Tracks A/B/C/D/E are merged-and-green; re-baseline against `ff2a3f59`.
- Add a **CI lint** that fails any new gateway domain reading org from a client header (no new `org_id_from_headers`; `x-verevon-org-id` stays in `STRIPPED_HEADERS`).
- Record the **day-15 Phase-1-C go/no-go** date for W2.
- **DoD:** dirty cc-go work committed (not lost); Phase 1 green on main; the no-client-header-org CI gate is red on a deliberate violation; W2 go/no-go date on record.

### `PR-1` — W4 ticket.classification executor *(THE SPINE)* — deps: PR-0 (Phase-1 B)
- In-process **durable JetStream consumer inside conversation-core-go** on `verevon.application.conversation.ai_action.reviewed` (filter `kind=ticket.classification` + `decision=approved`) → promote the suggested ticket + apply routing via existing `Service.UpdateTicket`; add `GetAIAction` read; emit `ai_action.executed`.
- **Idempotent** by AIAction id (only promote if not already executed); `-race` test with duplicate delivery.
- Build the reusable Application-Plane durable-consumer scaffold here (model on `notification-core/internal/consumers/`) — **W3 reuses it**.
- Flip B3 SPA copy "Decision recorded" → **"Applied" only after** the consumer is live-verified.
- **Plane rule:** executor lives in conversation-core-go, **never** Model Plane.
- **DoD:** approving a real ticket.classification action promotes + routes the ticket, emits `ai_action.executed` once (no double-apply on redelivery), `go test -race` green, copy reads "Applied".

### `PR-2` — E5 "Used by AI?" live + admin CP erasure — deps: PR-0 (Phase-1 E1); parallel with PR-1
- Add `tool_name` + `source` to `CompleteStep` proto + execution-core + session-core audit publisher; **verify the audit row carries the human-readable tool NAME (fix `details.tool` = call-id) before flipping the dark UI live**; light the SPA placeholder.
- Ship **admin Control-Plane erasure** (org-core `HardEraseUser`/anonymize) with RBAC + typed-confirm + step-up re-auth; **copy scoped to CP account data, states Model/Data purge pending** (no "erased everywhere").
- Keep `BuildDSARExport` CP-only disclosure **verbatim** (pin with a test). Reconcile CP `.env` DB_PASSWORD drift before erasure runs.
- **DoD:** a real tool action renders the tool name + source in the previously-dark UI (traced to a real session-core row); admin CP erasure works with step-up and never claims cross-plane purge; DSAR disclosure test-pinned.

### `PR-3` — W3 insight-core Postgres + subscriber + gateway brief *(Preview-gated)* — deps: PR-0 (Phase-1 A) + PR-1 (consumer scaffold) — *first cut if Phase-1 A slips*
- insight-core Postgres `Repository` (`RecordMetricEvent`/`ListMetricEvents`/`ListConnectorSlots`) + up/down migration mirroring social-core; **drop `replicas:1`**.
- In-process JetStream subscriber on `verevon.application.>` (REUSE the PR-1 scaffold) mapping cc-go inbox/ai-action events to metrics; idempotent.
- Gateway `briefs.rs` assembler (`authorized_org_id` + `proxy_json`) fan-in over insight rollups + Quarry change + model-gateway summary **with citations**; **refuses-or-labels "Preview" below a real-event threshold** — never renders trends over empty data.
- **DoD:** insight-core on Postgres with the subscriber mapping real events; brief assembles only from real events with a visible Preview label below threshold; an empty-org test proves it refuses/labels rather than fabricates.

### `PR-4` — W2 recurring monitoring honest MVP *(CONDITIONAL on the day-15 go/no-go)* — deps: PR-0 + Phase-1 Track C (C1+C2+C3) merged-and-green
- Pre-flight: confirm the edge compose runs `--features postgres-queue` + `QUARRY_EDGE__DATABASE_URL` or `/v1/change` returns 501.
- Add `org_id` + a target→Workflow/Args mapping to the durable `Schedule`; **fix the `Schedule`(store.go:122) vs `ScheduleSpec`(schedules.go:34) decode mismatch** so the reconciler creates a real `ChangeMonitorWF` (not `Workflow=""`); confirm orphan-reaping does **not** cross tenants (test two coexisting orgs).
- `quarry_sources` durable CRUD with org_id (replace the empty-page `/v1/sources` stub).
- `ChangeMonitorWF` + activity calling the **EXISTING** `compare_snapshot` → `save_baseline` (chain `prev_baseline_id`) on New/Changed + `create_diff_record` on Changed, nothing on Unchanged. *(Do NOT re-implement the store — it already exists + is tested.)*
- Fixed **hourly/daily/weekly presets only** (no cron UI); change → run-event-log → existing `fanoutWebhooks` + one in-product notification; gateway monitoring read surface; minimal v3 Monitoring tab.
- **DoD:** a real source on a daily preset persists a baseline + writes a diff on change + fires webhook + notification, with org_id on every schedule and proven cross-org isolation; **OR** W2 is explicitly logged Phase-3-deferred and **nothing half-built ships**.

### `PR-5` — W1 thin Brreg lead-builder *(metered, company-data-only)* — deps: PR-0 (gateway conventions); independent of Phase-1 tracks; built **last**
- NEW Application-Plane `leads-core` (Go, social-core template: Postgres + migration + repo + handlers + internal-key).
- A **NEW filtered Brreg client** (`naeringskode` + `kommunenummer` + `organisasjonsform` + employee-range + reg-date; `searchAfter` cursor for the 10k cap; handle the employee **1–4 → HTTP 400** band) with fixture-based table tests. **Do NOT patch** org-core's navn-only size-20 client.
- Gateway `/leads` domain via `authorized_org_id` + `proxy_json`; v3 `features/leads` slice (search → save named list → table → CSV export); **metered via a billing-core entitlement**; per-export audit event.
- A test asserting **ZERO roller/contact/named-person/birth-number field** is ever persisted or serialized.
- **DoD:** filter Enhetsregisteret by the live facets, save a list, export CSV of company fields only; leads-core is a new metered Application-plane service; the no-PII test passes; CSV org-scoped + audited; the 10k cap + 1–4 band handled.

### `PR-6` — W6 design-partner vertical slice + mandatory de-fakes *(the JOIN, finalized last)* — deps: PR-1 + PR-2 (guaranteed); folds in PR-3/PR-4/PR-5 as they land
- Assemble the thinnest coherent slice scoped to exactly what shipped: Brreg resolve (existing lookup) → AI proposes → human approves → **executes (W4)** → **audited in-region (E5)**, plus brief (W3) / monitor (W2) only if they landed.
- **Mandatory regardless:** remove the **"US region default" fabrication** at `AccountSettingsPage.tsx:413` (a real timezone like `America/New_York` is fine; nothing may be *labeled* a US/non-EU region) — add a grep/test enforcing it; centralize **Norwegian i18n**; no orphaned nav pointing at unbuilt engines.
- Run the slice against **one real Norwegian design-partner org** and their real monitored URLs; execute the pre-demo no-fakeness checklist.
- **DoD:** the slice runs end-to-end for one real Norwegian org through the gateway with **no unlabeled synthetic values**; the US-region fabrication is removed (test-enforced); i18n centralized; no nav points at an unbuilt engine.

---

## Resolved questions
- **Q1 scope / Q2 spine:** complete the loop (W4 + E5 + W3-thin) is the spine; lead-builder is thin/metered/last; one net-new engine, not two.
- **Q3 W4:** ship ticket.classification MVP first; "Decision recorded" → "Applied" only after the consumer is live; defer draft.reply-send + agent producer.
- **Q4 W2:** conditionally in, day-15 go/no-go on Phase-1-C; honest MVP = fixed presets + org_id + existing PostgresBaselineStore + webhook/notification; never half-ship.
- **Q5 W1:** company-data-only MVP; NEW leads-core (not org-core); metered add-on; filtered client is real net-new work.
- **Q6 W5:** E5 + admin CP erasure now; defer cross-plane DSAR + erasure subscribers; keep CP-only disclosure verbatim.
- **Q7 W3:** event-threshold gate + Preview label; assembler in gateway `briefs.rs`, not a new core.
- **Q8 gating:** Phase 0 is merged (`ff2a3f59`); gated on Phase 1; PR-0 reconciles the in-flight cc-go work first; W4+E5 parallel, then W3 + W2-lane, W1 last, W6 the join.

## Risk register
- **Live-but-empty brief (W3)** → hard event-threshold gate + Preview label; empty-org refuse/label test.
- **Approve-theater regression (W4)** → keep "Decision recorded" until the consumer is verified live; idempotent + duplicate-delivery test.
- **Over-claimed DSAR/erasure** → scope erasure copy to CP data (Model/Data pending); keep DSAR disclosure verbatim + test-pinned; no per-subject Data-Plane enumeration.
- **Residency contradiction** → remove the US-region option in PR-6; grep/test no UI labels a non-EU region.
- **Brreg PII** → company data only; no-PII assertion test; roller/contacts to Phase 3 behind a GDPR gate.
- **W2 hidden double-gating** → day-15 go/no-go; if Phase-1-C isn't green, W2 auto-defers wholesale.
- **Multi-tenant scheduler leak (W2)** → thread org_id Schedule→ScheduleSpec.Args→workflow→activity→store; test two orgs coexist through a reconcile + reaping is org-scoped.
- **Stale-baseline sequencing** → PR-0 re-baselines against `ff2a3f59` + commits the dirty cc-go work before any engine branch; do NOT re-implement the W2 store.
- **IDOR regression into new surfaces** → every new domain uses `authorized_org_id`; CI lint fails client-header org reads.

## Tooling & skills kit for the executor
- **Structural search:** `codegraph` MCP (`codegraph_context` → `codegraph_explore`); `context-mode` (`ctx_batch_execute`/`ctx_execute_file`) for big-file scans; re-confirm with `rg` before editing.
- **Library/API docs:** `context7` MCP / `ctx7` CLI for axum, sqlx, gin, NATS/JetStream, SolidJS, Better Auth, Temporal, and the **Brreg Enhetsregisteret API** (filters, `searchAfter`, the 1–4 employee band).
- **Go (PR-1 executor, PR-3 insight-core, PR-5 leads-core, PR-2 erasure):** skills `golang-pro`, `golang-patterns`, `golang-testing`, `go-concurrency-patterns`; agents `go-reviewer`, `go-build-resolver`.
- **Rust (PR-2 proto+session/execution-core, PR-4 quarry workflow, gateway domains):** skills `rust-patterns`, `rust-testing`; agents `rust-reviewer`, `rust-build-resolver`.
- **SolidJS SPA (all v3 surfaces):** skills `solidjs-vite-typescript`, `solid-*`, `vite` (NO Tailwind — semantic CSS in `global.css`; `<For>/<Show>`; `props.x`); agents `typescript-reviewer`, `build-error-resolver`.
- **DB/migrations (PR-3, PR-4, PR-5):** skills `database-migrations`, `postgres-patterns`; agent `database-reviewer`.
- **Security/GDPR (PR-2, PR-5):** skills `security-review`, `better-auth-security-best-practices`; agent `security-reviewer`.
- **Deploy (PR-3, PR-5):** skills `docker-patterns`, `deployment-patterns`.
- **Method:** skills `tdd`/`tdd-workflow` (RED-first for idempotency, no-PII, empty-org, cross-tenant tests), `code-review`, `verification-loop`; agents `code-reviewer`, `tdd-guide`, `e2e-runner`. **Browser smoke:** `playwright`/`chrome-devtools` MCP — verify each surface renders real data or an honest Preview/empty state.
- **PRs:** `github` MCP / `gh` CLI; conventional-commit titles; the per-PR DoD is the test plan.

## Global Definition of Done
Phase 1 merged-and-green + the cc-go Track-B work reconciled (not clobbered); W4 promotes/routes on approve with idempotency; E5 renders real tool name + source in the lit UI + admin CP erasure scoped honestly; W3 (if landed) briefs only from real events with a Preview gate; W1 (if landed) exports company-only CSV with the no-PII test green + metered; W2 either fully persists+alerts with cross-org isolation or is cleanly Phase-3-deferred; W6 runs the slice for one real Norwegian org with no unlabeled synthetic values + the US-region fabrication removed; the **no-new-fakeness audit passes**; every new gateway domain is IDOR-clean (CI-enforced); all quality gates green.
