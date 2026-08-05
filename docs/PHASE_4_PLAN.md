# Verevon — Phase 4 "Data-Driven UI" Execution Plan

> **Status:** Approved 2026-06-21 (recon-grounded + hardened by a 6-persona triage council). Source roadmap: `Verevon-ai-first.md`. Phase 4 = a **user-mandated headline (data-driven UI)** + the bounded completions deferred from Phase 3. Gating verified green on-branch (Ownership PR-1..5 `596edfa4`, schema `b856b09d`, retrieval/documents wiring `0e0f748e`, W2 change-producer `916f3cc4`/`938c560d`/`71684b45`).
>
> ## Headline
> **Make every remaining Verevon v3 surface render REAL backend data or an honest empty/Preview/Blueprint label — starting with the acute `WorkspaceSettingsPage` fabricated-security de-fake — on a shared `MeasurementState` read substrate**, plus the bounded completions (EU embedding residency, the HITL act-leg, the insights-overview proxy) that the headline makes coherent.
>
> ## THE MULTI-XL INVARIANT (the discipline that keeps this shippable)
> The backlog carries **five XL items** — and **ZERO of them build in Phase 4**: `A5-real-agent-config-wiring`, `B /embed public chatbot`, `C-D3c model-plane autonomous producer`, `F2 leads roller PII`, `G Tailwind→semantic-CSS rewrite`. Each is recorded as a **Phase 5** entry below, not silently dropped. **No code for any of the five may START until the Phase-4 headline (A) + F1 are live-verified.** If A slips, F1 is cut — never the headline.
>
> ## THE ABSOLUTE HONESTY RULE
> Real data, or an honest empty/Preview/Blueprint label via the shared `MeasurementState` pattern (loading/empty/preview/error/live — where **"live" never attaches to an unproduced value**, per the gold `insights-workspace.ts`). No false-security and no false-privacy claim ships without its backing gate in the same release. Every new gateway route is IDOR-clean (org+user from session, never client-supplied).

Paths: gateway = `apps/Frontend Plane/verevonv3/apps/gateway`; SPA = `apps/Frontend Plane/verevonv3/src` (quote the space).

---

## Scope

**IN (Phase 4):**
- **A1** `WorkspaceSettingsPage` de-fake — DO FIRST, release blocker.
- **A8** strengthen the no-fabricated-state lint (ship before the de-fake sweep).
- **A9** thin shared `read-data` substrate (extract `MeasurementState`/`ResourceResult`/`withResourceTimeout` from `insights-workspace.ts` — a contract, **not** a framework).
- **A4/A6/A7** cheap de-fake sweep (ChatbotStudio analytics, studio templates, dashboard cards) + strip the gateway `studio.rs` seeded `Ava Berg`/Unsplash demo blocks.
- **A5** agents-config — **honest LABEL only** ("Blueprint / not yet configured for this org"); the real per-org store is Phase 5.
- **D** insight non-OAuth legs: deploy insight-core durable (DATABASE_URL+NATS_URL) + brief citation enrichment + a new IDOR-clean `/api/v1/insights/overview` proxy so the SPA renders real metric values.
- **F1** EU embedding residency (scoped L) — proto region/zdr field + EU Azure embedding deployment + deny-by-default enforcement gate.
- **C** HITL act-leg — D-2 outbound-send executor (draft.reply via integration-corev2, idempotent) + narrow D-3 (proposal channel + generic `CreateAIAction`).

**DEFERRED to Phase 5:** A2 chat-history durability · A3 studio durable store · A5 real agent-config store+CRUD (co-design with B's identical need — build ONE store) · **B /embed** (own security sprint) · C-D3c autonomous producer · D GA4/GSC actual fetch (external-OAuth-blocked) · D Quarry-change producer (pull in only if cheap) · **E Quarry at-scale** (recon-corrected: `PostgresRequestQueue` is already built; only real gap = CrawlJobWF frontier checkpoint; no v3 consumer → defer fully) · **F2 leads roller PII** (LIA-first, consent/retention substrate, default-OFF, Ownership-paired) · **G** all Tailwind/styling debt + file splits.

---

## PR sequence

### `PR-0` — Gate confirmation + multi-XL ledger
- Verify the gating commits landed on the working branch; write the **five-XL ledger** + the "zero XLs build in Phase 4" invariant + the named forbidden-starts guard (B, C-D3c, F2, G-Tier1) into this doc.
- **DoD:** gating confirmed green; ledger + invariant + guard recorded.

### `PR-1` — Lint hardening + shared read-data substrate *(lands BEFORE any de-fake)* — deps: PR-0
- **A9:** extract `insights-workspace.ts` state machine into `shared/read-data` (`MeasurementState`, `ResourceResult<T>`, `withResourceTimeout`) with state-transition unit tests; `insights-workspace` re-exports (no behavior change).
- **A8:** extend eslint beyond `no-restricted-imports` to flag **inline literal arrays fed to `<For>`/`.map` in render scope** AND **boolean literals bound to security/compliance controls**; add a fixture proving it fires on a reintroduced A1-style array.
- **DoD:** `shared/read-data` exports the typed substrate with tests; lint fails CI on the fixture (and would have caught the pre-existing A1/A5/A6 violations); `pnpm verify` green.

### `PR-2` — WorkspaceSettingsPage de-fake *(DO FIRST, release blocker)* — deps: PR-1
The acute breach: `securityToggles` all `enabled:true` under a "Security policy" header + `'aquatiq.no Verified'` / `'Connected apps 2/4'` / `'Last sync 8 minutes ago'` / webhook `'200 OK 8 min ago'` static arrays — a **fabricated security/integration posture** (GDPR/SOC2 misrepresentation, worse than a missing feature).
- Replace `securityToggles` with real org-security reads or a disabled/unknown state — **never render a security control as enabled from a literal**; replace the status cards + `webhookRows` with live reads or honest empty state (via the A9 substrate). The honest pattern (`TrustCenterSection.tsx` `createResource`) is local + copyable.
- **DoD:** zero concrete-false values survive (grep-clean + a **negative render test** that fails if any known false string renders without a backing resource); verified live against a real org with **MFA NOT enabled** (shows disabled, not "Required").

### `PR-3` — Cheap de-fake sweep + agents honest label + studio seed strip — deps: PR-2
- **A4** ChatbotStudio analytics → real aggregates or "no measurement yet" (zero hardcoded `value:'0'`); **A6** studio templates honestly framed as starter templates; **A7** dashboard Weather/Traffic/News wired or relabeled (nothing implies a non-existent live feed).
- **A5** relabel `AgentsPage` + the 861-line `verevon-agent-blueprints` as **"Blueprint / not yet configured for this org"** (no Active/Private badge). Strip the gateway `studio.rs` seeded `Ava Berg`/Unsplash demo blocks → honest empty canvas.
- **DoD:** no implied-live values on any swept surface; seeded studio blocks gone; agents surface passes the 4-path honesty test; `cargo test` (studio.rs) + FE `pnpm verify` green. *(Add no new Tailwind when editing; any new empty-state component uses semantic CSS.)*

### `PR-4` — Insight non-OAuth legs + `/overview` proxy — deps: PR-3
- Add `DATABASE_URL`+`NATS_URL` to insight-core compose (dedicated schema on application-postgres; NATS at inter-plane-bus) — **reconcile the real Application-Plane postgres password** to avoid the known DB-auth-drift boot failure.
- Add a gateway **`/api/v1/insights/overview` proxy** mirroring `insights.rs /connectors` (server-set org scope, never client-supplied).
- SPA insights surface renders real metric values or honest `not_connected`; **GA4/GSC show "connect to enable", never placeholder numbers**; brief citation enrichment present.
- **DoD:** insight-core boots durable (no DB-auth failure); `/overview` proxy live + IDOR-clean; SPA renders real-or-honest-empty; passes the strengthened lint.

### `PR-5` — EU embedding residency *(scoped L; parallelizable)* — deps: PR-1
- 1-hour spike to locate the exact inference-core enforcement point (mirror `speech.rs` region gate) and confirm `CreateEmbeddingRequest` shape (recon: it has **no** region/zdr field today — only `InferRequest` carries zdr).
- Add `region`/`zdr` to `CreateEmbeddingRequest` + regen Go/Rust/Python; provision an **EU Azure embedding deployment** (same model, **no reindex**); add a **deny-by-default enforcement gate** (reject non-EU egress unless an explicit allow-flag) + startup **fail-loud if endpoint non-EU**; add the region-deny test.
- **DoD:** EU deployment provisioned; proto regenerated; non-EU path rejected (negative test); startup fail-loud; `clippy -D warnings` clean; **no UI in-region claim ships unless the gate is proven in the same release.**

### `PR-6` — HITL act-leg *(most net-new backend, LAST among IN)* — deps: PR-4
- Add a `draft.reply`/outbound-send kind to the existing idempotent `ai_action_executor.go` via integration-corev2; **reuse the atomic approved→executed guard so a redelivered approve does NOT double-send** (dedup on `approval_id`).
- Add the `verevon.model.action.proposed` NATS subject + a generic cc-go `CreateAIAction` path + consumer so a human/hook can queue a `draft.reply` end-to-end.
- Emit a terminal `ai_action.send_failed` event (no silent retry-forever); audit every send with `approval_id`; **UI never claims "sent" before adapter confirmation**.
- **DoD:** propose→approve→act flows end-to-end; redelivery does NOT double-send (dedup verified); send-failure surfaces honestly; new routes IDOR-clean; `go test` + `cargo test` green.

### Sequencing
PR-0 → **PR-1 (lint + substrate first — guardrail + leverage)** → **PR-2 (do-first de-fake)** → PR-3 sweep → PR-4 insight → PR-5 EU residency (parallel, disjoint code) → **PR-6 act-leg last**. **HARD GUARD:** no code for B, C-D3c, F2, or G-Tier1 may start until A + F1 are live-verified; if A slips, cut F1, not the headline.

---

## Risk register
- **Fabricated SECURITY posture (WorkspaceSettingsPage)** → PR-2 is the do-first release blocker; negative render test + live walk against an MFA-off org.
- **Public `/embed` attack surface** (gateway's first unauthenticated route class, over a model-gateway that accepts client-supplied tools with no server-side allowlist) → fully OUT of Phase 4; DoD asserts no public route / no `chatbot_agent` store / no embed.js appears.
- **Irreversible customer-send** → reuse the atomic approved→executed guard with dedup on `approval_id`; no-double-send is a release gate.
- **Leads roller PII lawful basis** → F2 hard-gated to Phase 5: written LIA first, then consent/retention store, field minimization (no fødselsnummer), GDPR fan-out, default-OFF, Ownership pairing.
- **Over-scoping (5 XLs)** → explicit ledger + "zero XLs build" invariant + named forbidden-starts that can't begin until A+F1 are live.
- **Synthetic-on-empty regression** → mandate the shared `MeasurementState` substrate; "live" never attaches to an unproduced value; strip the studio seed.

## Tooling & skills kit for the executor
- **Search:** codegraph + context-mode (re-confirm with `rg`). **Docs:** context7/`ctx7` for SolidJS, axum, sqlx, tonic/gRPC, NATS, Azure OpenAI (EU regions).
- **SolidJS (A1/A3-sweep/A4/A5/A6/A7 + the A9 substrate + D SPA):** `solidjs-vite-typescript`, `solid-*`, `vite` (NO Tailwind — semantic CSS in `global.css`; `<For>/<Show>`; `props.x`); agents `typescript-reviewer`, `build-error-resolver`.
- **Rust (F1 inference-core gate + proto, gateway `/overview` proxy, studio.rs seed strip):** `rust-patterns`, `rust-testing`; agents `rust-reviewer`, `rust-build-resolver`.
- **Go (insight-core deploy, cc-go act-leg, integration-corev2 send):** `golang-pro`, `golang-patterns`, `golang-testing`, `go-concurrency-patterns`; agents `go-reviewer`, `go-build-resolver`.
- **DB/migrations (insight-core schema, the embedding proto):** `database-migrations`, `postgres-patterns`; agent `database-reviewer`.
- **Security (PR-2 fabricated-security, PR-5 EU enforcement, PR-6 irreversible send, IDOR-clean routes):** `security-review`; agent `security-reviewer` (MUST review PR-2, PR-5, PR-6).
- **Method:** `tdd`/`tdd-workflow` (RED-first for the negative-render, lint-fixture, region-deny, no-double-send tests), `code-review`, `verification-loop`; agents `code-reviewer`, `tdd-guide`, `e2e-runner`. **Browser smoke:** playwright/chrome-devtools (walk each de-faked surface live). **PRs:** github MCP/`gh`.

## Global Definition of Done
Every remaining v3 surface renders real data or an honest empty/Preview/Blueprint label (verified by a live walk, not code-reading); `WorkspaceSettingsPage` has zero concrete fabricated values (negative test + live MFA-off walk); `shared/read-data` exists with tests and ≥1 consumer; the strengthened lint fails CI on a reintroduced fabricated array; agents surface labeled "Blueprint", studio seed stripped, cheap surfaces real-or-honest; insight-core durable + `/overview` proxy live + IDOR-clean (GA4/GSC honest "not connected"); EU embedding residency gate proven (non-EU rejected, fail-loud) with no UI claim ahead of the gate; HITL act-leg propose→approve→send works with no double-send; the **multi-XL invariant held** (no B/C-D3c/F2/G-Tier1/A5-real code started); all gates green (FE `pnpm verify`, `go test`, `cargo test` + `clippy -D warnings`).
