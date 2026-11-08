## 4. PARALLEL LANES, MERGE ORDER, CRITICAL PATH

### 4.1 Lanes

A lane is a team's git worktree and branch family. Lanes are file-disjoint with respect to the *concurrent* work inside a wave; where two lanes still meet in a hot file the section 3 queue order decides who merges first (a lane never waits on another lane's PR to be written, only to be merged). Slice labels read `slice wave days (flag)`; `(opt)` optional, `(gated)` blocked by a named decision, `(unsched)` design-level only. Every one of the 162 slices in section 1 belongs to exactly one lane.

| Lane | Scope | Slices in internal merge order |
|---|---|---|
| L1 | GRANTS-BROKER (Model Plane Go broker, proto, model-gateway grants, principal registry) | WS1.A W1 10d; WS1.B W1 10d; WS1.C W1 6d; WS6.E W2 5d; WS8.C W2 8.5d (gated); WS7.K W2 6.5d (gated); WS7.M W2 4d (opt) |
| L2 | ADMISSION (quarry-edge run control, agent_routes/*) | WS2.A W1 1.5d; WS2.B W1 2.5d; WS2.C W1 5.5d; WS2.D W1 6.5d; WS2.F W1 1d (opt); WS6.B W2 5d; WS7.D W2 4.5d; WS6.G W2 4.5d |
| L3 | DRIVER (quarry-browser chromiumoxide.rs queue) | WS3.A W1 2d; WS3.B W1 2d; WS3.E W1 1d; WS3.C W1 3d; WS3.D W1 3d; WS3.F W1 6d; WS3.H W1 5d; WS3.I W1 3d (gated); WS3.G W1 5d (opt); WS8.E W2 6d; WS12.B W2 4d; WS13.C W2 4d; WS3.K W2 8d; WS13.G W2 5d; WS12.F W2 7d; WS3.L W1 -d (unsched) |
| L4 | ZDR-TENANT (gateway search cache + Quarry ZDR/log hygiene/matrix) | WS4.B W1 1d; WS4.A W1 3d; WS4.D W1 1.5d; WS4.C W1 0.75d; WS4.E W1 3d; WS4.F W1 2.5d; WS4.G W1 6d; WS4.H W1 2.5d (gated); WS4.I W1 1.5d (gated); WS4.J W1 5d (gated) |
| L5 | SCRAPE (quarry-transform/runtime/edge scrape path, extraction, capture formats) | WS5.A W1 2d; WS5.H W1 3d; WS5.B W1 5d; WS5.D W1 5d; WS5.C W1 4d (opt); WS5.E W1 4d; WS5.F W1 2d; WS5.G W1 3d; WS13.A W2 2d; WS13.B W2 3d; WS13.D W2 1d; WS13.E W2 5d; WS13.F W2 7d; WS13.I W2 2d; WS16.A W2 1d; WS16.D W2 5d; WS16.E W2 5d; WS16.F W2 5d; WS16.G W2 8d; WS5.I W1 13.5d (unsched); WS16.J W2 9d (unsched) |
| L6 | SPA-GATEWAY (Verevon v3 SPA and gateway browser/knowledge domains) | WS2.E W1 0.5d; WS9.A W2 5d; WS9.F W2 4d; WS9.G W2 5d; WS9.C W2 5d; WS9.B W2 4d; WS9.D W2 4d; WS9.I W2 3d; WS9.H W2 4d; WS3.J W1 6d; WS8.D W2 4d; WS8.I W2 7d; WS9.J W2 9d; WS13.H W2 4d; WS16.H W2 2d; WS13.J W2 2d (opt); WS9.K W2 -d (unsched); WS9.L W2 -d (unsched) |
| L7 | CRAWL-JOBS (Go orchestrator, quarry-control, crawl/batch/schedule edge routes) | WS6.A W1 1.5d; WS15.A W2 1d; WS14.A W2 1.5d; WS14.B W2 3d; WS14.C W2 4d; WS15.B W2 4.5d; WS15.C W2 4.5d; WS14.D W2 6d; WS15.D W2 7d; WS14.E W2 7d; WS14.F W2 2d; WS14.G W2 6d; WS15.E W2 6.5d; WS15.F W2 4d; WS15.G W2 4d; WS14.I W2 1d; WS15.I W2 5d (gated); WS15.H W2 4d (gated); WS15.J W2 1.5d; WS6.H W2 5d; WS6.K W2 4d; WS14.H W2 6d (gated); WS15.K W2 3.5d (opt) |
| L8 | PLANNER-MP (Model Plane Rust: execution-core, session-core, model-gateway non-grant) | WS11.A W2 2d; WS11.B W2 4d; WS11.C W2 3d; WS11.D W2 5d; WS11.G W2 3d; WS11.H W2 3.5d; WS11.E W2 7d; WS11.F W2 2d; WS12.E W2 3.5d; WS16.B W2 5d; WS16.I W2 7d; WS9.E W2 4d; WS8.G W2 7d; WS7.L W2 3d; WS10.F W2 10d |
| L9 | CONTROL-PLANE (org-core, auth-core, registry; billing-core) | WS7.H W2 6.5d (gated); WS7.I W2 7.5d (gated); WS16.C W2 3d (gated); WS17.H W2 8d (gated) |
| L10 | SHARED-BROWSER-QUARRY (attach, human sessions, execution lanes) | WS8.A W2 1.5d; WS8.B W2 6d; WS8.F W2 12d; WS8.H W2 6d; WS8.J W2 5d; WS8.K W2 5d (opt) |
| L11 | PROFILES (profile stores, envelope encryption, capture/restore) | WS17.B W2 3d; WS17.A W2 1.5d; WS17.C W2 4d; WS17.D W2 8d; WS17.E W2 7d; WS17.F W2 4d; WS17.G W2 5d; WS17.I W2 5d; WS17.J W2 3d (opt) |
| L12 | TRACE (request id, run trace envelope, telemetry) | WS10.A W2 3d; WS10.B W2 4d; WS10.C W2 5d; WS10.D W2 4.5d; WS10.E W2 4.5d; WS10.H W2 8d; WS10.G W2 4.5d (gated); WS10.I W2 4d |
| L13 | WEB-POLICY-QUARRY (ResolvedPolicy library and gates) | WS7.A W2 0.5d (gated); WS7.B W2 3d; WS7.C W2 4d; WS7.F W2 2d; WS7.E W2 3.5d; WS7.G W2 4.5d; WS7.J W2 3d (gated) |
| L14 | REDACTION (secret redaction, Quarry side) | WS12.A W2 3.5d; WS12.G W2 1.5d; WS12.C W2 4d; WS12.D W2 4d; WS12.H W2 2d (opt) |
| L15 | CAPACITY-METERING (limits, metering, metrics, drain) | WS6.F W2 1.5d; WS6.I W2 2d; WS6.C W2 6d; WS6.D W2 5d; WS6.J W2 3d |

Reading the lane table: L1 starts with the broker stream of WS1.A on day 0 while its Quarry validator stream waits for WS2.B; L2 owns the `agent_routes.rs` split and everything that follows it; L3 serialises `chromiumoxide.rs` (WS3.C, WS3.D, WS3.F first, W2 driver work in readiness order, WS3.G last); L4 splits into a gateway branch (WS4.A, WS4.B, WS4.C) and a Quarry branch (WS4.D to WS4.J); L5 is the whole scrape path (WS5 then WS13 then WS16 Quarry halves); L7 splits naturally into a crawl branch (WS6.A, WS14) and a jobs branch (WS15, WS6.H, WS6.K) and needs two engineers; L8 holds every Model Plane Rust change that is not a grant (planner, redaction scrub, attach mode, session-core trace persistence), strictly sequential in `browser_agent.rs`.

### 4.2 Earliest parallel start per lane (hard dependencies only, in engineer-days from plan approval; 5 days = 1 week)

Computed over the hard-dependency graph of section 1 plus the process edges listed under the table (unlimited parallelism between lanes, one engineer per slice chain). "Day 0" lanes can start the day the plan is approved.

| Lane | Starts | Slices that cannot start on day 0 (earliest start day) | External gate that can hold the lane |
|---|---|---|---|
| L1 | day 0 | WS1.B (day 10, week 2.0); WS1.C (day 10, week 2.0); WS6.E (day 20, week 4.0); WS7.K (day 20, week 4.0); WS8.C (day 20, week 4.0) | Control Plane sign-off for the quarry-edge to browser-broker:read principal and an auth-core rebuild (WS1.A); ADR topic 3 (WS8.C); ADR topic 1 (WS7.K, WS7.M) |
| L2 | day 0 | WS2.B (day 1.5, week 0.3); WS2.C (day 4, week 0.8); WS2.D (day 4, week 0.8); WS6.B (day 9.5, week 1.9); WS7.D (day 9.5, week 1.9); WS6.G (day 10.5, week 2.1) | none for WS2; the Quarry prod-boot spike gates WS1.A's Quarry stream, not this lane |
| L3 | day 0 | WS3.D (day 3, week 0.6); WS12.B (day 3.5, week 0.7); WS3.H (day 4, week 0.8); WS3.F (day 6, week 1.2); WS8.E (day 6, week 1.2); WS3.I (day 9, week 1.8); and 3 more | ADR 5 (WS3.I); ADR 10 for any isolation claim (WS3.G) |
| L4 | day 0 | WS4.E (day 1.5, week 0.3); WS4.C (day 3, week 0.6); WS4.F (day 4.5, week 0.9); WS4.H (day 6, week 1.2); WS4.I (day 6, week 1.2); WS4.J (day 6, week 1.2) | ADR topic 4 (WS4.H, WS4.J); Model Plane Z-05a/b commit (WS4.I); legal review and QB-T17 commit (WS4.J) |
| L5 | day 0 | WS5.B (day 2, week 0.4); WS16.D (day 5, week 1.0); WS5.D (day 7, week 1.4); WS16.E (day 10, week 2.0); WS13.E (day 12, week 2.4); WS5.E (day 12, week 2.4); and 6 more | DR-15.1 ruling (WS16.C feeds WS16.G); ADR topic 6 only for opening model-facing scrape actions; WS14/F-t2-040 for WS5.I |
| L6 | day 0 | WS9.D (day 5, week 1.0); WS9.H (day 5, week 1.0); WS3.J (day 9, week 1.8); WS9.I (day 9.5, week 1.9); WS13.H (day 17, week 3.4); WS16.H (day 28, week 5.6); and 2 more | ADR topic 3 (WS9.K, WS9.L); Model Plane half of WS9.J (sibling b and WS8.G) |
| L7 | day 0 | WS14.D (day 4, week 0.8); WS15.D (day 4.5, week 0.9); WS15.E (day 4.5, week 0.9); WS15.F (day 4.5, week 0.9); WS6.H (day 4.5, week 0.9); WS14.H (day 7, week 1.4); and 8 more | ADR topic 1 override owner (WS14.H); decisions D9, D14, D15, D17 of design D14b (WS15.K, WS15.H, WS15.I); Control Plane and Application Plane approvals for WS15.I |
| L8 | day 0 | WS11.B (day 2, week 0.4); WS11.C (day 2, week 0.4); WS11.H (day 2, week 0.4); WS12.E (day 3.5, week 0.7); WS11.F (day 5, week 1.0); WS9.E (day 5, week 1.0); and 5 more | owner decision OD13 and ADR topic 1 (WS11.F stage 3); session-core 0044/0045 commit (WS10.F, WS11.H) |
| L9 | day 0 | WS16.C (day 1, week 0.2); WS7.I (day 6.5, week 1.3); WS17.H (day 12, week 2.4) | ADR topic 1 (WS7.H, WS7.I); key-management ownership and master-key custody (WS17.H) |
| L10 | day 0 | WS8.B (day 1.5, week 0.3); WS8.F (day 20, week 4.0); WS8.H (day 32, week 6.4); WS8.J (day 45, week 9.0) | ADR topic 3 ratification (WS8.A to WS8.C) |
| L11 | day 0 | WS17.A (day 2, week 0.4); WS17.D (day 4, week 0.8); WS17.F (day 4, week 0.8); WS17.E (day 6, week 1.2); WS17.I (day 12, week 2.4); WS17.G (day 13, week 2.6) | external review of the crypto module before `REQUIRED=1` (WS17.D); Control owner review (WS17.H) |
| L12 | day 0 | WS10.D (day 5, week 1.0); WS10.H (day 7, week 1.4); WS10.E (day 9.5, week 1.9); WS10.G (day 14, week 2.8); WS10.I (day 24, week 4.8) | ADR 8 part 2 (WS10.G); security review of the new NATS principal (WS10.E); session-core 0044/0045 (WS10.F) |
| L13 | day 0 | WS7.C (day 3, week 0.6); WS7.F (day 3, week 0.6); WS7.E (day 7, week 1.4); WS7.G (day 7, week 1.4); WS7.J (day 14, week 2.8) | ADR topic 1 (WS7.A, WS7.J) |
| L14 | day 0 | WS12.G (day 3.5, week 0.7); WS12.C (day 9, week 1.8); WS12.D (day 13, week 2.6) | product decision on receipt text policy (WS12.D) and on legacy rewrite (WS12.H) |
| L15 | day 0 | WS6.C (day 10.5, week 2.1); WS6.J (day 16.5, week 3.3) | Lago usage codes (WS6.C `on`); Control credential and fail-mode ADR topic 1 (WS6.D) |

Process edges added to the dependency graph (they are merge-order, not data dependencies): WS3.D before WS3.F and WS3.F before WS3.K (the `chromiumoxide.rs` queue); WS3.D before WS8.E and WS17.E; WS2.C before WS6.B, WS7.D, WS8.F, WS9.I; WS3.H before WS12.C; WS11.A before WS11.B.

### 4.3 Merge order (whole plan)

Each numbered position is a merge train: the PRs inside a position may merge in any order unless the text says otherwise (they are file-disjoint or append-only), and a position starts as soon as its stated prerequisites are merged, not when the previous position finishes. Cross-lane rules for hot files are in section 3. W1 is positions 1 to 11; W2 is positions 12 to 27; the W1 exit gate is stated after position 11.

| Pos | Wave | Train | Slices | Reason for the position |
|---|---|---|---|---|
| 1 | W1 | Hotfix and zero-dependency set (any order, file-disjoint) | WS4.A, WS6.A, WS2.A, WS3.A, WS5.A, WS3.E, WS1.A | Risk first and no inbound dependency. WS4.A (gateway only), WS6.A (orchestrator, edge limits, one gateway function), WS2.A (smallest edit in the hottest file), WS3.A (compose lines 109-110 and a 2-line guard), WS5.A (transform and runtime), WS3.E (deletes `stealth.rs`, must precede WS3.F which edits `lib.rs` 164), WS1.A broker stream (Go, proto, registry row, compose, CI, `buf.yml` fix; its Quarry validator stream waits for item 3). WS4.B forensics (steps 0-2) run before WS4.A deploys. |
| 2 | W1 | Module split of `agent_routes.rs` | WS2.B | Needs WS2.A merged. Announce a 2-working-day freeze; every PR touching `agent_routes.rs` (WS1.A Quarry stream, WS3.H, WS4 marks) waits. It is the head of the critical path into WS2.C, WS6.B, WS7.D, WS8.F. |
| 3 | W1 | Run control, tests and leak closure (parallel-safe after the split) | WS2.C, WS2.D, WS2.E, WS3.B, WS4.B, WS4.D, WS5.B | WS2.C and WS2.D start from a 0.5-day shared skeleton PR (`RunLive`, `RunHandle`, `lookup_run`) and agree the `RunEntry` field list; WS3.B gives the live regression net before WS3.C/D; WS4.D closes the log and meter leaks in `routes.rs` 488-509 and `pipeline.rs` 821-829 before WS4.E and WS6.C; WS5.B follows WS5.A; WS4.B purge steps run once WS4.A is on every replica. |
| 4 | W1 | ZDR carrier and request-layer chain | WS4.C, WS4.E | WS4.E edits the `routes.rs` layer chain 286-303 and `telemetry.rs` 25-56 and gateway `main.rs`; it merges before WS5.D (scrape handler in the same file) and before WS10.A (request-id layer at 303). |
| 5 | W1 | Per-page emulation | WS3.C | First of the serial `chromiumoxide.rs` queue; precondition for WS3.D, WS3.F, WS8.E, WS17.G. |
| 6 | W1 | Deadline table (one short window) | WS3.D | Touches about 20 call sites in `chromiumoxide.rs`; announce a one-day window so WS12.B, WS13.C, WS17.E, WS8.E rebase once. |
| 7 | W1 | ScrapeOptions contract | WS5.D | Rewrites the scrape handler and `ScrapeRequest`; WS7.C, WS13.E/F, WS16.G all land after it (D13: D5 first). |
| 8 | W1 | Derive API and delegated browser bearer | WS1.B | Extends WS1.A's `Repository`; its Quarry hunks (projection `grantId`, owner check, override) rebase onto the split. The longest chain element: everything under shared browser, budgets-in-grant and policy-in-grant waits for it. |
| 9 | W1 | Driver cap, log lint, clean_html, scrape record (parallel-safe) | WS3.F, WS4.F, WS5.E, WS5.F | WS3.F needs WS3.C/D; WS4.F needs WS4.D/E; WS5.E and WS5.F need WS5.D. File-disjoint apart from `routes.rs` route table (one-line additions) and `/ready` 312-326 (WS3.F only in W1). |
| 10 | W1 | Postgres grants, challenge taxonomy, matrix, consumers (parallel-safe) | WS1.C, WS3.H, WS4.G, WS5.G, WS5.H, WS5.C, WS2.F | WS1.C needs WS1.B semantics; WS3.H needs WS2.B (one `agent_routes` field) and WS3.D; WS4.G adds new files and golden rows (append-only); WS5.G needs WS5.D deployed; WS5.H schema half after WS5.D (generated schemas must precede any hand edit to them); WS5.C and WS2.F optional. |
| 11 | W1 | Cross-plane plumbing and gated W1 tail | WS3.J, WS3.I, WS3.G, WS4.H, WS4.I, WS4.J | WS3.J needs WS3.H and sibling planes; WS3.I waits for ADR 5; WS3.G (pool) is optional and waits for ADR 10 for isolation claims; WS4.H needs ADR topic 4 (observe first); WS4.I needs the Model Plane Z-05a/b commit; WS4.J needs ADR 4, legal review and the QB-T17 commit. W1 exit gate follows (section 4.4). |
| 12 | W2 | Request-id codemod window | WS10.A | A mechanical change across handlers (`RequestKind::new()` at 7 sites in `profile_routes.rs` alone) that would conflict with every open W2 route PR: merge first, announce a one-day freeze, every W2 PR rebases. |
| 13 | W2 | Pure and new-file foundation set (any order) | WS7.A, WS7.B, WS11.A, WS12.A, WS12.G, WS14.A, WS14.B, WS15.A, WS15.B, WS16.A, WS8.A, WS13.A, WS13.D, WS6.F, WS17.B, WS17.C, WS17.A, WS10.B, WS11.G | No inbound hard dependency beyond WS10.A (for WS17.B, per D16) and WS3.B (WS17.A). Libraries, docs, fixtures, schema-only and mechanical PRs. WS15.B (`jobCols` refactor) goes first and alone among the control-store edits; WS10.B repairs the buf gate and allocates proto numbers 14-18 and 150-156. |
| 14 | W2 | Quarry-edge scrape, policy and crawl-filter wiring | WS7.C, WS7.F, WS13.B, WS14.C, WS7.E | WS7.C replaces `RunPolicy::default()` in the scrape family after WS5.D; WS7.E (search, extract, profile routes) follows it and edits `search_routes.rs` last, after the owner's uncommitted rerank diff is committed; WS14.C is inert until WS14.D; WS13.B adds action/capture types after WS5.D. |
| 15 | W2 | Admission queue (post-split `agent_routes/*`) | WS6.B, WS7.D, WS12.C, WS12.D, WS10.C, WS10.D, WS8.B | Gate order is a contract: policy (WS7.D) and budget (WS6.B) before sensitive approval. WS6.B first because planner stage 2 needs enforced budgets; WS12.C/D need WS12.A and WS12.B; WS10.C/D need WS10.B; WS8.B adds human step semantics. Migration numbers follow section 3.1. |
| 16 | W2 | Driver queue after WS3.F, in readiness order | WS8.E, WS12.B, WS13.C, WS17.E, WS17.G, WS3.K, WS13.G, WS12.F, WS3.L | Each rebases over the previous one; WS3.K (actionability) lands after its prerequisites and WS13.G/WS12.F extend capture code last. The live lane (WS3.B) must be green. WS3.L (rich input) is unscheduled and listed only to hold its place. |
| 17 | W2 | Capacity and metering | WS6.C, WS6.D, WS6.I, WS6.J, WS6.G | WS6.C needs WS2.D; WS6.D the Control credential for `billing_core` mode (static mode first); WS6.I conflicts with WS4.E layers (already merged); WS6.J needs WS6.C (flush) and WS2.D (`finish_run`); WS6.G after WS6.D for plan values. |
| 18 | W2 | Crawl scope and job lifecycle queue | WS14.D, WS15.C, WS15.D, WS14.E, WS15.E, WS14.F, WS14.G, WS15.F, WS15.G, WS14.I, WS15.I, WS15.H, WS15.J, WS6.H, WS6.K, WS14.H, WS15.K | D14b's order (D6 S0 and D14a first, then S0, L1, S1, L2, S2, L3, L5, S3, S4, S5) with WS14.D's `crawl-scope-v1` ahead of WS15.D's `lifecycle-v1`. WS15.J (schedules go live) needs WS14 deployed and WS6.A live. WS6.H and WS6.K follow WS15.B because all three edit the `jobs` store lists; WS14.H waits for ADR topic 1 and a Control credential. |
| 19 | W2 | Planner queue (Model Plane Rust) | WS11.B, WS11.C, WS11.D, WS11.H, WS11.E, WS11.F, WS12.E, WS16.B, WS16.I, WS9.E, WS8.G, WS7.L, WS10.F | WS11.B introduces the single run-parameter mint helper in `browser_run.rs` and WS11.D restructures the loop; WS3.J, WS8.G, WS12.E rebase after them (D11); WS10.F needs WS10.E and session-core 0044/0045; WS11.H needs the same commits. Stage 3 of WS11.F additionally needs WS1, WS6.B/E, WS12.B-E, F-t2-044 Tier B and OD13. |
| 20 | W2 | Broker, proto and registry queue (Model Plane Go) | WS6.E, WS8.C, WS7.K, WS7.M | After WS1.C. Field and migration numbers are pre-allocated (section 3.1) so merge order is by readiness: WS6.E needs WS6.B; WS8.C needs ADR topic 3; WS7.K needs ADR topic 1 and WS7.I. |
| 21 | W2 | Control Plane queue | WS7.H, WS7.I, WS7.J, WS16.C, WS17.H | org-core migrations 021-023 (WS7.H) before 024 (WS17.H); one registry PR per wave regenerates the golden in three trees (WS7.I, WS16.C); WS7.J needs the golden. All gated by ADR topic 1, DR-15.1 or the key-management ownership decision. |
| 22 | W2 | Trace persistence and telemetry | WS10.E, WS10.H, WS10.I, WS10.G | WS10.E after WS10.D; WS10.H after WS4.D/E/F; WS10.I closes; WS10.G waits for ADR 8 part 2. |
| 23 | W2 | Attach core and execution lanes | WS8.F, WS8.H, WS8.J, WS8.K | After WS1.B, WS2.C, WS8.E and the admission queue items it shares (`step.rs`, `step_receipts.rs`); WS8.H is the GA gate for `QUARRY_EDGE__ATTACH`; WS8.J needs WS8.I and WS2.D (orphan sweep). |
| 24 | W2 | Scrape formats, actions and extraction queue | WS13.E, WS13.F, WS13.I, WS16.D, WS16.E, WS16.F, WS16.G, WS5.I, WS16.J | WS13.E/F after WS13.B/C/D and WS5.D; WS16.D-G in order (3 before 4 before 5 before 6) with DR-15.1 and the grant (WS16.C) before WS16.G's E2 tier; WS5.I and WS16.J are unscheduled follow-ons. |
| 25 | W2 | Profile stores and erasure | WS17.D, WS17.F, WS17.I, WS17.J | E1 after P1 (migration 0015), P2 after P1 and WS2.B, E3 after E1 (and E2 for shredding); S7 on demand. Profile route edits follow D16's order: WS10.A, WS17.B, WS4.H, WS7.E, WS17.F. |
| 26 | W2 | SPA and gateway queue | WS9.A, WS9.F, WS9.G, WS9.C, WS9.B, WS9.D, WS9.I, WS9.H, WS8.D, WS8.I, WS9.J, WS13.H, WS16.H, WS13.J, WS9.K, WS9.L | `BrowserChrome.tsx` takes small wiring hunks in this order; WS9.A's production flag waits for WS4.A; WS9.I and WS8.I wait for WS2.C deployed; WS8.D needs WS8.C; WS9.H GA needs WS8; WS9.K/L unscheduled behind ADR topic 3. Slices that touch only new modules (WS9.A, F, G, C) may merge at any point earlier. |
| 27 | W2 | Redaction closeout | WS12.H, WS7.G | Optional legacy-row scrub (needs a product decision) and the policy backstops (WS7.G 6b waits for WS14/WS15 deployed). |

**W1 exit gate (after position 11).** All of the following merged and deployed on the rebuilt local stack: WS1.A, WS1.B, WS1.C with `BROWSER_GRANT_ENFORCEMENT=strict` on the production-overlay stack; WS2.A to WS2.E; WS3.A to WS3.F, WS3.H, WS3.J (WS3.I, WS3.G optional or gated); WS4.A to WS4.G with the purge recorded; WS5.A, WS5.B, WS5.D to WS5.H; the PRE-1 commit of the in-flight work done. Gated or optional items (WS4.H, WS4.I, WS4.J, WS3.I, WS3.G, WS5.C, WS2.F) may trail without blocking W2 starts, except that WS4.A must be deployed before `VITE_BROWSER_OMNIBOX_SEARCH` is on in production.

**Merge-order resolutions where the designs disagree.**

- `chromiumoxide.rs`: D3 puts the F-t2-020/021/023/034 rebases between S3 and S5; D16 puts them after S10 and S6. Resolved by wave: WS3.C, WS3.D, WS3.F (W1) merge before every W2 driver slice; the W2 slices merge in readiness order; WS3.K then WS3.G last.
- `agent_routes.rs`: D1 says merge WS1 first (the broker conflict surface), D2 demands a freeze for the split. WS1's broker, proto, model-gateway and registry work does not touch the file and proceeds from day 0; only WS1.A's Quarry stream (hunks at 288, 314-338, 548-650, 1372-1413) waits for WS2.B.
- Migration numbering (D6 and D8 both name 0011; control 016 and 017 are claimed by D6 and D14b): section 3.1.
- WS6.A versus D14a's page caps: section 7.2 item 1.
- WS10 is a W2 workstream but its first slice (WS10.A) is placed at the head of W2 on purpose: it is a codemod that every other open route PR would otherwise have to rebase over.

### 4.4 Critical path

Longest chain of hard dependencies through scheduled, non-optional, non-unscheduled slices (durations are the designs' engineer-days; a gated slice is included when it lies on the path):

`WS1.A (10 d) -> WS1.B (10 d) -> WS8.F (12 d) -> WS8.H (6 d) -> WS8.I (7 d) -> WS8.J (5 d)`

| Step | Start (eng-day) | Finish (eng-day) |
|---|---|---|
| WS1.A prod-mode contract harness; broker issues kind/zdr/TTL/cap-bound grant | 0 | 10 |
| WS1.B DeriveRunGrant API, model-gateway POST/DELETE /v1/browser/grants, gate | 10 | 20 |
| WS8.F Quarry attach core: tab ledger, caller resolution, per-tab control, at | 20 | 32 |
| WS8.H execution lanes: human and agent actions run concurrently (GA gate for | 32 | 38 |
| WS8.I gateway and SPA: start with attach, agent tab, Reveal, Take over, Give | 38 | 45 |
| WS8.J phase 3: deliverable and handoff marks, turn-scoped release, orphan sw | 45 | 50 |

**Critical path length: 50 engineer-days = 10 weeks of one engineer-chain** (D1 itself says 16-18 working days with two engineers on WS1.A/B, so the path is 46-48 days with that split). Risk-adjusted (x1.3 to x1.8 for cross-plane review, in-flight rebases and ADR waits): 13 to 18 weeks. It is the shared browser: broker grants, then the attach core (WS8.F), then execution lanes (WS8.H), then the gateway/SPA attach UI (WS8.I), then phase 3 (WS8.J). Everything else is shorter.

Other long chains (all start on day 0 in parallel with the critical path):

| Chain | Length (eng-days) | Path |
|---|---|---|
| to WS16.H | 30 | WS16.B -> WS16.D -> WS16.E -> WS16.F -> WS16.G -> WS16.H |
| to WS10.I | 28 | WS10.C -> WS10.D -> WS10.E -> WS10.F -> WS10.I |
| to WS7.K | 26.5 | WS1.A -> WS1.B -> WS7.K |
| to WS13.I | 26 | WS5.A -> WS5.B -> WS5.D -> WS13.E -> WS13.F -> WS13.I |
| to WS15.J | 17.5 | WS15.B -> WS15.E -> WS15.I -> WS15.J |
| to WS14.I | 18 | WS14.C -> WS14.D -> WS14.E -> WS14.I |
| to WS17.I | 17 | WS17.C -> WS17.D -> WS17.I |
| to WS6.H | 9.5 | WS15.B -> WS6.H |

Two chains whose last step is a decision or an epic rather than a slice, so the graph under-states them: (1) planner default-on (WS11.F stage 3) needs WS1.A, WS1.B, WS6.B and WS6.E (engineer-day 25 on the graph, about 29 with the sibling Model Plane and execution-core days, 31 with WS11.F), WS12.A to WS12.E (15.5 days, off the path), F-t2-044 Tier B (about 12 days beyond Tier A, epic W3, can run beside WS6.E) and owner decision OD13; plan on engineer-day 31 at best and 37 to 43 realistically, plus the live-success floor. Stages 1 and 2 land earlier (stage 1 after WS11.A, B, C; stage 2 after WS1, an enforcing WS6.B and the eval corpus). (2) Scheduled crawls (WS15.J) are 17.5 days on the graph (WS15.B, WS15.E, WS15.I, WS15.J with WS14.D and WS6.A as prerequisites) plus the one-week observation soak of WS15.I before go-live: about 22.5 days; WS14.I (scope to enforce) needs its own soak.

<!-- part end -->
