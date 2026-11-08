
### 4.3 Whole-plan merge order (numbered, W1 then W2, with reasons) and critical path

Each position is a merge train. PRs inside a position may merge in any order unless the text says otherwise (they are file-disjoint or append-only). A position starts when its stated prerequisites are merged, not when the previous position finishes. W1 is positions 1 to 11 and W2 positions 12 to 27. Hot-file rules from 4.2 apply on top.

| Pos | Wave | Train | Slices | Reason |
|---|---|---|---|---|
| 0 | process | Baseline | PRE-1 commit of the in-flight work (X-3) | Every later lane rebases on it; the sync gate needs the 28 fixtures. |
| 1 | W1 | Hotfix and zero-dependency set | WS4.A, WS6.A, WS2.A, WS3.A, WS5.A, WS3.E, WS1.A | Risk first, no inbound dependency, file-disjoint. WS3.E (deletes `stealth.rs`) must precede WS3.F. WS1.A is the Go, proto, registry and compose stream; it merges only after X-1, and its Quarry validator stream waits for position 2. WS4.B forensics (steps 0-2) run before WS4.A deploys. |
| 2 | W1 | `agent_routes.rs` split | WS2.B | Needs WS2.A. Two-working-day freeze. Head of the critical path into WS2.C, WS6.B, WS7.D, WS8.F. |
| 3 | W1 | Run control and leak closure | WS2.C, WS2.D, WS2.E, WS3.B, WS4.B (purge), WS4.D, WS5.B | WS2.C/D start from the shared skeleton PR. WS3.B gives the live regression net before WS3.C/D. WS4.D closes log and meter leaks before WS4.E and WS6.C. The purge runs once WS4.A is on every replica. |
| 4 | W1 | ZDR carrier and request-layer chain | WS4.C, WS4.E | WS4.E edits the `routes.rs` layer chain and `telemetry.rs`; it merges before WS5.D and WS10.A. |
| 5 | W1 | Per-page emulation | WS3.C | First of the serial `chromiumoxide.rs` queue. |
| 6 | W1 | Deadline table | WS3.D | About 20 call sites; one-day window so WS12.B, WS13.C, WS17.E, WS8.E rebase once. |
| 7 | W1 | ScrapeOptions contract | WS5.D | Rewrites the scrape handler and `ScrapeRequest`; WS7.C, WS13.E/F, WS16.G land after it. |
| 8 | W1 | Derive API and delegated browser bearer | WS1.B | Extends WS1.A's `Repository`; longest chain element. Shared-browser, budgets-in-grant and policy-in-grant wait for it. |
| 9 | W1 | Driver cap, log lint, clean_html, scrape record | WS3.F, WS4.F, WS5.E, WS5.F | Each needs its predecessors (WS3.C/D; WS4.D/E; WS5.D). File-disjoint apart from one-line route additions. |
| 10 | W1 | Postgres grants, challenge taxonomy, matrix, consumers | WS1.C, WS3.H, WS4.G, WS5.G, WS5.H, WS5.C (opt), WS2.F (opt) | WS1.C needs WS1.B semantics; WS3.H needs WS2.B and WS3.D; WS4.G is new files and append-only golden rows; WS5.G needs WS5.D deployed; WS5.H's schema half follows WS5.D. |
| 11 | W1 | Plumbing and gated tail | WS3.J, WS3.I (g), WS3.G (opt), WS4.H (g), WS4.I (g), WS4.J (g) | WS3.J needs WS3.H and the sibling planes; the rest wait for ADR-5, ADR-10, ADR-4, the Model Plane Z-05a/b commit, legal review. |
| 12 | W2 | Request-id codemod | WS10.A | Mechanical change that conflicts with every open route PR: merge first, one-day freeze. |
| 13 | W2 | New-file foundation set | WS7.A (g), WS7.B, WS11.A, WS12.A, WS12.G, WS14.A, WS14.B, WS15.A, WS15.B, WS16.A, WS8.A, WS13.A, WS13.D, WS6.F, WS17.A, WS17.B, WS17.C, WS10.B, WS11.G | Libraries, docs, fixtures, schema-only and mechanical PRs. WS15.B goes first and alone among control-store edits. WS10.B repairs the buf gate and allocates 14-18 and 150-156. |
| 14 | W2 | Scrape, policy and crawl-filter wiring | WS7.C, WS7.F, WS13.B, WS14.C, WS7.E | WS7.C replaces `RunPolicy::default()` in the scrape family after WS5.D; WS7.E edits `search_routes.rs` last; WS14.C is inert until WS14.D. |
| 15 | W2 | Admission queue | WS6.B, WS7.D, WS12.C, WS12.D, WS10.C, WS10.D, WS8.B | Gate order is the R-7 contract. WS6.B first because planner stage 2 needs enforced budgets. |
| 16 | W2 | Driver queue after WS3.F | WS3.K, WS3.G, then WS8.E, WS12.B, WS12.F, WS13.C, WS13.G, WS17.E, WS17.G, WS3.L (holds its place) | R-31 order; see 4.2 row 2 for the WS8.E proposal. Live lane (WS3.B) must be green. |
| 17 | W2 | Capacity and metering | WS6.C, WS6.D, WS6.I, WS6.J, WS6.G | WS6.C needs WS2.D; WS6.D needs the Control credential for `billing_core` mode (static first); WS6.J needs WS6.C and WS2.D. |
| 18 | W2 | Crawl scope and job lifecycle | WS14.D, WS15.C, WS15.D, WS14.E, WS15.E, WS14.F, WS14.G, WS15.F, WS15.G, WS14.I, WS15.I (g), WS15.H (g), WS15.J, WS6.H, WS6.K, WS14.H (g), WS15.K (opt) | `crawl-scope-v1` ahead of `lifecycle-v1`. WS15.J needs WS14 deployed and WS6.A live. WS6.H and WS6.K follow WS15.B (same `jobs` store lists). |
| 19 | W2 | Planner queue (Model Plane Rust) | WS11.B, WS11.C, WS11.D, WS11.H, WS11.E, WS11.F, WS12.E, WS16.B, WS16.I, WS9.E, WS8.G, WS7.L, WS10.F | WS11.B introduces the single run-parameter helper; WS11.D restructures the loop; WS3.J, WS8.G, WS12.E rebase after them. WS10.F and WS11.H need `0044`/`0045` committed. |
| 20 | W2 | Broker, proto and registry queue | WS6.E, WS8.C (g), WS7.K (g), WS7.M (opt) | After WS1.C. Numbers are pre-allocated (4.2 ledger), so order is by readiness. |
| 21 | W2 | Control Plane queue | WS7.H, WS7.I, WS7.J, WS16.C, WS17.H | All gated (ADR-1, DR-15.1, X-8). Registry PR 1 gains rows only for designs that have reached merge (R-13). |
| 22 | W2 | Trace persistence and telemetry | WS10.E, WS10.H, WS10.I, WS10.G (g) | WS10.E after WS10.D; WS10.H after WS4.D/E/F; WS10.G waits for ADR-8 part 2. |
| 23 | W2 | Attach core and execution lanes | WS8.F, WS8.H, WS8.J, WS8.K (opt) | After WS1.B, WS2.C, WS8.E and the shared `step.rs` and receipt edits. WS8.H is the GA gate for `QUARRY_EDGE__ATTACH`. |
| 24 | W2 | Scrape formats, actions, extraction | WS13.E, WS13.F, WS13.I, WS16.D, WS16.E, WS16.F, WS16.G, WS5.I (unsched), WS16.J (unsched) | After WS13.B/C/D and WS5.D; WS16.D to G in order, with DR-15.1 and WS16.C before WS16.G's second tier. |
| 25 | W2 | Profile stores and erasure | WS17.D, WS17.F, WS17.I, WS17.J (opt) | E1 after P1 (migration 0015, R-1); P2 after P1 and WS2.B. |
| 26 | W2 | SPA and gateway queue | WS9.A, WS9.F, WS9.G, WS9.C, WS9.B, WS9.D, WS9.I, WS9.H, WS8.D, WS8.I, WS9.J, WS13.H, WS16.H, WS13.J, WS9.K, WS9.L | `BrowserChrome.tsx` takes small hunks in this order. WS9.A production flag waits for WS4.A deployed; WS9.I and WS8.I wait for WS2.C deployed; WS8.D needs WS8.C. New-module-only slices may merge earlier. |
| 27 | W2 | Redaction closeout | WS12.H (opt), WS7.G | Optional legacy-row scrub needs a product decision; WS7.G (6b) waits for WS14/WS15 deployed. |

WS10.A sits at the head of W2 on purpose (it is a codemod); the one-day freezes of WS3.D and WS10.A are announced to every open PR.

**W1 exit gate (after position 11).** Merged and deployed on the rebuilt local stack: WS1.A to WS1.C with `BROWSER_GRANT_ENFORCEMENT=strict` on the production-overlay stack; WS2.A to WS2.E; WS3.A to WS3.F, WS3.H, WS3.J; WS4.A to WS4.G with the purge recorded; WS5.A, WS5.B, WS5.D to WS5.H; PRE-1 done. Gated or optional items (WS4.H, WS4.I, WS4.J, WS3.I, WS3.G, WS5.C, WS2.F) may trail without blocking W2, except that WS4.A must be deployed before `VITE_BROWSER_OMNIBOX_SEARCH` is on in production.

**Critical path.** `WS1.A (10 d) -> WS1.B (10 d) -> WS8.F (12 d) -> WS8.H (6 d) -> WS8.I (7 d) -> WS8.J (5 d)` = 50 engineer-days, the shared browser. With D1's two-engineer split on WS1.A/B it is 46 to 48; risk-adjusted 13 to 18 weeks.

| Step | Start (eng-day) | Finish |
|---|---|---|
| WS1.A prod-mode contract harness; broker issues kind, zdr, TTL and cap-bound grants | 0 | 10 |
| WS1.B `DeriveRunGrant`, model-gateway `POST/DELETE /v1/browser/grants`, gateway enforcement | 10 | 20 |
| WS8.F Quarry attach core: tab ledger, caller resolution, per-tab control | 20 | 32 |
| WS8.H execution lanes: human and agent actions run concurrently (GA gate) | 32 | 38 |
| WS8.I gateway and SPA: start with attach, agent tab, Reveal, Take over, Give back | 38 | 45 |
| WS8.J phase 3: deliverable and handoff marks, turn-scoped release, orphan sweep | 45 | 50 |

Other chains, all starting on day 0 in parallel: WS16.B to WS16.H 30 d; WS10.C to WS10.I 28 d; WS1.A, WS1.B, WS7.K 26.5 d; WS5.A, WS5.B, WS5.D, WS13.E, WS13.F, WS13.I 26 d; WS14.C, WS14.D, WS14.E, WS14.I 18 d; WS15.B, WS15.E, WS15.I, WS15.J 17.5 d; WS17.C, WS17.D, WS17.I 17 d; WS15.B, WS6.H 9.5 d. Two chains end in a decision or epic, so the graph understates them: **planner default-on** (WS11.F stage 3) needs WS1.A, WS1.B, WS6.B, WS6.E, WS12.A to WS12.E, F-t2-044 Tier B and OD13; plan on engineer-day 31 at best, 37 to 43 realistically, plus the live-success floor. **Scheduled crawls** (WS15.J) are 17.5 d on the graph plus the one-week soak of WS15.I, about 22.5 d; WS14.I needs its own soak.

### 4.4 Effort roll-up

Assumptions. Days are the designs' own engineer-days (midpoints for ranges: WS2.C 5.5, WS2.D 6.5, WS9.J 9, WS5.I 13.5, WS16.J 9); engineer-weeks = days / 5. Columns: core = scheduled and unconditional; gated = blocked by a named decision but scheduled once it lands; opt = optional; unsched = design-level only and in no total. Calendar weeks assume 80% focus. WS6.A (1.5 d) is pulled into W1 as a hotfix and WS3.K (8 d) counts in W2. Sibling-owned days outside the Quarry figure (about 5.5 d: WS6.B +1.5 Model Plane, WS6.E +2 Model Plane and +2 execution-core) are not in the totals. Risk multipliers: W1 x1.15 to x1.5 (small, file-level, mostly verified); W2 x1.3 to x1.8 (cross-plane review, rebases on uncommitted diffs, ADR waits, migration renumbering). The audit's efforts were raised for 33 items after verification, and several designs say the audit understated (D2 S3 S to M, D11 M to L, D14a step 1 S to 10 d, D15 M to L, D10 L to XL).

| Wave | core d | gated d | opt d | unsched d | core+gated d | eng-weeks | risk-adjusted eng-weeks | calendar |
|---|---|---|---|---|---|---|---|---|
| W1 | 113.75 | 12 | 10 | 13.5 | 125.75 | 25.1 | 29 to 38 | 6.5 engineers: 5.6 to 7.3 weeks |
| W2 | 447 | 63 | 19.5 | 9 | 510 | 102.0 | 133 to 184 | 8 eng 21 to 29 wk; 12 eng 14 to 19 wk; 16 eng 10 to 14 wk |
| W1+W2 | 560.75 | 75 | 29.5 | 22.5 | 635.75 | 127.2 | 162 to 221 | about 5 to 7 months end to end |

The critical path (50 d, 13 to 18 weeks risk-adjusted) is a floor, not the estimate. W1 is throughput-bound (about 6 to 8 weeks at 6.5 engineers). W2 at 12 engineers is about 14 to 19 weeks, where throughput and critical path meet; beyond about 14 engineers the critical path binds and extra people do not shorten W2.

Per lane (core+gated days; opt and unsched shown separately):

| Lane | W1 d | W2 d | opt d | unsched d | eng-weeks | risk-adjusted eng-weeks | engineers to finish W2 in about 16 weeks |
|---|---|---|---|---|---|---|---|
| L1 GRANTS-BROKER | 26 | 20 | 4 | 0 | 9.2 | 11 to 15 | 1.5 |
| L2 ADMISSION | 16 | 14 | 1 | 0 | 6.0 | 7 to 10 | 1 |
| L3 DRIVER | 25 | 34 | 5 | 0 | 11.8 | 15 to 20 | 1.5 |
| L4 ZDR-TENANT | 26.75 | 0 | 0 | 0 | 5.3 | 6 to 8 | 1 (W1 only) |
| L5 SCRAPE | 24 | 44 | 4 | 22.5 | 13.6 | 17 to 23 | 1.5 |
| L6 SPA-GATEWAY | 6.5 | 60 | 2 | 0 | 13.3 | 17 to 24 | 2 |
| L7 CRAWL-JOBS | 1.5 | 87.5 | 3.5 | 0 | 17.8 | 23 to 32 | 2 |
| L8 PLANNER-MP | 0 | 69 | 0 | 0 | 13.8 | 18 to 25 | 2 |
| L9 CONTROL-PLANE | 0 | 25 | 0 | 0 | 5.0 | 6 to 9 | 1 (shared with L15's billing-core work) |
| L10 SHARED-BROWSER-QUARRY | 0 | 30.5 | 5 | 0 | 6.1 | 8 to 11 | 1 |
| L11 PROFILES | 0 | 37.5 | 3 | 0 | 7.5 | 10 to 14 | 1 |
| L12 TRACE | 0 | 37.5 | 0 | 0 | 7.5 | 10 to 14 | 1 |
| L13 WEB-POLICY-QUARRY | 0 | 20.5 | 0 | 0 | 4.1 | 5 to 7 | 0.5 (pairs with L14) |
| L14 REDACTION | 0 | 13 | 2 | 0 | 2.6 | 3 to 5 | 0.5 |
| L15 CAPACITY-METERING | 0 | 17.5 | 0 | 0 | 3.5 | 5 to 6 | 1 |

Lanes whose serial sum exceeds the roughly 65 working days of a 13-week W2 need a second engineer or a split: L7, L8, L3, L5, L6. Running every lane flat out needs about 17.5 engineers; at 12, L13 and L14 share one person, L9 and L15 share one, and L4 and L2 hand their W1 people to L10 and L12.

Not in any number: unscheduled design-level work (about 22.5 d if pulled in: WS3.L, WS5.I 13.5, WS9.K, WS9.L, WS16.J 9), D6's L-series (about 26 d), F-t2-024 E1 to E4 (20 to 30 d, W3), and the epics of W3 to W5 (48 items, one parked), which carry their own estimates in section 3.3 onward. The sibling days above add about 5.5 d to W2.

### 4.5 Feature flags and rollout table

Every behaviour change ships behind a default-off or observe-mode flag, and production defaults flip only after a recorded L2 run (6.2). Prefixes: edge `QUARRY_EDGE__*` (separator `__`; compose anchors drop the prefix, for example `USAGE_BROWSER_METRICS`), driver `QUARRY_BROWSER_*`, Go services `QUARRY_*`, Model Plane `<SERVICE>_*`, gateway unprefixed, SPA `VITE_*`. New flags use only the `QUARRY_EDGE__` form (the raw single-underscore `QUARRY_EDGE_AUTH_DEV_BYPASS` and `QUARRY_EDGE_ARTIFACT_BACKEND` are read case-sensitively by the production refusals: PRE-2). An unparsable value aborts boot in every new reader; the old `EdgeConfig` loader silently falls back to defaults (S9 `config.rs:334-341`) and must not be copied. Vocabularies differ by design (`off|observe|enforce` in WS7, WS14, WS17; `off|shadow|on` in WS6; `off|shadow|skip|on` in WS5; `lenient|strict` in WS5 and WS14); semantics are common: `observe` equals `shadow`.

| Flag / env var | Default | Owner | Removal condition |
|---|---|---|---|
| `GATEWAY_RESULT_CACHE_CONTENT` | `on` (`off` disables the two content caches only) | WS4.A | keep (emergency lever; the only safe rollback posture is `GATEWAY_CACHE_REDIS_URL` unset) |
| `QUARRY_ZDR_LOG_FILTER` | on | WS4.E | folds into the WS10.H span processor |
| `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` | observe first, then on | WS4.H (WS15.S1, WS17.B call it; R-25) | after one release at on and customer notice; keep as rollback until then |
| `QUARRY_EDGE__ZDR_TOMBSTONE`, `QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS`, `QUARRY_ZDR_TOMBSTONE_DRY_RUN` | off | WS4.J | enable per environment after contract and legal review; keep the retention knob |
| `QUARRY_UPDATE_GOLDEN` | unset | WS4.G | keep (test tool) |
| `QUARRY_CRAWL_DEFAULT_MAX_PAGES`, `QUARRY_CRAWL_HARD_MAX_PAGES`, `QUARRY_CRAWL_DEFAULT_MAX_DEPTH`, `QUARRY_CRAWL_HARD_MAX_DEPTH` | 1000, 5000, 5, 10 (`0` on a default disables the default, never the hard ceiling) | WS6.A (replaces D14a's `QUARRY_EDGE__CRAWL_DEFAULT_MAX_PAGES` and `CRAWL_MAX_PAGES_CAP`) | keep; `LIMITS_SOURCE=billing_core` supplies plan values above these floors |
| `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` | `0` (log-only `would_reject`) until WS11's `parse_typed_error` mapping and the gateway error translations are deployed, then `1` (R-10 overrides the design's default of 1) | WS2.C | after two releases with no `would_reject` noise |
| `QUARRY_EDGE__RUN_SWEEPER`, `RUN_SWEEPER_DRY_RUN`, `RUN_SWEEP_INTERVAL_S` | `1`, `0`, `15`; first deploy with dry-run `1` for 24 h | WS2.D | keep the sweeper switch |
| `QUARRY_EDGE__AGENT_RUN_IDLE_TTL_S`, `AGENT_RUN_MAX_TTL_S`, `AGENT_RUN_TTL_GRACE_S`, `INTERACTIVE_RUN_IDLE_TTL_S`, `INTERACTIVE_RUN_MAX_TTL_S`, `TAKEOVER_IDLE_TTL_S` | 300, 1800 (cap 3600), 30, 900, 14400, 1800 | WS2.D | keep; WS8 may change the interactive numbers |
| `QUARRY_EDGE__MAX_CONCURRENT_RUNS`, `MAX_CONCURRENT_RUNS_PER_ORG`, `MAX_INTERACTIVE_RUNS_PER_ORG`, `RUN_CAP_RETRY_AFTER_S` | 32, 8, 6, 10 (`0` = unlimited); placeholders until RSS is measured at 8, 16, 32 contexts | WS2.D | keep; WS6.G adds plan-aware values |
| `VITE_FLEET_CONSOLE` | off (route hidden) | WS2.E | when a fleet producer exists |
| `QUARRY_BROWSER_NO_SANDBOX` | `0` (refused when `ENVIRONMENT` is prod) | WS3.A | keep as the explicit local opt-out |
| `QUARRY_BROWSER_DEFAULT_VIEWPORT`, `QUARRY_BROWSER_PAGE_HYGIENE` | `1280x800`, on | WS3.C | hygiene switch removed after one release |
| `QUARRY_BROWSER_DEADLINE_SCALE` | `1.0` (range (0,2]) | WS3.D | keep |
| `QUARRY_BROWSER_MAX_SESSIONS`, `MAX_PAGES_PER_SLOT`, `SESSION_IDLE_S`, `SESSION_MAX_AGE_S`, `REAPER` (`0\|log\|1`), `REAPER_INTERVAL_S`, `PROBE_INTERVAL_S` | 12, 24, 3600, 21600, `log` for one week then on, 30, 15 | WS3.F | `log` phase ends after one week; caps kept |
| `QUARRY_BROWSER_POOL_SIZE`, `QUARRY_BROWSER_RETIRE_AFTER_PAGES` | 1, 100 | WS3.G | keep; raise only after the memory budget is measured |
| `QUARRY_CHALLENGE_CLASSIFIER` (`v2\|legacy`), `QUARRY_CHALLENGE_ENFORCE`, `QUARRY_BLOCK_ESCALATION` | `v2`, enforce (`log` while validating), `legacy` until ADR-5 then `honest` | WS3.H, WS3.I | `legacy` branches removed one release after `honest` is default |
| `QUARRY_BROWSER_TRUSTED_INPUT` | `off`, `on` after the live lane is green two weeks | WS3.K | removed one release after GA |
| `QUARRY_EDGE__DOC_ROUTING` (`off\|shadow\|skip\|on`), `DOC_PARSE_TIMEOUT_MS`, `DOC_PARSE_CONCURRENCY`, `DOC_PARSE_ISOLATION` (`process\|inprocess`) | `shadow`; isolation required for `on` in multi-tenant production | WS5.B, WS5.C | after `on` has run a release; isolation switch kept |
| `QUARRY_EDGE__SCRAPE_OPTIONS_ENABLED`, `SCRAPE_REQUEST_VALIDATION` (`lenient\|strict`), `SCRAPE_STRICT_AFTER`, `SCRAPE_INLINE_MAX_BYTES`, `SCRAPE_RECORDS` | `1`, `lenient`, unset, 1 MiB (ceiling 8 MiB), on | WS5.D, WS5.F | strict is the only mode after the date and a week of clean logs |
| `QUARRY_EDGE__BUDGET_ENFORCEMENT` (`off\|shadow\|on`), `BUDGET_ATTEMPT_FACTOR`, `BUDGET_BROWSER_SECOND_USD` | `shadow` 24 h minimum then `on`; 2; 0.00003 (set from the billing-core price table before `on`) | WS6.B | keep `off` as rollback |
| `QUARRY_EDGE__USAGE_BROWSER_METRICS` (anchor `USAGE_BROWSER_METRICS`), `QUARRY_EDGE__USAGE_LLM_METRICS` (anchor `USAGE_LLM_METRICS`) | `off`, then `shadow` a week, then `on` when Lago codes exist | WS6.C, WS16.G (must not read WS6.C's switch) | keep until invoices reconcile |
| `QUARRY_EDGE__LIMITS_SOURCE` (`static\|billing_core`), `LIMITS_FAIL_MODE` (`floor\|open\|closed`), `LIMITS_CACHE_TTL_S`, `LIMITS_STALE_MAX_S`, `LIMITS_BILLING_ORGS`, `QUARRY_EDGE__BILLING_CORE_URL`, `BILLING_CORE_SERVICE_TOKEN` | `static`, `floor`, 30, 900, canary list | WS6.D | canary list removed when `billing_core` is default; fail mode needs ADR-1 |
| `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM` (`off\|on`), `QUARRY_EDGE__GRANT_BUDGET_CLAIM` (`ignore\|enforce`) | `off`, `ignore` | WS6.E | `enforce` only after the broker deploys; removed after one release |
| `GATEWAY_RATE_CLASSES`, `GATEWAY_RATE_LIMIT_RPM_<CLASS>` | `on`; class numbers per design | WS6.F | keep |
| `QUARRY_EDGE__POOL_ORG_MAX_PERMITS` | `max(1, pool/2)` (`0` disables) | WS6.G | keep |
| `QUARRY_JOB_ADMISSION` (`off\|on`), `QUARRY_JOB_CAP_DEFAULT`, `QUARRY_JOB_BACKLOG_DEFAULT`, `QUARRY_JOB_BACKLOG_TTL`, `QUARRY_JOB_CLAIM_LEASE`, `QUARRY_JOB_CLAIM_BATCH` | `off`; 3, 200, 24h, 60s, 50 | WS6.H | `off` path removed after one release at `on` |
| `QUARRY_EDGE__METRICS_ENABLED`, `METRICS_ADDR`, `METRICS_TOKEN`, `DRAIN_DEADLINE_S` | on, `0.0.0.0:9092` (not published), required token, 30 (below the compose grace) | WS6.I, WS6.J | keep |
| `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` (`off\|observe\|enforce`), `WEB_POLICY_BASELINE_JSON`, `WEB_POLICY_REQUIRE_CLAIM`, `WEB_POLICY_FINAL_URL_CHECK`, `RUN_POLICY` | `enforce` (baseline equals today's behaviour); `observe` for any non-default baseline | WS7.C, WS7.G, WS7.J | `off` removed after claim-fed `enforce` is stable |
| `WEB_POLICY_CLAIM_ENABLED` (auth-core) | `1` once WS7.I is deployed | WS7.I | keep (Control disable lever) |
| `BROWSER_GRANT_ENFORCEMENT` (gateway, `off\|permissive\|strict`) | `permissive` outside prod, `strict` in prod | WS1.B | `off` removed once every stack has a broker |
| Broker: `DATABASE_URL`, `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT`, `BROWSER_BROKER_GRANT_TTL_SECONDS` (900, clamp 30-3600), `BROWSER_BROKER_MAX_SESSION_DOMAINS` (8), `BROWSER_BROKER_MAX_RUN_DOMAINS` (8), `BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER` (20) | as stated; production refuses to start without a database | WS1.A, WS1.C (one definition) | ephemeral flag dev only |
| `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` | empty in dev (Noop), set in prod and the contract overlay | WS1.A | keep (the name `BROWSER_BROKER_GRPC` in three docs is not read by any code) |
| `QUARRY_EDGE__HUMAN_BROWSE` (edge), `BROWSER_BROKER_HUMAN_BROWSE_ENABLED` (broker), `BROWSER_HUMAN_BROWSE` (gateway), `QUARRY_EDGE__HUMAN_RECEIPT_MODE` (`metadata\|full\|none`), `GRANT_VALIDATION_CACHE_MS` (0), `BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS` | edge and broker `1` in dev compose, `0` in the production overlay; gateway default not stated by the design (rollback is `off`); receipt mode `metadata` | WS8.B, WS8.C, WS8.D | three layer switches removed together at GA; receipt mode kept |
| `QUARRY_EDGE__ATTACH` (edge), `BROWSER_ATTACH` (gateway), `QUARRY_EDGE__ATTACH_IDLE_TTL_S`, `ATTACH_REQUIRE_DELEGATION` (`0`), `BROWSER_BROKER_MAX_ATTACH_DOMAINS` | off until WS8.H and the evidence; enable edge and gateway together | WS8.F, WS8.I, WS8.K | removed at GA (after WS8.H) |
| `VITE_BROWSER_OMNIBOX_SEARCH`, `VITE_CHAT_BROWSER_DOCK` | `1` in dev and staging builds, `0` in production builds until verified | WS9.A, WS9.G | after WS4.A deployed (omnibox) and after WS8 (interactive dock); then deleted |
| `QUARRY_EDGE__REQUEST_ID_ADOPT` | on (`0` safety valve) | WS10.A | after one release |
| `QUARRY_EDGE__TRACE_RING_EVENTS`, `TRACE_RING_MAX_BYTES`, `QUARRY_EDGE__NATS_AGENT_EVENTS`, `QUARRY_EDGE__MODEL_TRACE_NATS_{URL,USER,PASSWORD,QUEUE,SUBJECT_PREFIX}`, `QUARRY_TRACE_PRODUCER` | 512, 1 MiB; `0`; publishing is enabled by the URL | WS10.C, WS10.D, WS10.E | kill switches kept; `NATS_AGENT_EVENTS` removed after consumers are confirmed |
| `SESSION_CORE_TRACE_CONSUMER`, `SESSION_CORE_TRACE_RETENTION_DAYS` | `1` once shadow mode is healthy; 30 | WS10.F | keep |
| `QUARRY_TELEMETRY_EXPORT`, `QUARRY_TELEMETRY_ZDR_REGISTRY_CAP` | off; 100,000 | WS10.H | export stays off until the contract golden and WS4.F lint are green |
| `QUARRY_BROWSER_AGENT_PLANNER_MODE` (`off\|allowlist\|org_opt_in\|default_on`), `QUARRY_BROWSER_AGENT_LLM_ORGS`, `QUARRY_BROWSER_AGENT_PROCEDURE` (`browse/1\|legacy/0`), `..._MAX_CONSECUTIVE_FAILURES`, `..._MAX_TOTAL_FAILURES`, `..._MAX_PLANNER_TOKENS`, `BROWSER_AGENT_MAX_TOKENS_PER_RUN`, `QUARRY_BROWSER_AGENT_DEFAULT_ON_ACK` | `off`; stage-3 guard needs `grants,budgets,redaction,evals` | WS11.B to WS11.F | legacy `QUARRY_BROWSER_AGENT_LLM` retired at stage 4 (one release at stage 3 without rollback) and removed from both composes in the same PR |
| org-core quota keys `browser_agent_llm_planner` (1 on, 2 off), `browser_agent_max_tokens_per_run` | unset | WS11.C | replaced by the WS7 claim carrier if ADR-1 chooses it |
| `QUARRY_EDGE__REDACTION_MODE` (anchor `QUARRY_REDACTION_MODE`), `QUARRY_BROWSER_FIELD_PROBE`, `QUARRY_EDGE__RECEIPT_TEXT_MODE` (`sensitive\|all`), `QUARRY_BROWSER_SCREENSHOT_MASK`, `QUARRY_BROWSER_SCREENSHOT_OOPIF` (`allow\|mask`), `EXECUTION_CORE_REQUIRE_QUARRY_REDACTION_STAMP` | `enforce` (`shadow` and `off` refuse to boot in production); `on`; `sensitive`; `on`; `allow`; `0` then `1` after WS12.C deploys | WS12.B to WS12.F | shadow phase 3 to 5 days in staging; no production switch turns masking off |
| `QUARRY_EDGE__SCRAPE_CAPTURE` (`off\|on`), `SCRAPE_ACTIONS` (`off\|user\|on`), `SCRAPE_ACTIONS_MAX`, `SCRAPE_CAPTURE_INLINE_MAX_BYTES` (6 MiB), `SCRAPE_STITCH` (`off\|on`), `QUARRY_SCRAPE_STITCH_METHOD`, `QUARRY_SCRAPE_SESSION_ISOLATION` (on), `QUARRY_SCRAPE_HEAVY_PERMITS` (1) | capture `off` in committed compose and `on` in dev first; actions `off` committed, `user` in dev, `on` only after ADR-6; stitch `off` | WS13.A to WS13.G | actions `on` removes the committed `off`; isolation kept |
| `QUARRY_EDGE__CRAWL_SCOPE_MODE` (`off\|observe\|enforce`), `CRAWL_ROBOTS`, `ROBOTS_UNREACHABLE` (`disallow\|allow`), `CRAWL_REQUEST_VALIDATION` (`lenient\|strict`), `CRAWL_BATCH_MAX_URLS`, `QUARRY_CRAWL_DELAY_MAX_S` (10, ceiling 60), `QUARRY_ROBOTS_CACHE_MAX_ENTRIES` (10,000), `QUARRY_ROBOTS_FETCH_TIMEOUT_S` (8) | `observe`, `observe`, `disallow`, `lenient` | WS14.D, WS14.E, WS14.G, WS14.I | WS14.I flips scope to `enforce` after the soak; `off` removed one release later |
| `QUARRY_SCHEDULED_JOBS_ENABLED` | `0`; must stay 0 until WS14 is deployed and WS6.A is live | WS15.E | removed after the first week of live schedules |
| `QUARRY_EDGE__JOB_CONTROL`, `QUARRY_JOB_CONTROL_LOOP`, `QUARRY_CANCEL_GRACE_S` (120), `QUARRY_EDGE__JOB_STREAM_FOLLOW`, `JOB_STREAM_MAX` (500), `JOB_STREAM_MAX_PER_ORG` (20), `JOB_STREAM_MAX_AGE_S` (1800), `QUARRY_EDGE__JOB_RESULTS`, `QUARRY_JOB_TIMEOUT_{SCRAPE,BATCH,CRAWL}_S`, `QUARRY_SCHEDULE_CAPABILITIES`, `QUARRY_SCHEDULE_MIN_INTERVAL_S`, `QUARRY_SCHEDULE_REQUESTS`, `QUARRY_EDGE__SCHEDULE_POSTURE` (`observe\|enforce`), `QUARRY_INTERNAL_HMAC_REQUIRED`, `QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG` | job control on after a staging pass; loop on; others per design; posture `observe` until Control and Application approvals | WS15.B to WS15.I | capability flags flipped by S1, S2, S3 in turn |
| `QUARRY_EDGE__AI_FORMATS` (`0`), `AI_FORMATS_ZDR` (`refuse`), `AI_FORMATS_HARVEST` (`off\|context\|prefill`, `context`), `AI_FORMATS_REPAIR` (1), `MODEL_GATEWAY_AI_CHAT_METERING` (`off\|service\|all`, `service`), `MODEL_GATEWAY_SERVICE_RPM` (120), `MODEL_GATEWAY_EXTRACT_TOOL` (`0`) | as stated | WS16.B to WS16.G, WS16.I | `AI_FORMATS_ZDR=allow` only after the E2 ZDR proof; extract tool flag removed after WS16.G ships |
| `QUARRY_EDGE__PROFILE_ENCRYPTION` (`off\|compat\|on`, plus `REQUIRED=1`), `PROFILE_KEY_PROVIDER` (`env\|org_core`), `PROFILE_OWNER_ENFORCE` (`off\|observe\|enforce`, default `observe`), `ORG_CORE_KEYS_ENABLED` (`0`, ships dark), `QUARRY_BROWSER_PROFILE_CAPTURE_V2` (`0` one release), `QUARRY_BROWSER_PROFILE_IDB`, `QUARRY_BROWSER_PROFILE_MAX_BYTES`, `..._SESSION_COOKIE_MAX_AGE_S` (604800) | as stated | WS17.B to WS17.I | encryption moves one step per fully rolled-out deploy; sealed rows have no rollback to plaintext writes |

Rollout rules that cut across the table (resolutions R-23, R-26, R-28, R-29 of section 2.3).
- **One logical switch per feature, one per layer.** Human browsing is on only when broker `BROWSER_BROKER_HUMAN_BROWSE_ENABLED`, model-gateway, `QUARRY_EDGE__HUMAN_BROWSE`, gateway `BROWSER_HUMAN_BROWSE` and the SPA `VITE_*` flags are all on; attach likewise (`QUARRY_EDGE__ATTACH`, `BROWSER_ATTACH`). Deploy order: broker, model-gateway, Quarry, gateway. Production stays off everywhere until X-13 and ADR-3 are answered; the rollout matrix lives in the WS8 runbook.
- **Planner.** W1 and W2 evidence recipes leave `QUARRY_BROWSER_AGENT_LLM` and `QUARRY_BROWSER_AGENT_PLANNER_MODE` unset (deterministic planner); a recipe that sets planner mode names it.
- **Capacity.** `QUARRY_BROWSER_MAX_SESSIONS` must exceed `QUARRY_EDGE__MAX_CONCURRENT_RUNS` plus the scrape and heavy-lane budget; the driver reaper (idle 3600 s, max age 21600 s) sits above every run-sweeper number and the human-browse absolute grant TTL (4 h) equals the interactive backstop (14400 s); a boot check asserts the ordering.
- **Receipt text.** `QUARRY_EDGE__HUMAN_RECEIPT_MODE` and `QUARRY_EDGE__RECEIPT_TEXT_MODE` stay separate flags over one builder, `redacted_for_record`; the stricter wins for human steps.
- **Mirrored defaults.** The live `apps/Ingestion Plane/docker-compose.yml` [M] defaults `QUARRY_EDGE__AUTOPROMPT` and `SEMANTIC_RERANK` to 0, the committed one to 1, and the untracked switch test asserts 0: never commit one without the other.

**Per-wave deployment-evidence recipe.** Rebuilt local images, a running stack, and a verified end-to-end flow with a stated observable assertion (6.2 defines the standard).

Wave 1 exit evidence, on the rebuilt local stack:
1. WS4.A two-org Redis flow and ZDR bypass counters, WS4.B purge record; WS6.A 7-page crawl; WS2.A `evaluate` answered 403 then `get_content` returns `Example Domain` (the three PRs of 7.2).
2. WS2.C/D: a user takeover makes the next agent step 409 `USER_TOOK_OVER`; a viewer-less interactive run expires only after 900 s idle (dry-run logs first); a 33rd run returns 429 `org_run_cap` or `global_run_cap` with `Retry-After`.
3. WS3.A/C: `ps` shows zero `--no-sandbox` processes while a browser step succeeds, in dev and the production overlay; two sessions with different viewports do not relaunch Chromium (process start time unchanged). WS3.F: the 13th session returns 429 `browser_session_cap`; `pkill chromium` then a new run succeeds; `/ready` carries `browser`.
4. WS1.A to WS1.C: the production-overlay quarry-edge boots with the validator URL set; an AI browser run end to end through the gateway with `BROWSER_GRANT_ENFORCEMENT=strict`; a broker restart keeps an issued grant valid; `bash scripts/tests/browser-grant-prod-contract.sh` green with legs B and C turned from expected-fail to green.
5. WS5.B/D: a PDF through `/v1/scrape` returns text with `metadata.content_kind`; an inline ZDR scrape returns markdown and writes no artifact row.
6. Contract gates: `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`, `node --test scripts/tests/zdr-contract-schema.test.mjs`.

Wave 2 exit evidence:
1. Shared browser (WS8, WS9): a human session opens, the agent attaches in an agent-flagged tab, the user takes over (409 on the agent's next step), gives back and stops; the same run is visible in the dock with the evidence panels.
2. Planner (WS11): stage 1 `allowlist` on a dev org; stage 2 `org_opt_in` production-overlay run with a grant; a budget exhaustion yields 402 `BUDGET_EXHAUSTED` and an honest completion.
3. Crawl and jobs (WS14, WS15): the fixture-site crawl denies off-site and robots-disallowed URLs with typed denials; cancel, pause, resume; a schedule fires on its cron and a posture-violating fire shows `schedule_skipped` (`QUARRY_SCHEDULED_JOBS_ENABLED=1` only after WS14.I).
4. Scrape (WS13, WS16): `formats:[screenshot,pdf]` with `actions` returns captures; a `json` format over a real Model Plane grant returns schema-valid data and meters `quarry.extract.llm`.
5. Profiles (WS17): saved and restored across a Quarry restart; envelope rows unreadable without the key; erasure removes them.
6. Trace, metrics, redaction, policy (WS10, WS6, WS12, WS7): `GET /api/v1/runs/{id}/trace` after an AI run; `/metrics` on the internal listener; graceful drain with a live stream; a typed password never appears in receipts, events, artifacts or the planner prompt; a `ResolvedPolicy` denial shows `policy.denied` with `decision_source`.
