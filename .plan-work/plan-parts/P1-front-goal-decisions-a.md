# Quarry v2 and in-app browser: implementation plan

| Field | Value |
|---|---|
| Date | 2026-10-14 |
| Status | Draft for approval |
| Input audit | `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md`, sha256 `60d32ec5270718b82a8baf90d2659e1d55dffa07814abc7153bdcd456ed4d49e` (evidence pack: `C:/dev/verevon-audit-2026-10-14/audit-evidence/`; the folder is read-only) |
| Target | Commit `afb83928` (2026-10-10) of the live tree `C:/dev/CoresSystem` plus about 324 uncommitted entries (197 modified, 122 untracked, 5 deleted) |
| Branch note | The planning worktree (branch `claude/gracious-gagarin-087139`, HEAD `828d9d55`) is 61 commits behind main. All code was therefore read from the live tree `C:/dev/CoresSystem`, never from the worktree. Files that exist only as uncommitted work are tagged `[U]` (untracked); files committed at `afb83928` but modified in the live tree are tagged `[M]`; committed and unchanged is `[C]`; a file this plan would create is `[NEW]`. |
| Scope of this document | Priority items and waves W1 and W2 in file-level detail; waves W3 to W5 as epics. Nothing is built by the planning pipeline. |
| Path legend (this part) | `Q` = `apps/Ingestion Plane/Quarry-v2`; `QE` = `Q/crates/quarry-edge/src`; `QR` = `Q/crates/quarry-runtime/src`; `QB` = `Q/crates/quarry-browser/src`; `MP` = `apps/Model Plane`; `MPR` = `MP/rust/services`; `FE` = `apps/Frontend Plane/verevonv3`; `CP` = `apps/Control Plane`. |
| Slice ids | `WS<n>.<letter>`, one workstream per design (WS1 grants, WS2 run control, WS3 driver, WS4 ZDR and tenant boundary, WS5 scrape contract, WS6 budgets, WS7 web policy, WS8 shared-browser backend, WS9 shared-browser frontend, WS10 run trace, WS11 planner, WS12 redaction, WS13 scrape actions and capture, WS14 crawl scope and robots, WS15 job lifecycle and schedules, WS16 extraction, WS17 profiles). `WS3.D` is design D3's slice S3. The slice catalogue gives the letter-to-design mapping. |

## Files that exist only as uncommitted work and what depends on them

About 324 entries of the live tree are uncommitted, and nearly every design rebases onto them. Until the owner commits or branches them as a baseline (decision X-3, section 2.2), gates that pass on one machine may fail in CI and every lane that appends to a shared file conflicts. The table lists the notable files the plan depends on; the full list is `live-uncommitted.tsv` in the planning workspace.

| Path (tag) | What it is | Depended on by |
|---|---|---|
| `QE/main.rs`, `state.rs`, `config.rs`, `routes.rs`, `lib.rs`, `test_support.rs`, `Q/crates/quarry-edge/tests/support/mod.rs` [M] | Edge boot gates, `AppState`, config, router, crawl handoff | WS1.A (boot gate `main.rs` 183-201), WS2.B to WS2.D, WS3.F (`/ready`), WS4.D, WS4.E, WS5.D, WS6.A (`CrawlRequest`, `crawl_handoff`), WS7.C, WS10.A. Almost every W1 and W2 Quarry slice edits one of them. |
| `QE/search_routes.rs`, `extract_routes.rs`, `answer_routes.rs`, `QR/serp.rs`, `smart_router.rs`, `hybrid.rs`, `rerank.rs`, `fusion.rs`, `vector_index.rs` [M]; `QE/model_plane_gate.rs`, `Q/crates/quarry-edge/tests/rerank_behavior.rs`, `QR/data_plane_http.rs`, `Q/crates/quarry-runtime/tests/dp_rerank_contract.rs`, `dp_retrieve_contract.rs`, `Q/crates/quarry-runtime/tests/fixtures/dp-contracts/` [U] | The owner's web-rerank and Model Plane gate work | WS7.E (edits `search_routes.rs` last, after the rerank diff is committed), WS4.D (`serp.rs` log leak), WS16 (extract lane); `rerank_behavior.rs` is a regression guard (section 1.4). |
| `Q/docs/openapi.yaml` [M] (uncommitted hunks at 238-250 and 4012-4150) | Public contract | WS2.A, WS5.H, WS6.A, WS13.I and others; edits are additive only; stage hunks by hand. |
| `scripts/tests/zdr-contract-sync.test.sh`, `scripts/tests/zdr-contract-schema.mjs` [M] | ZDR and contract sync gate (uncommitted +95-line diff) | WS1, WS4, WS7, WS10, WS12, WS16 append rows only (R-14). |
| `Q/crates/quarry-runtime/contracts/cross-plane-token-requests.json` (+ `.sha256`), `QR/token_request_contract.rs`, `CP/auth-core/src/auth/fixtures/cross-plane-token-requests.json` (+ `.sha256`) [M] | Cross-plane token request manifest; `model-invoke` stays `granted:false` | WS1.A (row `browser-grant-validate`), WS16.C (`model-extract`, `model-extract-inference`); one Control Plane registry PR (R-13). |
| `MPR/model-gateway/tests/fixtures/auth-core/quarry-token-claims.golden.json` and `quarry-token-claims.golden.sha256` [U] | Third copy of the claims golden; the other two are `CP/auth-core/src/auth/fixtures/` [C] and `Q/crates/quarry-edge/tests/fixtures/auth-core/` [C] | WS7.I (claim minting); the three copies change together (X-3 item 5). |
| `MPR/model-gateway/src/browser_run.rs`, `http_routes.rs`, `grpc.rs`, `tool_loop.rs`, `quarry.rs`, `state.rs`, `zdr_precheck.rs`, `auth.rs` [M]; `MPR/execution-core/src/runtime_loop/agent.rs` [M]; `MPR/inference-core/src/auth.rs` [M] | Model Plane run, tool and auth paths with in-flight diffs | WS1.B (grant derive in `browser_run.rs`), WS6.E, WS8.G, WS11.B to WS11.F, WS12.E, WS16.B, WS16.I; one editor for the execution-core browser files (R-27). |
| `MPR/model-gateway/src/zdr_features.rs`, `zdr_posture.rs`, `MP/rust/crates/mp-contracts/src/zdr_refusal.rs`, `retention.rs`, `MP/api/contracts/zdr-refusals.json` (+ `.sha256`), `FE/src/shared/api/zdr-refusals.ts` [U] | The Model Plane ZDR refusal catalogue (Z-series initiative) | WS4.I (decision X-16), WS17.B (`browser_profile` key), WS16 refusals. |
| `MPR/session-core/migrations/0044_run_retention_receipt.sql`, `0045_zdr_waived_runs_org_index.sql`, `MPR/session-core/migrations/revert/`, `MPR/session-core/src/retention_receipt.rs` [U] | Session-core retention receipt | WS10.F (migration `0046_run_trace_events`), WS11.H (`0047_run_external_content`); R-4. |
| `FE/src/features/chat/components/ChatPage.tsx`, `use-chat-controller.ts`, `FE/src/features/dashboard/home/KnowledgeComposer.tsx`, `browser-omnibox.ts`, `FE/src/shared/api/chat-client.ts`, `FE/src/shared/i18n/errors.ts`, `FE/src/styles/global.css` [M]; `FE/src/features/chat/components/ChatUtilityPage.tsx` [U] | Chat controller, composer and omnibox | WS9.A, WS9.G, WS9.H (dock and omnibox hunks), WS3.J (blocked-state strings); sibling (b) edits the controller. |
| `Q/docs/RUNBOOKS.md`, `SELF_HOST.md`, `CROSS_PLANE_AUTH.md`, `Features.md` [M] | Docs with stale claims | WS1.A (`BROWSER_BROKER_GRPC` rename), WS3, and the stale-doc register rule (OQ-15). |
| `Q/scripts/tests/docs-contract.test.mjs`, `.github/workflows/quarry-docs-contract.yml` [U]; `.github/workflows/model-plane-rust-ci.yml`, `.githooks/pre-commit`, `.gitleaks.toml` [M] | Gate files CI and the sync script need | WS5.H (route-coverage CI builds on `docs-contract`); every PR. `.gitleaks.toml` must keep the singular `[allowlist]` table (CI pins gitleaks 8.24.3). |
| `apps/Ingestion Plane/docker-compose.yml` [M] | Ingestion compose | 18 design hunks (WS3.A sandbox, WS1.A broker network, WS6.A crawl env and others). |

## Plan at a glance

1. **Goal.** Make Quarry v2 a Firecrawl-plus-Apify-class acquisition platform and make Verevon's in-app browser one governed browser that users and the Model share. The Model proposes, Quarry executes or rejects; Firecrawl is a reference, never a runtime dependency; ZDR propagates through every content-persisting boundary.
2. **Workstreams.** 17 (WS1 to WS17), 162 slices in 15 lanes. Most missing capability is built-but-unwired code, so the plan wires before it builds.
3. **W1, unblock and make safe.** WS1 to WS5 plus the WS6.A crawl-bound hotfix: 125.75 engineer-days (113.75 core, 12 gated), 5.6 to 7.3 weeks at 6.5 engineers.
4. **W2, table-stakes parity.** WS6 to WS17 plus WS3.K (actionability): 510 engineer-days (447 core, 63 gated), 14 to 19 weeks at 12 engineers; beyond about 14 engineers the critical path binds.
5. **W3 to W5 as epics.** 21 W3 items (20 in epics E3.1 to E3.9; F-t2-046 is carried by WS6), 16 W4 items (E4.1 to E4.7), 12 W5 items (E5.1 to E5.3; F-t2-077 parked). None is estimated in the totals.
6. **Critical path.** The shared browser, 50 engineer-days: WS1.A, WS1.B, WS8.F, WS8.H, WS8.I, WS8.J (13 to 18 weeks risk-adjusted).
7. **First three PRs** (file-disjoint, merge in any order): WS4.A gateway result cache org scope and ZDR bypass; WS6.A crawl bounds; WS2.A deny raw page script (`evaluate`) on every agent entry point.
8. **Decisions that block.** X-1 (register `quarry-edge` as a read-only `browser-broker` principal) blocks the merge of WS1.A and every production browser path; ADR-3 (shared human-plus-agent sessions) gates WS8.C, WS8.F, WS8.G and the whole critical path from W2; ADR-1 (policy of record) gates the org-layer slices of WS7. Alongside them sits one process action: commit or branch the uncommitted baseline (X-3).
9. **W1 exit gate.** WS1.A to WS1.C with `BROWSER_GRANT_ENFORCEMENT=strict` on the production-overlay stack, WS2.A to WS2.E, WS3.A to WS3.F, WS3.H, WS3.J, WS4.A to WS4.G with the cache purge recorded, WS5.A, WS5.B, WS5.D to WS5.H, all deployed on rebuilt local images.
10. **Rollout.** Every behaviour change ships behind a default-off or observe-mode flag. Production flags flip only after an L2 run (rebuilt images, running stack, verified end-to-end flow) is recorded in the PR; a deployment claim counts on no weaker evidence.
11. **Out of scope.** Channel Plane, Firecrawl as a dependency, stealth and anti-bot bypass, MCP exposure, per-session VMs, Space members driving another member's browser, unattended browser runs, credential vaulting in pages.
12. **Totals.** W1 plus W2 is roughly 5 to 7 months end to end: 162 to 221 risk-adjusted engineer-weeks (W1 29 to 38, W2 133 to 184).

## Contents

- 1. Goal, success criteria and scope: 1.1 Goal; 1.2 Measurable success criteria; 1.3 Scope boundaries; 1.4 What must not regress
- 2. Decisions first: 2.1 Decision summary table; 2.2 Decision records; 2.3 Design reconciliations applied; 2.4 What can proceed without any decision
<!-- CONTENTS-TAIL: the assembler appends entries for sections 3 onward -->

## 1. Goal, success criteria and scope

### 1.1 Goal

Verevon's web layer has two halves that must become one product. The first is an acquisition API (scrape, crawl, batch, search, monitors, schedules) at the level of Firecrawl and Apify. The second is a remote, leased Chromium that users and the Model share, at the level of the Codex in-app browser and Aside. This plan turns the audit's 77 recommendations (24 Priority, 35 Recommended, 18 Nice-to-have; none already done) into slices that can be built, reviewed and deployed.

Three constraints bind every slice.

- **The Model proposes; Quarry executes or rejects.** Quarry validates a grant before every action, takes one governed action per `/step`, and is the final enforcer of takeover, raw-script denial, budgets and policy. Nothing on the Model Plane side may widen what Quarry allows.
- **Firecrawl is not a runtime dependency.** It is a reference for parity only. No Firecrawl SDK, API or fire-engine call is added anywhere, and parity docs are corrected to what the code does.
- **ZDR propagates.** Zero data retention reaches every boundary that can persist content: gateway caches, logs, spans, receipts, artifacts, checkpoints, schedules, profiles and AI formats. A refusal that exists today is never loosened. The detail is decision ADR-4 and workstream WS4.

Plane ownership is unchanged (`apps/master-ownership-matrix.md` [C]). Control Plane owns identity, billing, entitlements and the principal registry; Model Plane owns the BrowserBroker, grants, runs and cost; Ingestion Plane (Quarry v2) owns acquisition and browser execution and persists knowledge only through Data Plane contracts; Frontend Plane owns the SPA and the same-origin gateway; Application Plane owns projections and notifications. No database is crossed between planes, and Space access may only narrow an owner-plane decision.

The audit's ten headline findings map to the workstreams as follows: no production browser path and no grants (WS1), cross-tenant gateway search cache and ZDR gaps (WS4), takeover and raw script ungated inside Quarry (WS2), budgets not enforced and unbounded crawl (WS6, WS14), crawl without scope or robots (WS14), thin scrape contract and empty ZDR scrape (WS5), half-built shared browser (WS8, WS9), secrets unmasked (WS12, WS17), one Chromium relaunching on viewport change (WS3). The remaining workstreams (WS7, WS10, WS11, WS13, WS15, WS16) deliver the parity features on top.

### 1.2 Measurable success criteria

Twelve criteria define done for W1 and W2. Evidence types: **CI** hermetic test in CI; **L2** live production-overlay stack run on rebuilt images, recorded in the PR; **LOG** log or artifact scan with a canary; **PROBE** scripted adversarial request.

| # | Criterion (pass condition) | Measured by | Workstreams |
|---|---|---|---|
| SC1 | **Prod-mode browser path works.** With `APP_ENV=production` quarry-edge, a signed-in user's UI session and an AI run both start, act and close with zero 401/403 and zero grant-less fallbacks; a revoked or expired grant is 403 `SECURITY_BLOCKED`, not a 5xx; gateway `BROWSER_GRANT_ENFORCEMENT=strict` is the production default; no service principal holds `browser:write`. | L2 (Playwright API legs A to E plus log-scan wrapper); CI L1 contract test | WS1 |
| SC2 | **Raw script and takeover enforced inside Quarry.** `AgentAction::Evaluate` on `/step` is 403 `raw_script_denied` for every principal; an agent step during `HumanTakeover` is 409 `USER_TOOK_OVER` and the Model Plane run pauses rather than fails; a user token carrying `initiated_by:"agent"` is blocked the same way. | CI `admission_order_is_pinned`, contracts golden; L2 live takeover | WS2, WS11 (mapping), WS13 |
| SC3 | **No leaked or runaway runs.** An abandoned run frees its Chromium context within its class idle TTL (agent 300 s, interactive 900 s) and keeps a resumable checkpoint; the (cap+1)th concurrent run of an org is 429 `RATE_LIMITED` with `Retry-After` and `details.reason`; `close_run` releases the session even with a viewer attached; a run stuck behind a viewer still hits the 4 h backstop. | CI sweeper and registry tests; L2 RSS at 8, 16 and 32 contexts | WS2, WS6 |
| SC4 | **Driver stability.** A viewport mismatch between two sessions does not kill the other's pages (relaunch counter stays 0); a hung CDP call returns typed `TIMEOUT` with `details.effect` within its deadline and never wedges the slot; the production overlay launches Chromium with the sandbox on; a challenge page is a block outcome with at most one recovery and no stealth code remains. | CI SlotBook and deadline tests; L2 two-session viewport probe, sandbox launch | WS3 |
| SC5 | **ZDR and tenant boundary.** Org B querying an org A cached search gets no hit; ZDR search is neither read nor written from the gateway cache; a ZDR scrape and ZDR agent step write zero log lines containing URL, title, host or query; the published ZDR matrix is generated from route attributes and every pinned gap row has a behavioural test; a ZDR scrape returns inline markdown and writes zero artifacts. | LOG canary scan across edge, gateway and orchestrator; CI `cache_admission_only`, `log_field_discipline`, `zdr_behavior`, `zdr_claim_floor`; PROBE cross-org | WS4, WS5 |
| SC6 | **One strict scrape contract.** `/v1/scrape` returns the requested formats inline; PDF and DOCX URLs return extracted text; image and archive URLs are 415 `UNSUPPORTED_MEDIA`; unknown keys warn, then (after the strict date) 400 with `error.details.fields[]`; `GET /v1/scrape/{id}` is org-bound; a JSON Schema golden pins the contract. | CI schema golden, golden fixtures; L2 PDF and DOCX fixtures through the stack | WS5, WS13 |
| SC7 | **Budgets and capacity stop work.** A budget-exhausted agent run stops with 402 `BUDGET_EXHAUSTED`, partial results kept and the run resumable; a failed action still counts against attempts; `max_pages: 0` is refused and every crawl has a finite default page cap; manual (Interactive) sessions are never capped by agent budgets; every run response shows `constraints` and `clamped[]`; usage events exist for browser seconds, crawl pages and extraction. | CI; L2 budget exhaustion on the stack; billing metric codes confirmed | WS6, WS14, WS11 (token cap) |
| SC8 | **Crawl is scoped, polite and policy-bounded.** A crawl with an external link stays in scope by default; a robots-denied URL is not fetched and appears in the job's `denials` with a remedy; `Crawl-delay` feeds the scheduler; org policy can only tighten (a `RequestAsk` that widens is unrepresentable; the Model cannot set `ignore_robots_txt`); an override use writes an audit event before the job is acknowledged. | CI policy-meet property tests, `RobotsGate`; L2 fixture site with robots and an external link | WS14, WS7 |
| SC9 | **One shared browser.** A user browses past one host and past 120 s on a `human_browse` session; an AI run attaches to the user's visible tab, acts only in its pinned tab under the attach allow-list, and the user can Take over (agent gets `USER_TOOK_OVER`), Give back, Reveal and Stop; a Space member cannot attach to or view another user's session; the dock shows chat beside the page. | L2 attach flow and invariants I1 to I8; CI route-table scope test; Playwright (dock) | WS8, WS9, WS1, WS2 |
| SC10 | **Secrets never leave capture.** A seeded password and a seeded cookie value appear in no `/v1/*` response, receipt, event, log line, planner prompt or screenshot (masked rectangles); profiles are encrypted at rest with the DEK wrapped under an org KEK; a profile owned by user A is 404 to user B; service principals get 403 on `/v1/profiles*`; `GET /v1/profiles/:id` returns structure with `value: null`. | LOG and PROBE canary; CI `secret-shapes.v1.json` golden (both copies) and rule version; L2 | WS12, WS17 |
| SC11 | **Jobs have a lifecycle and schedules execute.** `GET /v1/jobs/{id}`, cancel, pause, resume, errors and a live tail work for scrape, crawl and batch with typed `timed_out`; a created schedule fires exactly once per window under Temporal and a skipped fire records a typed reason; a cross-tenant schedule id is 404; a ZDR org cannot create a schedule or monitor (after enforcement); the SPA hides what the backend cannot honour. | L2 Temporal schedule fire on the stack; CI; PROBE cross-tenant | WS15, WS14, WS4 |
| SC12 | **One canonical run trace and governed extraction.** A browser run's trace (receipts, egress decisions, artifacts, control changes) is readable from `GET /api/v1/runs/{id}/trace` as `model_plane.v1.Event`s ordered by `producer_seq`, with a request id from SPA to Quarry and none for ZDR runs; `formats:[{type:"json",schema}]` returns schema-valid, verbatim-verified JSON through the governed model lane, with a typed refusal under ZDR and `BUDGET_EXHAUSTED` when over budget. | L2 trace round trip and extraction on the stack; CI trace allowlist golden | WS10, WS16, WS11 |

Global gate for every criterion: the regression guards (section 1.4 and section 6) stay green, the docs named in the slice are corrected, and the stale statement is registered in `apps/STALE_DOC_DELETION_REGISTER.md` [C] (OQ-15).

### 1.3 Scope boundaries

**In scope.**

| Wave | Items | Treatment |
|---|---|---|
| W1 | 10: F-t2-001 to 007, 009, 026, 027 (plus F-t2-013 phase 1, takeover enforced inside Quarry) | File-level slices, WS1 to WS5, with WS6.A pulled forward as a hotfix |
| W2 | 18: F-t2-008, 010 to 023, 025, 029, 045 | File-level slices, WS6 to WS17, with WS3.K timed here |
| W3 | 21: F-t2-024, 028, 030 to 042, 044, 046 to 050 (046 is carried by WS6) | Epics E3.1 to E3.9, outlined |
| W4 | 16: F-t2-043, 051 to 065 | Epics E4.1 to E4.7, outlined |
| W5 | 12: F-t2-066 to 077 (077 parked) | Epics E5.1 to E5.3, outlined |

29 items have file-level design coverage (W1, W2 and F-t2-046); 48 are epics.

**Explicitly out of this plan.**

- Channel Plane (docs-only; nothing is built against it).
- Firecrawl as a runtime dependency (reference only).
- The user's own Chrome through an extension (F-t2-077), parked by owner ruling 2026-09-28.
- Stealth, fingerprint rotation and anti-bot bypass. WS3 deletes `QB/stealth.rs` [C] (no caller outside the file) and narrows rotation; no new evasion, CAPTCHA solving or consent auto-accept (F-t2-066 stays in W5 behind the anti-bot ADR).
- An MCP server for Quarry (F-t2-064) beyond the OpenAPI-from-types slice (WS5.H), until ADR-7.
- Per-session or per-tenant VMs and containers (F-t2-060), remote browser providers (F-t2-059) and egress tiers (F-t2-058), until ADR-10 and a reviewed relaxation of the loopback-only egress proxy type. Browserbase, Browserless and Kernel run no scrape actions or captures (501).
- Space members viewing or driving another member's session, shared run ACLs and Space-scoped grants (a new design; F-t2-072).
- Durable crawl, batch, schedule or monitor under ZDR (refusals kept, never loosened) and any hash-only cache.
- Password references and a credential vault in pages (F-t2-024 E3 and E5) until ADR-2; the `fill_otp` approval card waits for WS1 and F-t2-013.
- Unattended agent browser runs and routines (F-t2-061): draft-only through W2.
- Model-originated scrape action lists (opening `scrape:interact` to any principal) until ADR-6.
- Code-mode browser control (F-t2-063) and cached extraction profiles (F-t2-065).
- The fleet console build: it is hidden (WS2.E), not built.
- Agent PC (sandbox) metering and egress: sibling session (a).
- Document parsing depth, OCR routing and XLSX, PPTX, ODF and EPUB (F-t2-037), and fail-closed PII redaction (F-t2-050): the typed 415 stands until the model-gateway grant exists.

**Deferred inside waves W1 and W2** (designed or flagged, not scheduled): WS3.L (rich input), WS5.I (ScrapeOptions in crawl and batch), WS8.K (acting-user claim, optional), WS9.K and WS9.L (attach UI, agent-flagged tabs), WS10 live fan-in (S6c) and tag 24, WS15.K (job results, needs a source decision), design-level WS6 L1 to L7 (about 26 days), WS12.H (legacy-row scrub, product decision), WS16.J (AI formats in crawl and batch), WS17.J (single-writer profile lease). Phase-limited items: F-t2-013 phases 2 and 3 (WS8), F-t2-004 phase 2 (WS10), F-t2-045 skip-only, F-t2-027 live part (deletion first, ADR-5 flip later), F-t2-003 phase 2 (enforce rows after ADR-4). About 22.5 days of design-level work (WS3.L, WS5.I, WS9.K, WS9.L, WS16.J) are in no total.

**Adjacent sessions (flagged, not planned here).** Sibling (a), Verevon Space: agent PC and browser inside Spaces, ZDR-safe telemetry and projections. Sibling (b), Model Plane plus chat and agents: BrowserBroker grants, the agent browser path, delegated bearer forwarding, approvals, `browser.*` schema, the execution-core planner, `browser_run`, cost-core. The cross-session interface table names who owns which code; this plan owns the grant, attach and cap contracts.

### 1.4 What must not regress

The audit's strengths are protected by named tests. Test names tagged [design] are introduced by the slices; others exist today. The full guard list with owning workstreams stays in section 6.

| Strength | Pinned by (must stay green) |
|---|---|
| Search: multi-provider router, hybrid rerank, ZDR gating, org-keyed `SearchCache` | `Q/crates/quarry-edge/tests/cache_policy.rs` [C], `rerank_behavior.rs` [U], `zdr_behavior.rs` [C]; `cache_admission_only.rs` [design]; Quarry `SearchCache` key keeps the org segment (fix only the gateway cache) |
| SSRF and egress containment | `Q/crates/quarry-runtime/tests/browser_egress_boundary.rs` [C], `Q/crates/quarry-browser/tests/ssrf.rs` [C], `agent_navigation_blocks_private_targets_at_edge_boundary` in `QE/agent_routes.rs` [C]; private, loopback, link-local and metadata targets stay blocked under open web |
| Governed execution: one action per `/step`, grant validated before every action, broker domains replace the client list | `admission_order_is_pinned` [design]; L1 contract tests "single minting path" and "attach delegate may not widen domains" |
| One-shot sensitive approvals | `budget_refusal_does_not_consume_single_use_approval` [design]; no code path creates a reusable `sensitive_approval` |
| Risk gates in code | `browser_risk.rs` and `provenance.rs` tests; risk gates stay in code; all three `browser.*` outputs classify as external content; `raw_script_denied` |
| Evidence: immutable receipts, checkpoints, cursor timeline, `from_seq` replay | Migration tests for runtime `0010` to `0014` on a populated table; `legacy_checkpoint_without_budget_fields_deserialises` [design]; receipts masked on read only; `from_seq` replay unchanged |
| ZDR strictness | `Q/crates/quarry-edge/tests/zdr_claim_floor.rs` [M], `zdr_behavior.rs` [C]; `zdr-feature-matrix.json` golden [design] with a never-loosen rule; `log_field_discipline.rs` [design]; sync-script rows append only |
| Observation and locators | Existing chromiumoxide locator tests (live tests return early until WS3.B wires `CHROMIUMOXIDE_TEST`); `TargetRepairRequired` stays 409 |
| Politeness and durable orchestration | Scheduler and queue tests; Crawl-delay feeds `HostScheduler::set_min_interval`; no second queue store; `GetVersion` ids unique |
| Webhook delivery and the error envelope | `TestErrorCodeStatusMirror` [design]; `Q/crates/quarry-core/tests/contracts.rs` [C] updated per new code; dispatcher untouched |
| Production defaults: refuse ephemeral stores and the default control key, Chromium pinned, HMAC internal calls | Boot-gate tests; production without a browser grant validator refuses to start (`QE/main.rs` [M] 197-201) |
| Integration plane: AES-GCM vault, short-lived token broker, raw proxy 410 | integration-corev2 vault and `legacy_routes` 410 tests; no Quarry read of user secrets |
| MCP client hardening | MCP client tests; any future MCP front reuses the exact allowlist |
| Browser profiles with governance | `Q/crates/quarry-edge/tests/profile_roundtrip.rs` [C]; `profile_guard` table tests [design]; a seeded cookie never appears in a response or log; run-start ZDR refusal in `agent_routes.rs` unchanged |
| Bulk plus interactive stay separate | Crawl and batch unreachable to service principals; scrape `actions` never route through `/v1/agent/*` |
| Takeover authority | Gateway 409 check in `FE/apps/gateway/src/domains/browser.rs` [C] stays until WS2.C is live with `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=on`; `agent_step_during_human_takeover_returns_user_took_over` [design] |
