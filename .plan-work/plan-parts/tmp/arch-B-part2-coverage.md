## 2. COVERAGE MATRIX (77 audit items, F-t2-001..F-t2-077)

Source of the item list and audit waves: `audit-sections/appendix-b-index.md` (77 rows, W1 10, W2 18, W3 21, W4 16, W5 12). "Handled by" names a catalogue slice (section 1), or `epic (W3)` / `epic (W4)` / `epic (W5)` for items another agent is writing as epics, or `parked`. A phase or part that the design describes but does not schedule is marked `UNSCHED` and is repeated in the gap list below the table.

Totals: 29 items have file-level design coverage (F-t2-001..023, 025, 026, 027, 029, 045, plus 046 phase A); 48 items are epic-only (20 W3, 16 W4, 12 W5 of which 1 is parked). Every one of the 77 items has an audit wave; no item is without an owner. The residual gaps are partial phases, listed after the table.

| Item | Audit wave | Handled by | Notes |
|---|---|---|---|
| F-t2-001 | W1 | WS1.A, WS1.B, WS1.C | Audit L stands (about 26 engineer-days). Gated by a new ledger entry plus Control sign-off (design D1 section 5), not by an ADR topic. Hard-blocks F-t2-010/011/012/013 ph2/024/032/033/039/063. |
| F-t2-002 | W1 | WS2.A | 1.5 d. No broker or proto change (the audit's script-hash scope option is not taken). |
| F-t2-003 | W1 | WS4.A, WS4.B, WS4.C (phase 1, the hotfix); WS4.G (matrix v1); WS4.H, WS4.I, WS4.J (phase 2, gated) | WS4.H and WS4.J wait for ADR topic 4; WS4.I waits for the Model Plane Z-05a/Z-05b commit. Stock compose has no gateway cache, so exposure is deployment-conditional (design D4). |
| F-t2-004 | W1 | WS4.D, WS4.E, WS4.F (phase 1); WS10.H (phase 2: sampler, late-ZDR registry, span processor, exporter flag) | Phase 2 hard-depends on phase 1 (audit dependency note 8). No span may be added before WS10.H merges. |
| F-t2-005 | W1 | WS5.D, WS5.E, WS5.F, WS5.G, WS5.H | Crawl and batch embedding of `ScrapeOptions` is WS5.I, `UNSCHED` (D5 Epic E1, 12-15 d). WS5.H is also the do-now OpenAPI slice of F-t2-064. |
| F-t2-006 | W1 | WS2.D (registry, caps, sweeper), WS2.E (fleet console hidden) | A real fleet console waits for a fleet producer (not scheduled). Run caps are infrastructure limits; plan-aware caps are WS6.G. |
| F-t2-007 | W1 | WS3.C (part A: per-page emulation), WS3.F (part B, K=1), WS3.G (K>1 pool, `OPT`) | WS3.A (sandbox default) and WS3.B (live lane) are the same design's enabling slices. |
| F-t2-008 | W2 | WS6.B (slice 1), WS6.D (2a), WS6.E (2b); WS6.A is the immediate crawl bound | Model Plane and execution-core sibling days (about 5.5 d) are outside the Quarry figures. Hard-blocks F-t2-010 stage 2/3. |
| F-t2-009 | W1 | WS5.A, WS5.B, WS5.C (`OPT`, the production gate for `DOC_ROUTING=on`) | |
| F-t2-010 | W2 | WS11.A to WS11.F (planner, budgets, staged flag, recovery, widened actions, rollout), WS11.G (connector-first) | Stage 3 (default-on) needs WS1, WS6.B/WS6.E, WS12.B-WS12.E, F-t2-044 Tier B (epic W3) and owner decision OD13. Audit effort M is L (about 26 d). |
| F-t2-011 | W2 | Backend: WS8.A, WS8.B, WS8.C, WS8.D. Frontend: WS9.A (omnibox), WS9.B (reload) | WS8.C waits on ADR topic 3. `VITE_BROWSER_OMNIBOX_SEARCH` stays off in production until WS4.A is deployed. |
| F-t2-012 | W2 | WS9.F, WS9.G, WS9.H (dock), WS9.J (bind to a live session) | WS9.K (attach UI) is `UNSCHED`. GA of the interactive dock needs WS8. |
| F-t2-013 | W2 (phase 1 in W1) | Phase 1: WS2.C. Phase 2: WS8.E, WS8.F, WS8.G, WS8.H, WS8.I. Phase 3: WS8.J. UI: WS9.I (step c1) | WS9.L (agent-flagged tabs UI) `UNSCHED`; WS8.K (acting-user claim) `OPT`. Phase 2 about 40 d (XL); the item is L only for phase 1. |
| F-t2-014 | W2 | UI first slice: WS9.C, WS9.D, WS9.E. Canonical trace: WS10.A to WS10.G, WS10.I | WS10.G (Application projection) gated by ADR 8 part 2. Audit L is XL by repo scale (about 47.5 d). |
| F-t2-015 | W2 | WS3.K | Placed in WS3 by the fixed numbering; it is a W2-timed slice (needs WS3.C, WS3.D, WS3.F). Rich input follow-ups are WS3.L, `UNSCHED`. |
| F-t2-016 | W2 | WS14.A to WS14.G, WS14.I; WS14.H (ignore-robots entitlement, gated) | Must precede F-t2-018 go-live (WS15.J). Audit "step 1 is S" is about 10 d in the design. |
| F-t2-017 | W2 | WS15.A, WS15.B, WS15.D, WS15.F, WS15.G; WS15.K (`OPT`, results source decision) | |
| F-t2-018 | W2 | WS15.C, WS15.E, WS15.H, WS15.I, WS15.J | `QUARRY_SCHEDULED_JOBS_ENABLED` stays 0 until WS14 scope is deployed and WS6.A is live. WS15.C also closes a live cross-tenant schedule exposure and should ship early. |
| F-t2-019 | W2 | WS16.A to WS16.H (grant, model lane, engine, API), WS16.I (interim Model-proposed route) | WS16.J (crawl/batch embedding) `UNSCHED`. Gated by DR-15.1 (WS16.C). Audit M is L (about 41 d). |
| F-t2-020 | W2 | WS17.A to WS17.J | Track A (stores, envelope) and Track B (capture, restore) run in parallel; WS17.H waits for the key-management ownership decision. |
| F-t2-021 | W2 | WS12.A to WS12.G; WS12.H (`OPT`) | Secrets in scraped page text wait for F-t2-050 (epic W3). Hard-blocks F-t2-024. |
| F-t2-022 | W2 | WS7.A to WS7.M | Owner-neutral part (WS7.B to WS7.G, 21.5 d) needs no decision; WS7.A, WS7.H to WS7.K, WS7.M need ADR topic 1. |
| F-t2-023 | W2 | WS13.A to WS13.I; WS13.J (`OPT`) | Model-facing scrape actions stay closed until ADR topic 6. |
| F-t2-024 | W3 | epic (W3) | Outline only in design D12 (fill_otp first, secrets later). Hard-blocked on WS12.A to WS12.F, WS1, WS8.F (holder semantics), WS3.K (trusted input); custody ADR (topic 2) for the secrets phase. |
| F-t2-025 | W2 | WS6.F, WS6.G, WS6.H | Design-level slices L1 to L7 in D6 (about 26 d, includes the multi-Chromium pool) are `UNSCHED`. |
| F-t2-026 | W1 | WS3.D | |
| F-t2-027 | W1 | WS3.E (deletion), WS3.H (taxonomy), WS3.I (live triggers, ADR 5), WS3.J (plumbing to Verevon) | |
| F-t2-028 | W3 | epic (W3) | Consumes WS9.E (untrusted-content classification fix) and WS3.K (hit-testing needs a fixed viewport). |
| F-t2-029 | W2 | WS6.I (recorder, `/metrics`), WS6.J (drain), WS6.K (per-run statistics) = phase 1 | Phases 2 and 3 (span vocabulary on D10's contract, memory-aware admission, alert rules) are design-level in D6 and `UNSCHED`; WS10.H provides the span safety contract they need. |
| F-t2-030 | W3 | epic (W3) | Judge needs WS16 (F-t2-019 lane). |
| F-t2-031 | W3 | epic (W3) | Shares `store.go`, `resources.go` and migrations with WS15; needs WS15.B. |
| F-t2-032 | W3 | epic (W3) | Outline in D7 (Epic E-032, XL). Reuses WS7 `ResolvedPolicy`; proto fields 21-39 are reserved for it (section 3). |
| F-t2-033 | W3 | epic (W3) | Needs F-t2-032 and WS1; Model Plane approval continuation overlaps sibling (b). |
| F-t2-034 | W3 | epic (W3) | Rebases after WS3.D; shares `observation.rs` and `contracts.rs` with WS3.H, WS3.K, WS12.C. |
| F-t2-035 | W3 | epic (W3) | Must reuse the WS8 session state machine (D8-10) and WS17 `SaveMeta`/owner guard. |
| F-t2-036 | W3 | epic (W3) | Builds on WS1.C Postgres rows; touches `step` receipts (merge after WS2.C, WS12.D). |
| F-t2-037 | W3 | epic (W3) | Extends WS5.B document router; shares the grant PR with WS16.C. |
| F-t2-038 | W3 | epic (W3) | Consumes WS9.I and WS9.J; `BrowserChrome.tsx` queue. |
| F-t2-039 | W3 | epic (W3) | Needs WS1 and the `browser.act.upload/download` capability keys of WS7.B. |
| F-t2-040 | W3 | epic (W3) | Uses WS15.D `controlGate` between waves; WS5.I for shared options. |
| F-t2-041 | W3 | epic (W3) | Seeds from WS14.C `crawl_filter`; `sitemap` and `ignore_query_parameters` request fields reserved by D14a. |
| F-t2-042 | W3 | epic (W3) | Needs WS5.D, WS6.B/WS6.E (`clamped`, 402), WS7. |
| F-t2-043 | W4 | epic (W4) | |
| F-t2-044 | W3 | epic (W3) | Tier A hermetic harness is inside WS11.A and WS11.D; Tier B (about 12 d beyond Tier A) is the stage-3 gate of F-t2-010. |
| F-t2-045 | W2 | WS11.H (skip-only slice) | Verify-first item; the audit's `session_tool_audit_intents` taint derivation is wrong for execution-core runs (D11). |
| F-t2-046 | W3 | WS6.C, WS6.D (phase A: browser time, run, crawl, extract, map metrics; plan keys) - pulled forward from W3; remainder epic (W3) | Phase B (browser metrics beyond phase A, price table, outcome attributes) epic (W3). |
| F-t2-047 | W3 | epic (W3) | `error.rs` coordination with WS3.D, WS5.A, WS6.B. |
| F-t2-048 | W3 | epic (W3) | Must consume WS4.A scope/key module and check `domains.deny` and ZDR before any read. |
| F-t2-049 | W3 | epic (W3) | Reuses the WS7.G `SecurityEngine` decorator pattern. |
| F-t2-050 | W3 | epic (W3) | Ordering contract recorded in D5: redaction sits between markdown computation and artifact/inline assembly and must cover `html` and `raw_html`; WS16 AI formats must read post-redaction text. |
| F-t2-051 | W4 | epic (W4) | |
| F-t2-052 | W4 | epic (W4) | |
| F-t2-053 | W4 | epic (W4) | |
| F-t2-054 | W4 | epic (W4) | Consumes the WS8 state machine (D8-10). |
| F-t2-055 | W4 | epic (W4) | Uses the WS14 evaluator and `map_routes.rs`. |
| F-t2-056 | W4 | epic (W4) | |
| F-t2-057 | W4 | epic (W4) | |
| F-t2-058 | W4 | epic (W4) | Consumes WS7 `proxy.max_tier` and WS17 profile pinning. |
| F-t2-059 | W4 | epic (W4) | Needs the capability flags of WS3.F/WS3.G/WS8.E and the ADR 10 egress proof. |
| F-t2-060 | W4 | epic (W4) | ADR 10. |
| F-t2-061 | W4 | epic (W4) | Reuses WS1.B run-grant derivation. |
| F-t2-062 | W4 | epic (W4) | Reuses WS6 `clamped`/`limits` and WS1.B. |
| F-t2-063 | W4 | epic (W4) | Hard-blocked on WS1. |
| F-t2-064 | W4 | epic (W4); early slice WS5.H (OpenAPI from types, route-coverage CI) | MCP exposure stays gated by ADR topic 7. |
| F-t2-065 | W4 | epic (W4) | |
| F-t2-066 | W5 | epic (W5) | |
| F-t2-067 | W5 | epic (W5) | Replaces the `ROBOTS_PRODUCT_TOKENS` constant of WS14.E. |
| F-t2-068 | W5 | epic (W5) | |
| F-t2-069 | W5 | epic (W5) | |
| F-t2-070 | W5 | epic (W5) | `BrowserChrome.tsx` queue. |
| F-t2-071 | W5 | epic (W5) | |
| F-t2-072 | W5 | epic (W5) | |
| F-t2-073 | W5 | epic (W5) | |
| F-t2-074 | W5 | epic (W5) | The two fail-open candidates found in S9 (H3, H4 below) are real in committed code; they should not wait for W5 (see PRE-2). |
| F-t2-075 | W5 | epic (W5) | |
| F-t2-076 | W5 | epic (W5) | |
| F-t2-077 | W5 | parked | Owner ruling 2026-09-28; reopens only with an owner decision, the F-t2-059 multi-backend abstraction and a redesigned fail-closed gate. |

### 2.1 Gaps: designed but unscheduled, partially covered, or owned by nobody

Unscheduled design-level work (not in any effort total): WS3.L rich input; WS5.I crawl/batch embedding (12-15 d); WS9.K attach UI and WS9.L agent-flagged tabs (L to XL); WS16.J crawl/batch AI formats (8-10 d); D6 design-level L1 to L7 (about 26 d: multi-Chromium pool, span vocabulary, memory-aware admission, alerts, fleet); F-t2-024 E1 to E4 (20-30 d, W3).

Partial phases that the audit lists under a wave but no scheduled slice finishes: F-t2-029 phases 2 and 3; F-t2-025 beyond WS6.F/G/H (the L-series); F-t2-046 phase B; F-t2-012 slice 6b; F-t2-013 phase B acting-user claim (WS8.K optional).

Prerequisites with no owner in any design (flagged for the integrating architect; section 8 explains each):
- **PRE-1** (hard): the live tree carries about 324 uncommitted entries that every design says to rebase on, plus 28 untracked golden/fixture files and 6 untracked gate files that the committed sync script and CI need (S9 H1). Until they are committed, the gates pass only on the owner's machine and every lane's `zdr-contract-sync.test.sh` row edit conflicts. Owner action, not a slice.
- **PRE-2** (small, high value): S9 H3 (dev-bypass parse mismatch: `apps/Ingestion Plane/Quarry-v2/crates/quarry-edge/src/main.rs:133` [M] accepts `1|true|TRUE|yes` case-sensitively while `.../src/auth.rs:258-261` [C] accepts any case of `true`) and H4 (artifact backend `S3`/`Fs` passes the durability gate and selects the in-memory store, `main.rs:369-373` [M]). These are fail-open candidates in committed code. They belong to F-t2-074 (W5, verify first); recommend a 1 d fix PR in W1 (section 6, candidate 4).
- **PRE-3**: `.github/workflows/buf.yml` runs `buf lint` with an unquoted path (splits at the space) and a 0-byte baseline (S9 H5). D1 and D10 each assume they repair it; assign it to WS1.A (first merger).
- **PRE-4**: the Model Plane Go services (browser-broker 7, capability-core 88, cost-core 19 test files) have no CI (S9 H8). WS1.A adds `browser-broker-ci.yml`; capability-core and cost-core stay uncovered.
- **PRE-5**: Quarry smoke scripts default to port 8080 (`smoke.sh`, `start-stack.sh`) while the edge publishes 38080 (standalone) or 8082 (Ingestion compose) (S9 H7); every wave's evidence recipe below states the real port.
- **PRE-6**: `apps/Control Plane/auth-core/package.json:21,23` [C] `test:e2e`/`test:smoke` point at the missing `apps/auth/test/...` path (S9 section 5.1); not needed by any slice but misleads anyone running the contract tests.
- **PRE-7**: `BROWSER_BROKER_GRPC` is named in `deploy/compose/README.md:95` [C], `docs/SELF_HOST.md:60` [M], `docs/RUNBOOKS.md:84` [M] but no code reads it; the real name is `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL`. WS1.A corrects the three docs (D1 section 4).

<!-- part end -->
