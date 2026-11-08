# Fixer F1 log: segment 1 (plan lines 1-416, `fix/seg1.md`)

Only `fix/seg1.md` and this log were written. Edits were applied one by one with the Edit tool (the file is LF). Provenance was re-run for every new or corrected path (all as tagged in the segment): `browser_agent.rs`, `quarry_agent.rs`, `llm_planner.rs`, `tool_bridge/mod.rs`, `browser_agent/browser_risk.rs`, `provenance.rs` [C]; `model-gateway/src/browser_run.rs`, `http_routes.rs`, `grpc.rs` [M]; `tools.rs` [C]; `quarrycontracts/envelope.go`, `quarry-core/tests/contracts.rs`, `error.rs`, `fallback_driver.rs`, `driver_plan.rs`, `step_receipts.rs`, `CONTRACTS.md`, `quarry-browser/src/lib.rs`, `stealth.rs`, `quarry-edge/src/cache.rs`, `events.proto`, `docker-compose.production.yml`, `interactive-retention-policy.ts`, `plane-service-principals.json`, `service_auth.go` (org-core and audit-core), `billing-core/internal/internalkey`, `notification-core/internal/http/server.go`, `sandbox-manager/docker-entrypoint.sh`, `pg.go`, `migrations.rs`, `BrowserChrome.tsx` [C]; `docker-compose.yml` [M]; `docker-compose.grant-contract.yml` and `src/features/browser/` MISSING so tagged [NEW]. No proposed correction was missing on disk, so no [UNVERIFIED] was added for a path. Code facts re-read in the live tree before use: `events.proto` 175-181 (types 142 and 143), `stealth` declared at `quarry-browser/src/lib.rs:164`, `grpc.rs` 2698-2707 and `tools.rs` 1182-1190 (typed-error arms, fallback at 2706 and 1189), `browser_agent.rs` 745-752 and 768, `step_receipts.rs` 453-458 and 555-558, `migrations.rs:32`, `pg.go` 162-204, `service_auth.go` credential list, `plane-service-principals.json` rows with `"quarry": "zdr"`.

## Skeptic K1

| id | status | note |
|---|---|---|
| K1-01 | applied | SC3 evidence (RSS runs set `QUARRY_BROWSER_MAX_SESSIONS=48`), X-15 caps row (pair 40 sessions, 32 runs, heavy lane 1; boot check WARN-only until RSS recorded, then fails), R-26 (same pair; the WS3.F design default of 12 is named as the violation). Locations 580, 588, 691, 3427, 3432, 3471 are out of range. |
| K1-02 | applied | SC1 (full compose paths, contract overlay may only set env and volumes, WS1.A0, WS3.A as dependency, `ps` zero `--no-sandbox` and `navigate` 200 recorded), W1 exit gate (at-a-glance item 9), OQ-10 text. 449 and 3624 out of range. |
| K1-03 | applied | SC7 rewritten (`0` or absent takes the finite default 1000, `limits.max_pages.clamped_by:["default"]`, strict 400 only under `QUARRY_EDGE__CRAWL_REQUEST_VALIDATION=strict`, flag added by WS14.G and flipped by WS14.I); X-10 table row and X-10 record aligned (also K3-11 text). The reviewers named WS14.I and WS14.S6 respectively; both are cited (S6 is WS14.G). 993-1002, 3688 out of range. |
| K1-04 | applied | SC10 amended (JSON responses, artifacts and stored screenshots; owner's live frame stream excluded per X-19; Space members receipts only; PROBE that the exclusion is exactly `frames/stream` and `frames/ws`). |
| K1-05 | applied | SC2 split into SC2a (W1, flag `1` named, service-principal refusal by the claims-injecting test) and SC2b (W2, WS11.D). Also corrected the strengths table row (`TAKEOVER_ENFORCEMENT=on` became `=1` plus the WS11.D mapping), because the flag is `0` or `1` everywhere else. 564, 570, 3424, 3478 out of range. |
| K1-06 | applied-modified | The task offered the reviewer's second option unless the plan already moved WS5.C. F2 moved WS5.C from optional to gated and into the W1 exit gate in seg2 (gated 16, optional 6, core 132.25), so I aligned with that: SC6 says `DOC_ROUTING=on` becomes the production default once WS5.C lands, WS5.C is in the gate, 2.4 B row says gated, 2.4 counts say 17 gated and 8 optional after the move. Exit evidence 5 (3482) is in seg7 and must follow. |
| K1-07 | applied | At-a-glance item 7 now "hunk-disjoint"; X-3 "Proceeds without" rewritten (WS5.A, WS5.B and WS3.A removed; WS2.A and WS4.K need one hand-staged hunk each; "stage hunks by hand"); last paragraph of 2.4; the Ingestion compose row in the uncommitted-files table gives the merge order WS3.A, WS5.A, WS1.A0, WS1.A2. The 4.1 one-line table and row 3265-3288 are seg7. |
| K1-08 | applied | Item 7 and the 2.4 closing paragraph: PR 1 and PR 2 share `domains/knowledge/quarry.rs` in non-overlapping regions (merge PR 1 first), PR 2 needs position 0. 7.2 wording itself is seg7. |
| K1-09 | applied (range part) | PR 3 is PRE-2, WS2.A is PR 4 (item 7 and 2.4 closing paragraph); W1 exit gate lists PRE-1 and PRE-2. 7.2 and 3701 are seg7 and must renumber. |
| K1-10 | applied (range part) | Item 6 states WS2.A is not on the critical path. 3694 and 3363 are out of range. |
| K1-11 | applied | WS1.A split into WS1.A1 (broker half, no X-1) and WS1.A2 (registry row, manifest row, Quarry credential path, boot gates, L2 legs A to C; only A2 waits for X-1), matching F2. Updated at-a-glance 8 and 9, the uncommitted-files table (WS1.A2, WS1.A0), 2.1 rows X-1, X-15 and the Blocks wording, the X-1 record Gates and Proceeds, X-2 Proceeds, OQ-10, OQ-15, R-6, R-13 and the 2.4 B row; WS1.A0 and WS1.A1 moved to 2.4 A. Critical path (item 6) uses WS1.A1 (10 d), so it stays 50 d, as F2 states. |
| K1-12 | out-of-range-skipped | The fix is in WS4.H (824). ADR-4 in range already says "profile CRUD (except list and delete)", so it matches the fix. |
| K1-13 | applied | SC5 (PageCache cross-org probe, test `page_cache_is_not_readable_across_orgs`), 1.4 strengths row (PageCache key gains an org segment in WS4.K), W1 exit gate, ADR-4 "Proceeds without", 2.4 A, item 7 (PR 1 with WS4.K). Slice count 162 to 166 (see below). |
| K1-14 | applied | R-11: takeover pause and resume reuse 142 and 143 with a `reason` (`user_takeover`, `approval`, `budget`); only `BrowserAttachSharedJar` takes 160; sibling (b) starts at 161. Verified in `events.proto` 175-181. |
| K1-15 | applied | X-20 record gets the `initiated_by` tighten-only hint, the 403 `scrape_interact_scope_required`, the test `agent_labelled_user_token_cannot_send_actions` and "`QUARRY_EDGE__SCRAPE_ACTIONS=user` is dev-only until that test exists" (matches F4's WS13.F edit). |
| K1-16 | applied-modified | In range the numbers appear only in at-a-glance items 3, 4, 6 and 12 and in 2.4. Used F2's figures: W1 core 132.25 (113.75 + WS1.A 6 + WS1.A0 1.5 + WS2.C 4.5 + WS3.F 4 + WS6.A 2 + WS4.K 0.5), gated 16 (WS5.C moved), so 148.25 d; risk-adjusted 34 to 44 eng-weeks (29.65 x 1.15 and x 1.5); 6.6 to 8.6 weeks at 6.5 engineers (divide by 5.2). The reviewer's "about 140 d" is loose. W2 510.5 (447.5 core) with WS14.S0b; W2 risk-adjusted stays 133 to 184. Totals 167 to 228. Slice table and 4.4 are seg7 and must carry the same arithmetic. |
| K1-17 | applied (range part) | R-13 states why the four credentials are static (each service authenticates callers with a compose-held credential or internal key; org-core, audit-core, billing-core and notification-core files cited), that every credential is read-only and org-bound where the API allows, that `org:retention:read:any` is the one narrow cross-tenant read, and that WS4.H reads posture only through it. X-9 already says the same and was left. WS4.H and WS6.D are seg2 and seg3 (both done by F2 and F3). |
| K1-18 | applied | ADR-1 option C reworded: precedent only for `tenant` retention (`resolveTenantServiceRetentionPosture` throws otherwise); WS7.I adds a second resolver for `zdr` and `persistent` service rows and must not change what those rows mean. Verified in `interactive-retention-policy.ts` and `plane-service-principals.json`. |
| K1-19 | applied | R-1 (sqlx gap-tolerant, cited), plus the same facts added to R-2 (quarry-control applies by name in lexical order, `pg.go` 162-204) and R-3 (broker entrypoint loop re-applies every file each start, so statements must be idempotent). Item (d) of 7.1 is seg7. |
| K1-20 | applied (no occurrences) | No `src/tests`, `src/src` or `src/../tests` path remains in 1-416: lines 134-135 already use `crates/*/tests/`. |
| K1-21 | applied | SC5 and the ADR-4 recommendation: matrix generated from the Rust `ROWS` table, completeness by the CI scan, one canary per route family or `log-unverified`. |
| K1-22 | out-of-range-skipped | 4.1 column name is seg7. |
| K1-23 | applied | R-29: gateway `BROWSER_HUMAN_BROWSE` and `BROWSER_ATTACH` default `0` in every environment until X-13 and ADR-3 are answered. The flag-table row (3451) is seg7. |

## Skeptic K2

| id | status | note |
|---|---|---|
| K2-01 | applied | SC1 and the W1 exit gate name WS1.A0 (new slice, 1.5 d, owned by F2), the production overlay as the base of the L2 stack, and the contract overlay as env and volumes only. OQ-10 says the prod-boot spike is a real fix. The 2.4 A list and the uncommitted-files table carry WS1.A0. |
| K2-02, K2-03, K2-05, K2-06, K2-07, K2-14, K2-15, K2-16 | out-of-range-skipped | Locations 449 and later. |
| K2-04 | out-of-range-skipped | WS1.A, WS6.E, WS7.K, WS8.C are outside; no `buf generate` text in 1-416. |
| K2-08 | applied-modified | R-10 (346) rewritten: Go mirror 12 of 17 codes, the five missing named, `HTTPStatus()` default arm 500, each new code needs a `case`; `grpc.rs` (2698-2707, fallback 2706) and `tools.rs` (1182-1190, fallback 1189) are the fetch, search and extract client only (need `UNSUPPORTED_MEDIA` and a crawl `BUDGET_EXHAUSTED` arm; `tools.rs` lacks `UPSTREAM_BLOCKED`); the agent path is `execution-core/src/quarry_agent.rs`. Did not repeat the reviewer's six-number status list (it covers eight codes); the three new codes keep the statuses already in R-10. |
| K2-09 | applied (range part) | R-10 paths in full, `error_code_short` declared 575 and arm 587, `contracts.rs` 36-60. |
| K2-10 | applied (range part) | The same gap-tolerance facts as K1-19 are in R-1 to R-3; the four [UNVERIFIED] notes named are outside. |
| K2-11 | out-of-range-skipped | WS5. |
| K2-12 | applied (range part) | WS6.A is listed in the W1 exit gate and counted in the W1 total (item 9). The `launch_count()` and WS3 header parts are seg2. |
| K2-13 | out-of-range-skipped | WS2.A, WS7.C, WS7.D. |
| K2-17 | applied-modified | In range only the totals: WS1.A is 16 d (A1 10 + A2 6) with WS1.A0 1.5 d on top, as F2 priced it, not the reviewer's 15 d inside. |

## Skeptic K3

| id | status | note |
|---|---|---|
| K3-01, K3-02, K3-03, K3-04, K3-05, K3-06, K3-08, K3-09, K3-12, K3-14, K3-15, K3-17 to K3-22 | out-of-range-skipped | All locations are 1572 or later, or seg2 to seg6 equivalents done by their fixers. K3-06's slice WS14.S0b exists in seg5 (0.5 d), so the slice count and W2 total in this segment include it. |
| K3-07 | applied | New row R-32 (notification-core migrations: `015_register_quarry_schedule_notifications` WS15.I, `016_register_approval_requested` epic E3.2, `017`+ later); header "31 places" became 32. E3.2's own `015` mention (2951) is seg6. |
| K3-10 | applied | R-31 replaced by the one queue line verbatim ("WS3.C, WS3.D, WS3.F, WS3.K, WS3.G, WS8.E, WS12.B, WS13.C, WS12.F, WS17.E (C1), WS17.G (C2)") and X-3 cites it. Added that WS13.G (tile-and-stitch) is outside the queue and merges before WS12.F (F4 inconsistency 1). |
| K3-11 | applied | X-10 record and table row (see K1-03). |
| K3-13 | applied | ADR-4 table row and record: `AI_FORMATS_ZDR=allow` needs an ADR-4 amendment, stays `refuse` until recorded. WS16.S6 is seg5. |
| K3-16 | applied | R-2: `021_webhook_org_scope` epic E3.5, `022_jobs_idempotency_org` epic E3.7; R-4: session-core `0048` epic E4.6, `0049` epic E4.2 only if a session-core table is chosen, otherwise capability-core `0017_procedures` (`0016` reserved for WS7 outcome B). |

## Path checkers (only C1 has rows in 1-416; C2 to C7 start at 417)

| row | status | note |
|---|---|---|
| C1 358 (R-22 `browser_agent.rs` 742-745) | applied | 749-752 and 768, full path. |
| C1 359 (R-23 `ReceiptBuilder` 453-556) | applied | 455-557, struct at 457. |
| C1 346 (R-10 "six", envelope.go 36-77) | applied | five missing, consts 31-47, `HTTPStatus` 50-73, full path. |
| C1 363 (R-27 `browser_run.rs`, `http_routes.rs`) | applied | Moved under model-gateway with full paths and [M]; execution-core files given in full with [C]; row title now "Execution-core and model-gateway browser files". |
| C1 367 (R-31 `features/browser/*`, `BrowserChrome.tsx`) | applied | `src/features/browser/*` [NEW]; `apps/Frontend Plane/verevonv3/src/features/dashboard/home/BrowserChrome.tsx` [C]. |
| C1 23 and 350 (zdr-contract-sync "+95") | applied | "+93/-2, 95 lines touched" in both places. |
| C1 111, 201, 232 (`stealth.rs` and `lib.rs:164`) | applied | Removal of `pub mod stealth;` at `quarry-browser/src/lib.rs` 164 added to 1.3, the 2.2 evidence paragraph and the ADR-5 "Proceeds without" line. |
| C1 11 (convention) and about 60 basename citations | applied-modified | Expanded every citation C1 resolved (230, 283, 346, 358, 367, 29 ones were in lists sharing a directory). Rather than rewrite all list shorthand, the convention row now states the one allowed shorthand (files in the same directory as the first full path within one cell or sentence). |

## Segment-specific additions

1. Section 1.2: SC1 to SC12 mapped to W1 or W2 exit in a new table after the criteria; SC2a and SC2b; SC5 PageCache probe; SC6 per F2's WS5.C move; SC7 and SC10 rewritten.
2. Section 1.3: "Apify-class capabilities: where they are covered" table, 10 rows (actor-style runs, run store, datasets, run console, schedules, webhooks, monitors, saved job definitions, key-value store and queue as separate products, proxies and marketplace), each mapped to a slice or epic id or an explicit "Not in scope" (sources: arch-B section 2 coverage matrix, epics E3.5, E4.1, E4.4, E4.5, WS9, WS10, WS15).
3. Section 2 decision ids unchanged; K1-11, K1-14, K1-17, K1-18, K1-19 applied as above.
4. Line "Owners: workstream and decision owners are plane roles in this draft; naming individuals is open question OQ-owners in 7.1." added under the section 1 heading. The seg7 fixer must add OQ-owners to 7.1.

## Extra fixes not from a finding

- 1.4 Risk-gates row named `browser_risk.rs` and `provenance.rs` without a path; now `apps/Model Plane/rust/services/execution-core/src/browser_agent/browser_risk.rs` [C] and `.../execution-core/src/provenance.rs` [C].
- R-23 had literal pipes inside code spans (`sensitive|all`, `metadata|full|none`) that split table cells; reworded.
- Slice count in at-a-glance item 2 and 2.4: 162 to 166 (WS1.A0, the WS1.A split, WS4.K, WS14.S0b).

## Cross-segment items other fixers must match

- Slice table and 4.4 (seg7): W1 core 132.25, gated 16, optional 6, core plus gated 148.25; eng-weeks 29.65; risk-adjusted 34 to 44; calendar 6.6 to 8.6 weeks at 6.5 engineers; W2 510.5 (447.5 core, 63 gated), risk-adjusted 133 to 184, 14 to 19 weeks at 12 engineers; W1 plus W2 167 to 228 risk-adjusted engineer-weeks; critical path 50 d naming WS1.A1 (F3's WS8 sentence still says the 50 d "must be recomputed" and should now say it holds); 166 slices.
- Exit gate wording and exit evidence 5 (3482): WS5.A to WS5.H including WS5.C, WS6.A, WS4.K, WS1.A0 to WS1.C, PRE-1 and PRE-2.
- 7.2: PR 1 WS4.A with WS4.K, PR 2 WS6.A, PR 3 PRE-2, PR 4 WS2.A; PR 1 and PR 2 share `knowledge/quarry.rs`; PR 2 and PR 4 (WS2.A) share `openapi.yaml`.
- 4.1 one-line table (compose order WS3.A, WS5.A, WS1.A0, WS1.A2; openapi WS2.A, WS6.A, WS5.A; `quarry.rs` WS4.A, WS6.A) and the "Primary editor of" column name; my 2.1 uncommitted-files table points to it as "section 4.1".
- 4.5 flag table: sessions 40, runs 32, heavy lane 1; boot check WARN-only until RSS recorded; gateway `BROWSER_HUMAN_BROWSE` and `BROWSER_ATTACH` default `0`; `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` values are `0` and `1`, not `on`.

## New inconsistencies noticed, not fixed

1. R-26 still carries an unrelated `[UNVERIFIED]` ("`heavy_browser_busy` is a `details.reason`") and X-20 a `[UNVERIFIED]` on the 125 s front-proxy deadline; neither has a finding.
2. No consolidated programme risk register and no "before" baselines for SC3 and SC4 (K1 "gaps vs mission"); not added, they need an owner and numbers.
3. The slice id `WS14.S0b` breaks the plan's letter convention (WS14.A to WS14.I); F5 chose it, so it was kept.
4. The `CHAT_GAP_CLOSURE_PLAN_2026-10-01` named in X-11 (OD13 bundle) is cited without a path; not checked, no finding.

<!-- COMPLETE -->
