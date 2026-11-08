# Architect B: sequencing, parallelism and coverage for the Quarry v2 and shared-browser plan

Date: 2026-10-15. Role: architect B (sequencing, parallelism, coverage). Audit source (read-only): `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md`. Inputs: designs `.plan-work/designs/D1` to `D16` (sections 4, 5, 7, 8, 10 read; sections 2 and 3 read only where a slice or collision needed them), `.plan-work/audit-sections/appendix-b-index.md`, `09-roadmap-decisions.md`, and `.plan-work/sweeps/S9-ci-and-run-commands.md`. Nothing under `C:/dev/CoresSystem` was edited; the only writes are this file and its scratch parts.

## 0. Summary

- **Scope sequenced:** 17 workstreams (WS1 to WS17), 162 slices (157 scheduled, 5 design-level unscheduled). W1 (WS1 to WS5, plus the WS6.A hotfix, minus WS3.K) is 125.75 engineer-days of core and gated work plus 10 optional; W2 (WS6 to WS17 plus WS3.K) is 510 engineer-days plus 19.5 optional; 22.5 days of design-level work are not in any total (section 5).
- **Critical path:** 50 engineer-days, the shared browser: `WS1.A -> WS1.B -> WS8.F -> WS8.H -> WS8.I -> WS8.J` (46 to 48 with D1's two-engineer split; 13 to 18 weeks risk-adjusted). Everything else is at most 30 days.
- **Lanes and merge order:** 15 lanes (section 4.1), 27 numbered merge positions (section 4.3: W1 positions 1 to 11, W2 positions 12 to 27). The one serial resource that shapes everything is `agent_routes.rs`: WS2.A, then the WS2.B split behind a 2-working-day freeze, before any other edit to it.
- **First three PRs:** (1) gateway search-cache org scope and ZDR bypass (WS4.A), (2) crawl page/depth/failure bounds (WS6.A), (3) raw-script (`evaluate`) deny (WS2.A). File-disjoint, mergeable in parallel; section 6 has branches, titles, files, tests and evidence.
- **Coverage:** all 77 audit items have an owner or wave: 29 have file-level design coverage, 48 are epics (20 W3, 16 W4, 12 W5 of which F-t2-077 is parked). Gaps, partial phases and seven unowned prerequisites (PRE-1 to PRE-7) are in section 2.1; the two that matter most are PRE-1 (the live tree's uncommitted work, including 28 untracked golden fixtures and 6 untracked gate files that CI and the sync script need) and PRE-2 (two fail-open parse bugs in committed code that belong to a W5 item).
- **Collisions found between designs and resolved here:** crawl cap env names and ceiling (WS6.A versus D14a), quarry-runtime migration 0011 (D6 and D8), quarry-control migrations 016 and 017 (D6 and D14b), the `chromiumoxide.rs` queue order (D3 versus D16), `agent_routes.rs` ordering (D1 versus D2), `ZDR_REFUSE_DURABLE_RESOURCES` defined twice, the proto field ledger. Sections 3.1, 4.3 and 7.2.
- **Decisions the plan is waiting on:** section 4.5 (ADR topics 1, 3, 4, 5, 8 part 2, 10, DR-15.1, key-management ownership, OD13, four D14b decisions, the crawl hard ceiling). 12 days of W1 and 63 days of W2 are gated by them; the plan names a default for each so lanes can proceed up to the gate.

Method and limits. Slice goals, days, owners and dependencies come from each design's section 4 and section 10; hotspot files and line ranges from section 5 of each design; commands, CI and known failures from S9. Provenance tags `[C]`, `[M]`, `[U]`, `[NEW]` come from `tools/provenance.sh` run on 2026-10-15 for every file named in sections 3 and 6 (a path the plan would create is `[NEW]`; `[MISSING]` was only returned for paths that are `[NEW]` in a design, plus one wrong citation I corrected: `chat-run-watch.ts` lives at `apps/Frontend Plane/verevonv3/src/features/chat/lib/chat-run-watch.ts` [C]). Line ranges are quoted from the designs, which verified them against the live tree; this document did not re-open product code to re-verify them, so any `path:line` drifts as earlier slices land. Where two designs disagree, this document says so and records the resolution; no audit claim was found false in the parts read here beyond those the designs already record (`[CLAIM-FALSE]` items live in the designs: the `BROWSER_BROKER_GRPC` name, the stale `smoke.sh` port). Design-internal workstream labels differ in one place (D13 calls itself WS17); only the fixed WS1 to WS17 numbering is used.

## 1. SLICE CATALOGUE

Scale used in the Effort column: S = up to 2.5 engineer-days, M = more than 2.5 and up to 5.5, L = more than 5.5 (unless a design states otherwise; the days are the design's own figure from its section 4 or section 10, so a slice the design labels differently keeps the design's number). Flags in the Notes column: `OPT` optional, `GATED` blocked by a named decision, `UNSCHED` design-level only (not estimated for a sprint). Owner = plane and service. Hard deps list other slice ids; text after `soft:` is merge-order or evidence coupling. A letter is the slice id suffix: slice `WS3.D` is design D3 slice S3. One design numbers itself differently (D13 calls itself WS17 in its section 10; D14a calls itself WS14a); this plan uses only the fixed WS1..WS17 numbering.

### WS1 Grants and delegated bearer (design D1, wave W1, 26 d total, 26 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS1.A | 1a | prod-mode contract harness; broker issues kind/zdr/TTL/cap-bound grants on the in-memory Repository; Quarry-to-broker credential, registry row, compose network | L | 10 | Model Plane Go browser-broker + model-gateway; Control registry; Ingestion quarry-edge validator wiring | none | soft: Control sign-off for the quarry-edge to browser-broker:read principal; Quarry prod-boot spike; Quarry hunks rebase after WS2.B; two PR streams: broker/proto/registry/compose/CI (start day 0) and Quarry validator + contract harness (after WS2.B) |
| WS1.B | 1b | DeriveRunGrant API, model-gateway POST/DELETE /v1/browser/grants, gateway grant wiring and delegated browser bearer (BROWSER_GRANT_ENFORCEMENT) | L | 10 | Model Plane (broker, model-gateway) + Frontend gateway + quarry-edge projection/owner check | WS1.A |  |
| WS1.C | 1c | Postgres grant store behind a routing Repository (ZDR grants never persisted); migration 0001, entrypoint, restart test | L | 6 | Model Plane browser-broker | WS1.A | soft: Derive SQL merges after WS1.B |

### WS2 Quarry run control (design D2, wave W1, 17.5 d total, 16.5 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS2.A | S1 | hard-deny AgentAction::Evaluate on REST step, interact, WebSocket and the executor; typed 403 raw_script_denied | S | 1.5 | Ingestion quarry-edge, quarry-runtime | none |  |
| WS2.B | S2 | behaviour-preserving split of agent_routes.rs into agent_routes/*; announced 2-working-day freeze | S | 2.5 | Ingestion quarry-edge | WS2.A | soft: process: WS2.A merged and the freeze announced |
| WS2.C | S3 | control authority inside Quarry: initiator, control epoch, USER_TOOK_OVER, Skipped receipt, migration 0010 (F-t2-013 phase 1) | M | 5.5 | Ingestion quarry-edge/runtime/core; gateway browser.rs one field | WS2.B | soft: WS2.B (the monolith is possible at higher conflict cost) |
| WS2.D | S4 | run registry, concurrent-run caps (429 reasons), idle and absolute reaper (dry-run first), close_run leak fix, agent.expired | L | 6.5 | Ingestion quarry-edge/runtime | WS2.B | soft: WS2.C skeleton (RunLive/RunHandle, 0.5 d shared PR) |
| WS2.E | S5 | hide the fleet console until a fleet producer exists | S | 0.5 | Frontend SPA | none |  |
| WS2.F | S6 | wire-code accuracy for agent routes (status_err mislabels) | S | 1 | Ingestion quarry-edge | none | `OPT`; soft: better after WS2.C |

### WS3 Browser driver stability and actionability (design D3, wave W1, 44 d total, 39 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS3.A | S0 | Chromium sandbox default on in all compose files, seccomp profile, production refusal of QUARRY_BROWSER_NO_SANDBOX | S | 2 | Ingestion compose + quarry-browser | none |  |
| WS3.B | S1 | live-browser CI lane (quarry-browser-live) and repair of the rotted CHROMIUMOXIDE_TEST tests | S | 2 | Ingestion CI + quarry-browser tests | none | soft: WS3.A |
| WS3.C | S2 | per-page viewport emulation and page hygiene; viewport removed from browser identity (F-t2-007 part A) | M | 3 | Ingestion quarry-browser | none | soft: WS3.B |
| WS3.D | S3 | one CDP deadline table, typed Timeout, bounded tab attach, retry-once on destroyed context (F-t2-026); lands in one short window (about 20 call sites) | M | 3 | Ingestion quarry-browser/core | WS3.C | soft: merge order after WS3.C |
| WS3.E | S4 | delete the dead stealth code (F-t2-027 deletion, no ADR) | S | 1 | Ingestion quarry-browser | none |  |
| WS3.F | S5 | driver admission cap, idle reaper, crash detection, page-opening health probe behind /ready (F-t2-007 part B, K=1) | L | 6 | Ingestion quarry-browser, quarry-edge /ready | WS3.C | soft: WS3.D |
| WS3.G | S6 | slot pool of K Chromium processes with retire-after-N | M | 5 | Ingestion quarry-browser | WS3.F | `OPT`; soft: ADR 10 before any isolation claim |
| WS3.H | S7 | closed block taxonomy, non-signals, single-recovery enforcement, ChallengeDetected made real (F-t2-027 core) | M | 5 | Ingestion quarry-runtime/core/edge | WS2.B | soft: WS3.D |
| WS3.I | S8 | narrow live evasion-adjacent triggers, then rename the stealth preset (default flips honest) | M | 3 | Ingestion quarry-runtime, quarry-control presets | WS3.H | `GATED`: ADR 5 (anti-bot) gates the default |
| WS3.J | S9 | Quarry-to-Verevon plumbing of the blocked state (execution-core, model-gateway, gateway, SPA) | L | 6 | Cross-plane: Model Plane + Frontend (4 d sibling planes) | WS3.H |  |
| WS3.K | S10 | actionability pipeline and trusted input for ref-targeted click/type/select (F-t2-015; W2-timed slice inside WS3) | L | 8 | Ingestion quarry-browser/runtime | WS3.C, WS3.D | soft: WS3.B live lane green two weeks before QUARRY_BROWSER_TRUSTED_INPUT=on; **wave moved to W2** (design places it in the other wave) |
| WS3.L | S11 | rich input follow-ups (drag, hover, file input); each needs its own design | - | unscheduled | Ingestion quarry-browser | WS3.K | `UNSCHED` |

### WS4 ZDR and tenant boundary (design D4, wave W1, 26.75 d total, 26.75 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS4.A | S-1 | gateway content-cache admission: org-scoped keys, ZDR bypass, zdr:true forwarded to Quarry, GATEWAY_RESULT_CACHE_CONTENT lever (F-t2-003 phase 1) | M | 3 | Frontend gateway (search.rs, knowledge/quarry.rs, cache.rs, zdr.rs) | none |  |
| WS4.B | S-2 | incident response: forensics (steps 0-2 BEFORE S-1 deploys), purge script, hit-rate answer to audit question 3 | S | 1 | Frontend gateway scripts and runbook | none | soft: purge after WS4.A is on every replica |
| WS4.C | S-3 | organisation erasure reaches the gateway result cache | S | 0.75 | Frontend gateway gdpr_nats.rs | WS4.A |  |
| WS4.D | S-4 | close known ZDR leaks in logs and meter metadata (F-t2-004 phase 1a) | S | 1.5 | Ingestion quarry-edge routes.rs, quarry-runtime pipeline.rs/serp.rs | none | soft: parallel with WS4.A |
| WS4.E | S-5 | ZDR carrier and event filter for logs: zdr span field, marker layer, SENSITIVE_FIELDS (F-t2-004 phase 1b) | M | 3 | Ingestion quarry-edge telemetry/routes + gateway main.rs | WS4.D |  |
| WS4.F | S-6 | reqwest URL stripping sweep and the log-field discipline lint with ratchet baseline | S | 2.5 | Ingestion + gateway | WS4.D, WS4.E |  |
| WS4.G | S-7 | Quarry ZDR feature matrix v1: Rust table, golden, route-scan completeness, behaviour tests (F-t2-003 phase 2a) | L | 6 | Ingestion quarry-core/edge + scripts/tests | none | soft: WS4.D shares the harness |
| WS4.H | S-8 | enforce the gap rows with catalogue-coded refusals (observe, then enforce) (F-t2-003 phase 2b) | S | 2.5 | Ingestion quarry-edge | WS4.G | `GATED`: ADR topic 4 (ZDR per feature) |
| WS4.I | S-9 | consume the Model Plane ZDR catalogue; add Quarry feature keys | S | 1.5 | Ingestion quarry-edge + Model Plane catalogue | WS4.G | `GATED`: waits for the Model Plane Z-05a/Z-05b commit |
| WS4.J | S-10 | metadata-only ZDR run tombstone with retention janitor (migration 015) | M | 5 | Ingestion quarry-control + quarry-edge | WS4.G | `GATED`: ADR topic 4, legal review, QB-T17 state.rs commit |

### WS5 Scrape contract and file routing (design D5, wave W1, 41.5 d total, 24 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS5.A | S1 | content-kind sniff, typed unsupported_media (415), hard body cap (F-t2-009 a) | S | 2 | Ingestion quarry-transform/runtime/core | none |  |
| WS5.B | S2 | PDF and DOCX parse routing, bounded parsing, ZDR inline text; QUARRY_EDGE__DOC_ROUTING shadow, skip, on (F-t2-009 b) | M | 5 | Ingestion quarry-transform/runtime/edge | WS5.A |  |
| WS5.C | S3 | parser sandbox: process isolation for parsing; gate for DOC_ROUTING=on in multi-tenant production (F-t2-009 c) | M | 4 | Ingestion quarry-edge doc_parse_child | WS5.B | `OPT` |
| WS5.D | S4 | ScrapeOptions contract: formats[], inline return (ZDR path), effective options, strict validation (F-t2-005 a) | M | 5 | Ingestion quarry-core/edge/runtime | WS5.B |  |
| WS5.E | S5 | clean_html stage, selectable extractor, selectors, golden fixtures (F-t2-005 b) | M | 4 | Ingestion quarry-transform/runtime | WS5.D |  |
| WS5.F | S6 | scrape record and GET /v1/scrape/:id (F-t2-005 c) | S | 2 | Ingestion quarry-edge/core | WS5.D |  |
| WS5.G | S7 | consumers: model-gateway inline path, gateway cleanup, SPA types (F-t2-005 d) | M | 3 | Model Plane model-gateway (client), Frontend gateway/SPA | WS5.D | soft: WS5.D deployed first |
| WS5.H | S8 | OpenAPI from types, route-coverage CI, stale-doc fixes (F-t2-005 e; the do-now slice of F-t2-064) | M | 3 | Ingestion docs, CI | none | soft: route-coverage half independent; schema half needs WS5.D |
| WS5.I | E1 | crawl and batch embedding of ScrapeOptions (design-level epic; blocked on WS14/F-t2-040) | L | 13.5 | Ingestion quarry-edge, orchestrator | WS5.D, WS14.D | `UNSCHED` |

### WS6 Budgets, capacity, metering (design D6, wave W2, 42.5 d total, 42.5 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS6.A | S0 | crawl max_pages 0 = unlimited, failed pages and depth unbounded: finite defaults and hard ceilings, effective limits returned (F-t2-008 immediate fix; hotfix, pulled into W1) | S | 1.5 | Ingestion orchestrator Go, quarry-edge, gateway | none | **wave moved to W1** (design places it in the other wave) |
| WS6.B | S1 | budget accumulator and enforce_budget for agent-class runs; BUDGET_EXHAUSTED 402; shadow then on (F-t2-008 slice 1) | M | 5 | Ingestion quarry-edge/runtime (+1.5 d Model Plane model-gateway, sibling b) | WS2.C | soft: soft: monolith possible; WS2.D finish_run |
| WS6.C | S2 | usage metering for browser time, agent runs, crawl pages, extract, map; ZDR-clean metadata (F-t2-046 phase A) | L | 6 | Ingestion quarry-edge/runtime | WS2.D | soft: WS4.D scrape metadata; Lago codes hard only for flag on |
| WS6.D | S3 | limits resolver, billing-core quota read, clamp to remaining entitlement with the value returned (F-t2-008 2a, F-t2-025 numbers, F-t2-046 plan keys) | M | 5 | Ingestion quarry-edge + Control billing-core | none | soft: WS6.C; Control credential for billing_core mode (static mode needs none); fail-mode ADR topic 1 |
| WS6.E | S4 | budget claim in the BrowserBroker grant: proto 40-44, broker migration 0003, model-gateway mint, Quarry reader (F-t2-008 slice 2b) | M | 5 | Model Plane broker/model-gateway + Ingestion quarry-edge (+2 d Model Plane, +2 d execution-core sibling b) | WS1.B, WS6.B | soft: WS6.D |
| WS6.F | S5 | gateway class-aware, org-keyed rate limits (F-t2-025 part a) | S | 1.5 | Frontend gateway rate_limit.rs | none |  |
| WS6.G | S6 | per-org capacity in Quarry: plan-aware agent run caps and per-org render permits (F-t2-025 part b) | M | 4.5 | Ingestion quarry-edge/runtime | WS2.D | soft: WS6.D |
| WS6.H | S7 | job admission, bounded backlog and fair share at quarry-control (migration; /v1/internal/jobs/claim) (F-t2-025 part c) | M | 5 | Ingestion quarry-control, orchestrator, edge | WS15.B | soft: WS6.D; control, orchestrator and edge deploy together |
| WS6.I | S8 | Quarry Prometheus recorder, internal /metrics, stale docs corrected (F-t2-029 phase 1) | S | 2 | Ingestion quarry-edge | none | soft: WS2.D RegistryStats; conflicts with WS4.E router layers |
| WS6.J | S9 | graceful drain of runs, streams, usage (F-t2-029 phase 1b) | M | 3 | Ingestion quarry-edge/runtime | WS2.D, WS6.C |  |
| WS6.K | S10 | per-run statistics for crawl jobs (migration) | M | 4 | Ingestion quarry-control/orchestrator | none | soft: shares the jobs table with WS6.H |

### WS7 Web policy / ResolvedPolicy (design D7, wave W2, 52.5 d total, 48.5 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS7.A | S0 | ledger entry: policy owner (audit topic 1) and claim carrier | S | 0.5 | docs | none | `GATED`: ADR topic 1 |
| WS7.B | S1 | quarry_core::web_policy: catalogue, lattice, ResolvedPolicy, typed denials (pure library) | M | 3 | Ingestion quarry-core | none | soft: catalogue key names reviewed with Control |
| WS7.C | S2 | edge wiring for the scrape family: replace RunPolicy::default(), RequestAsk, policy_baseline.rs | M | 4 | Ingestion quarry-edge | WS7.B | soft: WS5.D (ScrapeRequest field) |
| WS7.D | S3 | agent path: start-run and per-step gates (agent_policy_gate.rs) | M | 4.5 | Ingestion quarry-edge | WS7.B, WS7.C | soft: WS2.A, WS2.B, WS2.C |
| WS7.E | S4 | remaining edge routes (search, extract, profile) and ZDR consolidation | M | 3.5 | Ingestion quarry-edge | WS7.C | soft: in-flight search_routes.rs diff committed; WS17.B |
| WS7.F | S5 | reconcile and retire: JobPolicy rename, preset requires, /v1/blocklists 410 | S | 2 | Ingestion quarry-core, quarry-control | WS7.B |  |
| WS7.G | S6 | backstops: PolicySecurityEngine, final-URL recheck, egress deny list; 6b durable-job snapshot | M | 4.5 | Ingestion quarry-runtime/browser/orchestrator | WS7.C | soft: 6b needs WS14 and WS15 |
| WS7.H | S7 | org-core storage, admin API, internal read (migrations 021-023) | L | 6.5 | Control org-core (Go) | none | `GATED`: ADR topic 1 |
| WS7.I | S8 | auth-core claim minting, registry webPolicyByAudience, goldens in three trees | L | 7.5 | Control auth-core (TS) | WS7.H | `GATED`: ADR topic 1; soft: Control sign-off |
| WS7.J | S9 | Quarry claim intake and the observe to enforce switch | M | 3 | Ingestion quarry-edge | WS7.C, WS7.I | `GATED`: ADR topic 1 |
| WS7.K | S10 | broker web-policy snapshot in grants (proto field 20, migration 0002), PolicyResolver implementation, Quarry validator field | L | 6.5 | Model Plane broker + Ingestion quarry-runtime | WS1.B, WS7.I | `GATED`: ADR topic 1; soft: WS1.C for persistence; merge WS1 first |
| WS7.L | S11 | Model-driven runs: request layer and denial rendering (sibling b owns the code) | M | 3 | Model Plane execution-core | WS7.D | soft: WS7.K |
| WS7.M | S12 | revocation push | M | 4 | Ingestion + Model Plane | WS7.H, WS7.K | `OPT` |

### WS8 Shared browser backend (design D8, wave W2, 68 d total, 63 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS8.A | S8.0 | spikes, measurements and the ledger entry (no product code) | S | 1.5 | docs | none | soft: ADR topic 3 ratification |
| WS8.B | S8.1 | Quarry: open-web navigation policy, human step semantics, human evidence retention (F-t2-011 Quarry half) | L | 6 | Ingestion quarry-edge/runtime | WS8.A | soft: WS2.B |
| WS8.C | S8.2 | BrowserBroker: human_browse kind, RenewGrant, attach derive purpose (proto 50-59), model-gateway routes | L | 8.5 | Model Plane broker + model-gateway | WS1.A, WS1.B | `GATED`: ADR topic 3 (D8-1..3) ratification and proto allocation; soft: WS1.C for production |
| WS8.D | S8.3 | gateway and SPA: human sessions, keep-alive, remove the 12-step / 120-second literals | M | 4 | Frontend gateway + SPA | WS8.C |  |
| WS8.E | S8.4 | driver foundation: background tab, tab pin, per-tab egress, tab-scoped collectors (F-t2-013 phase 2) | L | 6 | Ingestion quarry-browser | none | soft: merge after WS3.C and WS3.D |
| WS8.F | S8.5 | Quarry attach core: tab ledger, caller resolution, per-tab control, attach endpoints (migration 0011) | L | 12 | Ingestion quarry-edge/runtime/core | WS1.B, WS2.B, WS2.C, WS8.E |  |
| WS8.G | S8.6 | model-gateway and execution-core attach mode | L | 7 | Model Plane execution-core + model-gateway | WS8.C | soft: WS8.F (can start against wiremock); WS11.A, WS11.B, WS11.D first in browser_agent.rs |
| WS8.H | S8.7 | execution lanes: human and agent actions run concurrently (GA gate for ATTACH) | L | 6 | Ingestion quarry-edge | WS8.F |  |
| WS8.I | S8.8 | gateway and SPA: start with attach, agent tab, Reveal, Take over, Give back, Stop | L | 7 | Frontend gateway + SPA | WS8.C, WS8.F, WS8.G, WS8.H |  |
| WS8.J | S8.9 | phase 3: deliverable and handoff marks, turn-scoped release, orphan sweep | M | 5 | Ingestion + Model Plane + SPA | WS8.I | soft: WS2.D (orphan sweep) |
| WS8.K | S8.10 | acting-user claim on attach (phase B hardening) | M | 5 | Model Plane + Control registry | WS8.F | `OPT` |

### WS9 Shared browser frontend (design D9, wave W2, 47 d total, 47 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS9.A | S1 | omnibox: resolver, local history, suggestions, in-frame search results (F-t2-011a) | M | 5 | Frontend SPA + gateway search | none | soft: WS4.A before the production flag; WS8 for usefulness |
| WS9.B | S2 | reload, hard reload, navigation error page (F-t2-011a second half) | M | 4 | Frontend SPA + gateway + Quarry reload action | none | soft: WS3.D order in chromiumoxide.rs; Quarry deployed before gateway |
| WS9.C | S3a | Quarry devtools filters and gateway evidence proxies (receipts, egress-receipts, procedure) (F-t2-014 first slice) | M | 5 | Ingestion quarry-edge + gateway | none | soft: WS2.B rebase |
| WS9.D | S3b | evidence panels in BrowserChrome | M | 4 | Frontend SPA | WS9.C |  |
| WS9.E | S3c | browser.console Model tool and the untrusted-content classification fix (sibling b) | M | 4 | Model Plane execution-core/model-gateway | WS9.C | soft: WS1 for end-to-end evidence |
| WS9.F | S4a | useBrowserSessionController without touching KnowledgeComposer | M | 4 | Frontend SPA | none |  |
| WS9.G | S4b | dock positions around the live-run panel (VITE_CHAT_BROWSER_DOCK) | M | 5 | Frontend SPA | none | soft: ChatPage.tsx in-flight edits |
| WS9.H | S4c | interactive docked session, ask-about-this-page, chat per tab | M | 4 | Frontend SPA | WS9.F, WS9.A | soft: WS8 for general availability |
| WS9.I | S5 | control strip, tab provenance markers, Reveal (F-t2-013 UI step c1) | M | 3 | Frontend SPA + gateway | WS2.C | soft: WS2.C deployed |
| WS9.J | S6a | bind the panel to a live session: observed live view, unary frame and evidence RPCs (F-t2-012b) | L | 9 | Frontend + Model Plane relay routes | none | soft: Model Plane half (sibling b + WS8); WS9.C |
| WS9.K | S6b | attach UI: owner-driven shared session (design-level) | - | unscheduled | Frontend SPA | WS8.I | `UNSCHED`; soft: ADR topic 3 |
| WS9.L | S7 | agent-flagged tabs, background open, per-step tab targeting (outline) | - | unscheduled | Frontend SPA | WS8.J | `UNSCHED`; soft: ADR topic 3 |

### WS10 Run trace envelope (design D10, wave W2, 47.5 d total, 47.5 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS10.A | S1 | x-request-id intake: Quarry tower layer, codemod over handlers, gateway adopt and forward, Go parity | M | 3 | Ingestion quarry-edge + gateway + Go | none | soft: WS4.E shared span hunk; quiet window (codemod) |
| WS10.B | S2 | envelope contract: events.proto fields 14-18 and event types 150-156, codegen, JSON twins, golden, buf gate repair | M | 4 | Model Plane proto + Ingestion contracts | none |  |
| WS10.C | S3 | Quarry trace core: one mint, per-run sequence, replay ring, real from_seq, close_run fix, gateway events proxy | M | 5 | Ingestion quarry-edge/runtime | none | soft: WS2.B |
| WS10.D | S4 | run origin, trace context, new trace event types, allowlisted projection, Ingestion-bus exclusion | M | 4.5 | Ingestion quarry-edge/runtime/core | WS10.B, WS10.C | soft: WS2.C migration 0010 and fields |
| WS10.E | S5 | Quarry to Model Plane publisher and NATS topology (principal quarry-trace-producer) | M | 4.5 | Ingestion + Model Plane NATS | WS10.D | soft: security review of the new NATS principal |
| WS10.F | S6 | session-core persistence (migration 0046), EventLog.QueryEvents, model-gateway and gateway trace read | L | 10 | Model Plane session-core/model-gateway + gateway | WS10.B, WS10.E | soft: session-core migrations 0044 and 0045 committed |
| WS10.G | S7 | Application Plane projection (Convex) | M | 4.5 | Application convex-core | WS10.E | `GATED`: ADR 8 part 2 (Application projection and Space reads) |
| WS10.H | S8 | F-t2-004 phase 2: telemetry contract, exporter flag, ZDR sampler, late-ZDR registry, span processor (Rust and Go) | L | 8 | Ingestion quarry-edge + Go quarryotel + gateway | WS4.D, WS4.E, WS4.F |  |
| WS10.I | S9 | cross-plane contract tests and volume soak | M | 4 | Cross-plane | WS10.F |  |

### WS11 LLM planner default-on and memory guard (design D11, wave W2, 29.5 d total, 29.5 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS11.A | S0 | planner seam, prompt hygiene, hermetic harness (F-t2-010 b first half) | S | 2 | Model Plane execution-core | none |  |
| WS11.B | S1 | budgets, ceilings and typed resource outcomes (token budget); the shared run-parameter mint helper in model-gateway browser_run.rs | M | 4 | Model Plane execution-core + model-gateway | WS11.A |  |
| WS11.C | S2 | staged org-policy flag (quota keys browser_agent_llm_planner) replaces the env gate | M | 3 | Model Plane execution-core + model-gateway org_quota | WS11.A | soft: Control sign-off on quota-key convention for production |
| WS11.D | S3 | recoverable failures, the browse/1 procedure, action log, honest completion | M | 5 | Model Plane execution-core | WS11.A, WS11.B |  |
| WS11.E | S4 | widened action set and the risk gate that covers it | L | 7 | Model Plane execution-core | WS11.D |  |
| WS11.F | S5 | staged rollout switch, boot guard (DEFAULT_ON_ACK), metrics | S | 2 | Model Plane execution-core | WS11.C | soft: stage 3: WS1, WS6.B, WS6.E, WS12.B-E, F-t2-044 Tier B (epic W3), OD13 |
| WS11.G | S6 | connector-first routing guidance (prompt) | M | 3 | Model Plane execution-core | none |  |
| WS11.H | S7 | web-taint memory guard, skip-only (F-t2-045) | M | 3.5 | Model Plane execution-core + session-core (migration 0047) | WS11.A | soft: session-core 0044/0045 committed |

### WS12 Secret redaction (design D12, wave W2, 29.5 d total, 27.5 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS12.A | S0 | rule module, golden fixture and conformance harness (pure, nothing wired) | M | 3.5 | Ingestion quarry-core/redaction + scripts | none |  |
| WS12.B | S1 | field-facts probe and AX/target masking in the driver | M | 4 | Ingestion quarry-browser | WS12.A | soft: WS3.D (deadline wrapper); WS3.K rebase |
| WS12.C | S2 | observation-level masking, rule-version stamp, events and error surfaces | M | 4 | Ingestion quarry-runtime/core/edge | WS12.A, WS12.B | soft: WS2.B/C, WS3.H (BrowserObservation literals) |
| WS12.D | S3 | receipts, typed text, checkpoints, procedures (migration 0012 tentative) | M | 4 | Ingestion quarry-runtime/edge | WS12.A, WS12.C | soft: WS2.C, WS8.B, WS6.B (migration numbers) |
| WS12.E | S4 | Model Plane shared mp-scrub crate, planner-prompt scrub, event scrub, stamp consumption | M | 3.5 | Model Plane (mp-scrub, execution-core, model-gateway) | WS12.A | soft: WS12.C wire stamp; WS11 llm_planner.rs ordering |
| WS12.F | S5 | screenshot masking by element rectangle (fail closed) | L | 7 | Ingestion quarry-browser | WS12.B, WS12.C | soft: WS3.C |
| WS12.G | S6 | logs: nothing page-derived reaches tracing, enforced by a lint test | S | 1.5 | Ingestion quarry-core/edge | WS12.A |  |
| WS12.H | S7 | scrub legacy durable rows (only if the product owner asks; migration 0013) | S | 2 | Ingestion quarry-runtime | WS12.D | `OPT`; soft: product decision |

### WS13 Scrape-time actions, screenshot and PDF (design D13, wave W2, 35 d total, 33 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS13.A | 1 | isolated, org-stamped scrape sessions and a heavy lane (prerequisite) | S | 2 | Ingestion quarry-runtime browser_driver.rs | none |  |
| WS13.B | 2 | contract types: action wire enum, capture options, validation (quarry-core only) | M | 3 | Ingestion quarry-core | none | soft: WS5.D for ScrapeFormat edits |
| WS13.C | 3 | driver capture API and Chromiumoxide single-capture implementation | M | 4 | Ingestion quarry-browser | none | soft: WS3.C, WS3.D rebase |
| WS13.D | 4a | FetchResponse.captures plumbing (mechanical) | S | 1 | Ingestion quarry-runtime | none | soft: quiet-window PR |
| WS13.E | 4b | formats end to end: screenshot and pdf on /v1/scrape (first shippable half) | M | 5 | Ingestion quarry-edge/runtime | WS13.B, WS13.C, WS13.D, WS5.D | soft: WS4.A Admission |
| WS13.F | 5 | scrape-time actions[] on the same page (second shippable half) | L | 7 | Ingestion quarry-edge/runtime/browser | WS13.A, WS13.E | soft: WS2.A (Evaluate deny) |
| WS13.G | 6 | tile-and-stitch for pages taller than one capture | M | 5 | Ingestion quarry-browser | WS13.C, WS13.E |  |
| WS13.H | 7 | gateway artifact route, preview admission, SPA rendering | M | 4 | Frontend gateway + SPA | WS13.E |  |
| WS13.I | 8 | contract docs, OpenAPI, SDK, parity-doc corrections, e2e script | S | 2 | Ingestion docs | WS13.E, WS13.F, WS13.G |  |
| WS13.J | 7b | surface agent-path PDFs in the run timeline | S | 2 | Frontend + Model Plane | WS13.H | `OPT` |

### WS14 Crawl scope and robots (design D14a, wave W2, 36.5 d total, 36.5 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS14.A | S0 | truth-telling docs and characterization tests | S | 1.5 | Ingestion docs + tests | none |  |
| WS14.B | S1 | hermetic fixture site and real-stack e2e harness | M | 3 | Ingestion compose overlay + tests | none |  |
| WS14.C | S2 | Rust crawl filter and run_page crawl context (inert until used) | M | 4 | Ingestion quarry-core/runtime/edge | none | soft: WS14.B tests |
| WS14.D | S3 | Go scope default, typed denials, batch enqueue, replay safety, minimal UI summary (crawl-scope-v1) | L | 6 | Ingestion orchestrator + quarry-edge + gateway/SPA | WS14.C, WS6.A |  |
| WS14.E | S4 | robots gate: guarded fetch, RFC 9309 semantics, Crawl-delay into the scheduler | L | 7 | Ingestion quarry-runtime/transform | WS14.C, WS14.D |  |
| WS14.F | S5 | batch check (same fetch-time gate for batches) | S | 2 | Ingestion quarry-edge/orchestrator | WS14.D | soft: WS14.E |
| WS14.G | S6 | scoping vocabulary on the crawl request, gateway, action schema and UI | L | 6 | Ingestion edge + gateway + SPA | WS14.D |  |
| WS14.H | S7 | ignore-robots entitlement and audited use (+2 d Control Plane) | L | 6 | Ingestion + Control audit-core | WS7.C, WS7.H | `GATED`: ADR topic 1 (override owner); soft: Control Plane credential; grantor decision |
| WS14.I | S8 | rollout closeout: flip scope to enforce after soak | S | 1 | Ingestion config | WS14.C, WS14.D, WS14.E, WS14.F, WS14.G |  |

### WS15 Job lifecycle and schedules (design D14b, wave W2, 45.5 d total, 42 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS15.A | S0 | hide what the backend cannot honour (schedule capabilities all false) | S | 1 | Frontend gateway + SPA | none | soft: ships first |
| WS15.B | L1 | job record v2, origin, status/errors/ongoing read surface (migration 016; jobCols refactor first and alone) | M | 4.5 | Ingestion quarry-control + edge | none | soft: WS6.A soft |
| WS15.C | S1 | tenant-safe schedule persistence and creation (closes cross-tenant schedule list and control; migration 017) | M | 4.5 | Ingestion quarry-control + edge | none | soft: WS4.H shared refusal call site; ship early |
| WS15.D | L2 | cancel, pause, resume: desired state, orchestrator control loop, batch handlers, typed timeout (lifecycle-v1, batch-control-v1) | L | 7 | Ingestion orchestrator + control + edge | WS15.B |  |
| WS15.E | S2 | executable schedules: ScheduledJobWF, workflow mapping, overlap, timezone (QUARRY_SCHEDULED_JOBS_ENABLED stays 0) | L | 6.5 | Ingestion orchestrator + control | WS15.B, WS15.C | soft: WS6.D entitlement leg |
| WS15.F | L3 | live job-event tail with snapshot, Last-Event-ID and per-org caps | M | 4 | Ingestion quarry-edge + control | WS15.B | soft: WS15.D |
| WS15.G | L5 | gateway proxy, SPA controls and Model Plane registry actions | M | 4 | Frontend gateway/SPA + Model Plane registry | WS15.B, WS15.D | soft: WS15.F |
| WS15.H | S3 | trigger and backfill through pull-model request rows (migration 018) | M | 4 | Ingestion orchestrator + control + edge | WS15.E | `GATED`: decision D14 (command transport) |
| WS15.I | S4 | fire-time posture and entitlement, skip reasons, failure notices, runtime state (monitor-preflight-v1) | M | 5 | Ingestion + Control grant + Application notification-core | WS15.E | `GATED`: D15 (ZDR for schedules), D17 (notification principal) partly; soft: WS6.D, WS4.H |
| WS15.J | S5 | go-live, runbook and documentation; first schedule fire | S | 1.5 | Ingestion | WS15.I, WS14.D, WS6.A | soft: S4 observed one week; F-t2-016 scope deployed |
| WS15.K | L4 | paged job results, phase 1 page index | M | 3.5 | Ingestion quarry-control/edge | WS15.B | `OPT`: decision D9 results source |

### WS16 Structured extraction (design D15, wave W2, 50 d total, 41 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS16.A | S0 | decision record DR-15.1, stale claims corrected | S | 1 | docs | none |  |
| WS16.B | S1 | Model Plane service lane: bounded, metered, own rate class, golden route table (owner sibling b) | M | 5 | Model Plane model-gateway | none |  |
| WS16.C | S2 | the grant: registry, manifest rows, pins, specs | M | 3 | Control registry + Ingestion manifest | WS16.A | `GATED`: DR-15.1 (adjacent topics 1 and 4); soft: DR-15.1 ruling; Control co-signs |
| WS16.D | S3 | Quarry model client lane and the hermetic fake gateway | M | 5 | Ingestion quarry-runtime/edge | WS16.B |  |
| WS16.E | S4 | schema normalisation, validator extension and the json engine with one repair pass | M | 5 | Ingestion quarry-runtime/core | WS16.D |  |
| WS16.F | S5 | harvest first, verbatim highlights, question, summary | M | 5 | Ingestion quarry-runtime | WS16.E |  |
| WS16.G | S6 | Quarry API surface and governance: formats, /v1/extract facade, scopes, budgets, metering, ZDR | L | 8 | Ingestion quarry-edge | WS5.D, WS16.F, WS16.B, WS16.C | soft: WS6.D soft |
| WS16.H | S7 | frontend gateway forwarding of the schema key | S | 2 | Frontend gateway runs.rs | WS16.G |  |
| WS16.I | S8 | interim Model-proposed route needing no Quarry grant (MODEL_GATEWAY_EXTRACT_TOOL) | L | 7 | Model Plane model-gateway tool_loop | none | soft: independent of S1-S7; owner sibling b |
| WS16.J | S9 | crawl and batch embedding of AI formats (design-level) | L | 9 | Ingestion | WS5.I, WS15.D, WS6.A, WS6.D | `UNSCHED`; soft: F-t2-040 and F-t2-017 |

### WS17 Profile hardening (design D16, wave W2, 48.5 d total, 45.5 d without optional/unscheduled)

| Slice | Design slice | Goal | Effort | Days | Owner plane/service | Hard deps | Notes |
|---|---|---|---|---|---|---|---|
| WS17.A | S0 | CDP capability spike (no product code merged except tests) | S | 1.5 | Ingestion quarry-browser tests | WS3.B |  |
| WS17.B | P0 | profile route guard, redacted reads, ZDR and service-principal refusal (no schema change) | M | 3 | Ingestion quarry-edge profile_routes/guard | none | soft: WS4.H helper (soft); WS10.A merge order |
| WS17.C | P1 | monotonic saves, deletion tombstones, exists (migration 0014) | M | 4 | Ingestion quarry-runtime/browser | none |  |
| WS17.D | E1 | envelope layer below the cache, blob-store split, interim env key provider (migration 0015) | L | 8 | Ingestion quarry-runtime | WS17.C | soft: external review of crypto module before REQUIRED=1 |
| WS17.E | C1 | complete snapshot capture in quarry-browser (cookies, storage, IndexedDB; capture v2) | L | 7 | Ingestion quarry-browser | WS17.A | soft: WS3.D (deadline table) |
| WS17.F | P2 | owner and ACL enforcement on profiles | M | 4 | Ingestion quarry-edge | WS17.C | soft: WS2.B split; WS1.B delegated bearer |
| WS17.G | C2 | restore (first-tab-of-context only) | M | 5 | Ingestion quarry-browser | WS17.E, WS3.C | soft: WS8 attach |
| WS17.H | E2 | Control Plane key API in org-core and the org_core key provider (org-core migration 024) | L | 8 | Control org-core (Go) + Ingestion | WS17.D | `GATED`: key-management ownership and master-key custody; soft: Control owner review; key-management ownership ADR (9.3 item 11); WS7.H migrations 021-023 first |
| WS17.I | E3 | backfill, erasure hop, tombstone and cache purge, then plaintext removal (migration 0016) | M | 5 | Ingestion quarry-runtime/control gdpr | WS17.D | soft: WS17.H for shredding; backfill window; WS4 tombstone path |
| WS17.J | S7 | single-writer profile lease for shared profiles | M | 3 | Ingestion quarry-runtime | WS17.F | `OPT` |

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

## 3. FILE-CONFLICT MATRIX

Built from the section 5 hotspot tables of designs D1 to D16 (line ranges are the designs' live-tree ranges and drift once earlier slices land; re-read before editing). Path prefixes: `Q/` = `apps/Ingestion Plane/Quarry-v2/`; `QE/` = `Q/crates/quarry-edge/src/`; `QR/` = `Q/crates/quarry-runtime/src/`; `QC/` = `Q/crates/quarry-core/src/`; `QB/` = `Q/crates/quarry-browser/src/`; `GW/` = `apps/Frontend Plane/verevonv3/apps/gateway/src/`; `FE/` = `apps/Frontend Plane/verevonv3/src/`; `MP/` = `apps/Model Plane/`; `BB/` = `MP/go/services/browser-broker/`; `EC/` = `MP/rust/services/execution-core/src/`; `MG/` = `MP/rust/services/model-gateway/src/`; `SC/` = `MP/rust/services/session-core/`; `CP/` = `apps/Control Plane/`. Provenance tags come from `tools/provenance.sh` run on 2026-10-15: `[C]` committed and unchanged, `[M]` committed but modified in the live tree (carries in-flight hunks), `[U]` untracked, `[NEW]` the plan would create it.

Merge-order rule shared by every row: slices are listed in the order they must merge; "queue" means the file has one serial merge queue and each later PR rebases (never hand-merges generated files or goldens). A file marked `[M]` also carries uncommitted hunks of the owner (PRE-1): those must be committed or consciously carried before the first lane edits it.

| # | Hot file (provenance) | Slices that touch it, in required merge order | Order rule and handling |
|---|---|---|---|
| 1 | `QE/agent_routes.rs` [C] (becomes `QE/agent_routes/*` after WS2.B) | WS2.A, WS2.B (split, freeze), WS2.C, WS2.D; WS1.A Quarry stream (`validate_run_grant` 314-338, `start_run` 1372-1413); WS4.E and WS4.H (one-line ZDR marks 1347-1353, 1443, 1469-1476); WS3.H (one field 1571-1599); WS7.D; WS6.B; WS12.C, WS12.D; WS10.C, WS10.D; WS8.B, WS8.F, WS8.H; WS9.C (1061-1066, 3454-3510); WS17.F (1415-1476) | WS2.A merges first (smallest edit, unblocks the split). WS2.B announces a 2-working-day freeze: no PR touching the file merges during it; open PRs rebase after. Nothing else merges here before WS2.B except WS2.A. After the split, edits land in `admission.rs`, `step.rs`, `lifecycle.rs`, `projection.rs`, `tabs.rs`, new `attach.rs`; the shared structs are `RunEntry`/`RunLive` (agree the field list in the 0.5-day skeleton PR). Gate order inside `step` is a contract: policy (WS7.D) and budget (WS6.B) before the sensitive-approval path (D2 request A2). |
| 2 | `QB/chromiumoxide.rs` [C], 3551 lines | WS3.A (2-line sandbox guard in `ensure_browser`), WS3.C, WS3.D (one short window, about 20 call sites), WS3.F; then in readiness order WS8.E, WS12.B, WS13.C, WS17.E, WS17.G; then WS3.K; then WS13.G and WS12.F; WS3.G (pool) last or never | D3's own order is S2, S3, then rebases of F-t2-020/021/023/034, then S5, S10, S6; D16 writes S2, S3, S5, S10, S6 then the others. Resolution: the W1 slices (WS3.C, D, F) merge before any W2 slice because they land first by wave; W2 slices then merge in readiness order, WS3.K and WS3.G last. Hunks: `open_tab_page` 371-557 (WS3.C, F, G, WS8.E, WS17.G), `ensure_browser` 194-271, `acquire`/`release` 1914-1986, screenshot/pdf 2392-2445 (WS13.C, WS13.G, WS12.F), input methods 2800-3062 (WS3.K, WS12.B). New modules (`slots.rs`, `deadlines.rs`, `actionability.rs`, `profile_capture.rs`, `profile_restore.rs`, `stitch.rs`, `field_probe.rs`) keep the hotspot diffs small. |
| 3 | `QB/lib.rs` [C] (capabilities 269-286, trait 413-430, `BrowserSession` 666-677) | WS3.C, WS3.F, WS3.K (`trusted_input` bit), WS8.E (`tab_pin`, 13 `BrowserSession` literals), WS13.C | All additive with `#[serde(default)]`; the one literal in `chromiumoxide.rs` 1873-1889 (`capabilities()`) is edited by three slices: land in the order listed. |
| 4 | `QE/routes.rs` [M] | (a) router layers 286-303: WS4.E, WS10.A, WS6.I. (b) scrape meter 488-509: WS4.D, WS6.C. (c) scrape handler 398-530 and `ScrapeRequest` 345-379: WS5.D, WS7.C, WS13.E, WS13.F, WS16.G. (d) crawl/batch handoff 590-725, `HandoffAck`: WS6.A, WS14.D, WS7.G(6b), WS15.B, WS15.D, WS6.H. (e) refusal helper 563-588: WS4.H, WS14.D, WS15.C. (f) `/ready` 312-326: WS3.F, WS6.J. (g) route table 125-293: WS5.F, WS15.B/D/F, WS16.G | Each region has its own order (a to g). The 60 s `TimeoutLayer` (lines 81, 303-305): WS5.D caps `timeout_ms` at 55 s; WS13.F raises the layer to 115 s only when actions are enabled: WS5.D must state both numbers. WS14.D and WS6.A edit the same `crawl_handoff`: WS6.A first, WS14.D consumes its `limits`. In-flight hunks must be committed first (PRE-1). |
| 5 | `QR/pipeline.rs` [C] | WS4.D (821-829), WS5.A and WS5.B (421-476), WS5.D and WS5.E (506-508, 595-705), WS7.C and WS7.G (436-448 final-URL), WS13.E (702-703, new `persist_captures`), WS14.C (298-331), WS16.E/F (`InlineParts`) | F-t2-050 (epic W3) must later insert its stage between markdown computation (506-518) and the artifact writes (601-683) and the `assemble_inline` path (recorded in D5). |
| 6 | `QE/main.rs` [M], `QE/state.rs` [M], `QE/config.rs` [M], `QE/lib.rs` [M], `QE/test_support.rs` [M], `QE/tests/support/mod.rs` [M] | WS1.A (182-201, 659-672), WS2.A to WS2.D (`mod` lines 26-47, 1284-1290, 1500-1517), WS4.E/H/J, WS6.A/D/G/I/J (`limits`, pools 323/337, meter 451-458, serve 1513-1535), WS7.C/J, WS10.A/C/E (405-445), WS12.C (`redaction_boot.rs`), WS14.D (`AppState` literal ~1431), WS8.B/F | Avoided on purpose by most designs. Rule: a PR adds at most one `mod` line and one `AppState` field; the `AppState {..}` literal count (16 per D5) means a field added without `Default` breaks every test harness: add fields via a builder or one shared helper PR. The uncommitted QB-T17 hunks (`config.rs +187`, `main.rs +188`, `state.rs` 57-93) land first (PRE-1). |
| 7 | `QR/step_receipts.rs`, `QR/postgres_step_receipts.rs` [C] + `QR/migrations/` | WS2.C (0010), WS6.B (0011 `cost_source`), WS12.D (`StepReceipt` + column), WS8.F (0011 `tab_attach` in the design), WS12.H (opt), WS17.C/D/I (0014-0016 provisional) | Migration numbers collide in the designs (D6 and D8 both name 0011; D12 tentative 0012/0013; D16 0014-0016). Plan rule: the integrating architect keeps one allocation table (section 3.1); first-come at merge, renumber only before first apply. Builder hunks (`StepReceipt` 183-210, `ReceiptBuilder` 453-556): WS2.C, WS6.B, WS12.D, WS8.B (human receipt function `redacted_for_record`), WS10.D. |
| 8 | `QR/observation.rs` [C] | WS2.A (arm 542-544), WS3.H (903-909), WS3.K (gate 364-371), WS12.C (386-396, 614-644, 965-989), WS6.B (budget reject) | Seven `BrowserObservation {..}` literals are edited by WS3.H, WS12.C, WS6.B, WS8.B: land the additive fields in one PR if two streams are in flight. |
| 9 | `QC/contracts.rs` [C], `QC/error.rs` [C], `QC/event.rs` [C], Go `Q/pkg/quarrycontracts/envelope.go` [C] | WS2.C (`USER_TOOK_OVER`, `agent.expired`), WS3.D (`Timeout.details`), WS3.F (`RateLimited.details`), WS3.H (`ChallengeSignal`), WS5.A (`UNSUPPORTED_MEDIA`), WS6.B (`BUDGET_EXHAUSTED`), WS9.B (`Reload` action 824), WS10.C/D (events), WS12.C/D (stamp, `ProofBundle`) | Append at the end of each enum; every new `ErrorCode` passes the mirror checklist of D2 S3 (error mapping, OpenAPI, Go envelope, SPA `errors.ts`). Textual neighbours, not semantic conflicts. |
| 10 | `Q/docs/openapi.yaml` [M] (about 4.5k lines) | WS2.A (2787, 3042), WS2.C (263-345), WS3.F (32-43, 2432-2458), WS5.H (generation from types), WS6.A (4361-4374), then every WS adding schemas | WS5.H's schema half generates from Rust types: after it lands, hand edits to generated schemas are forbidden (D5). Before that, additive edits only. In-flight hunks at 238-250 and 4012-4150 (+78 lines) are committed first (PRE-1). |
| 11 | `GW/domains/browser.rs` [C] | WS1.B (`create_session` 451-565, `close_session` 1023-1054, `start_ai_run` 1212-1382), WS2.C (`run_action` 663-760, `forward_quarry_failure` 3873-3885), WS8.D (literals 483-495), WS9.C/I, WS10.A/C (router 368-460, stream 1730-1782), WS6 (router 369-449), WS8.I, WS13.H (3922-3938) | D1's rule "merge WS1 first" holds for `create_session`/`start_ai_run`; WS2.C's hunk is disjoint (initiator field and two error translations), so no WS2/WS1 ordering is needed. The router block is edited by four slices: one route-table PR order, WS9.C, WS10.C, WS8.D. |
| 12 | `GW/domains/search.rs` [C] | WS4.A (51-293, 589-622), WS5.G (116-136, 175-188), WS9.A (524) | WS4.A first; the SPA production flag `VITE_BROWSER_OMNIBOX_SEARCH` stays 0 until WS4.A is deployed. |
| 13 | `GW/domains/knowledge/quarry.rs` [C] | WS4.A (`scrape_preview` 62-203, `CacheAdmission` 212-282 removed), WS6.A (`start_crawl` 306-376), WS5.G, WS13.H, WS14.D/G (`normalize_crawl_body` 386-416), WS15.G | WS4.A and WS6.A touch different functions and may merge in either order; WS5.G rebases over WS4.A. |
| 14 | `GW/main.rs` [C], `GW/config.rs` [C] (`AppState` flags) | WS4.A (mod line), WS4.E (tracing init 47-50, span 194-202), WS10.A, WS6.F (layers 143-148); config: WS1.B, WS8.D, WS8.I, WS6.F, WS4.A | `AppState` has five literals (`main.rs:312`, `browser.rs:4915`, `chat/shared.rs:745`, `orchestration.rs:632`, `config.rs:146`). WS4.A keeps its switch inside `ResultCache` precisely to avoid editing them; other slices must do the same or edit all five together. |
| 15 | `GW/domains/chat/shared.rs` [C] | WS1.B (`proxy_model_json_with_delegations` 629-690) | Shared with siblings (a) and (b) bearer plumbing: coordinate by name, one editor. |
| 16 | `FE/features/dashboard/home/BrowserChrome.tsx` [C], 2172 lines | WS9.A (846-851, 1166-1212), WS9.B, WS3.J (error copy), WS9.D (199, 1907-2004, 2026-2060), WS9.I (1073-1100, 1220-1245), WS8.D (keep-alive effect), WS8.I (tab strip, chip 1225-1245, takeover 497/624); then epics F-t2-028/033/038/070 | New code goes into new modules (`src/features/browser/{omnibox,evidence,dock,session}/*` [NEW]); the file receives only small wiring hunks. WS9.I and WS8.I wait for WS2.C deployed. |
| 17 | `FE/features/chat/components/ChatPage.tsx` [M], `use-chat-controller.ts` [M], `ChatLiveRunPanel.tsx` [C], `FE/features/chat/lib/chat-run-watch.ts` [C] | WS9.F, WS9.G, WS9.H, WS9.J | Seven uncommitted hunks plus sibling (a)/(b) chat edits: re-read before editing; WS9.G (dock layouts) is the slice most exposed. |
| 18 | `FE/shared/actions/action-registry.ts` [C], `GW/domains/actions/{handlers,dispatchers}.rs` [C], `FE/shared/i18n/errors.ts` [M] | WS9.A (omnibox action ids), WS15.G, WS14.G (no new id), WS16.H, WS2.E, WS3.J | `action-surface-contract.test.mjs` asserts registry ids equal handler ids, so a new action id edits both files in one PR. `errors.ts` additions are additive key blocks. |
| 19 | broker `BB/internal/grant/grant.go`, `server/server.go`, `authz/authz.go` [C], `grant/policy.go` [NEW] | WS1.A, WS1.B, WS1.C, then WS6.E, WS8.C, WS7.K; epics F-t2-024 (`allowedSensitiveActions` 32-36), F-t2-032, F-t2-036 | The largest conflict surface in the plan. WS1 merges first and owns the `Repository` interface; every later slice adds a field or a kind, not a rewrite. |
| 20 | `MP/proto/model_plane/v1/browser.proto` [C] + `MP/go/gen/model_plane/v1/browser*.pb.go` [C] + the vendored copy `Q/crates/quarry-runtime/proto/browser.proto` [C] | WS1.A/B (10-19, `DeriveRunGrant`), WS7.K (field 20 `WebPolicy`), [F-t2-032 reserved 21-39], WS6.E (40-44), WS8.C (50-53 and 50-59 reserved, `RenewGrant`, `DeriveRunGrant.resource_id=50`) | Field-number ledger in section 3.1. Generated code is regenerated on every rebase, never hand-merged; `buf` is not installed on the host and `buf.yml` is broken (PRE-3): regenerate in the pinned toolchain container. |
| 21 | `MP/proto/model_plane/v1/events.proto` [C] and twins | WS10.B (fields 14-18, event types 150-156), WS8 (event types 160 and up, any further envelope field 21 and up) | D10 owns 14-18 and 150-159; other workstreams take 160+. |
| 22 | `CP/config/plane-service-principals.json` [C], `CP/auth-core/src/auth/fixtures/*` (`quarry-token-claims.golden.json` and `.sha256` [C], `cross-plane-token-requests.json` [M]), `MG/tests/fixtures/auth-core/quarry-token-claims.golden.json` [U], `Q/crates/quarry-edge/tests/fixtures/auth-core/` [C] | WS1.A (quarry-edge to browser-broker row), WS7.I (claims, `webPolicyByAudience`, three golden copies), WS16.C (model-gateway audience on the same `quarry-edge` principal), WS17.H (org-core service credential), WS8.K (opt) | One registry PR per wave: hand-merge the JSON, regenerate the golden once and commit the three copies together. auth-core baseline is 15 failing tests in 3 suites on HEAD (S9): diff the failing sets, never trust the exit code. |
| 23 | `scripts/tests/zdr-contract-sync.test.sh` [M] (GOLDENS, VENDORED, CONSUMERS arrays), `scripts/tests/zdr-contract-schema.mjs` [M] | WS1.A (manifest row), WS4.G (matrix golden), WS12.A (secret-shapes golden), WS7.B/I (catalogue, claims), WS16.C, WS4.I/J, WS10.B/H (telemetry golden) | Append-only rows, one contiguous block per array; rebase last. The 28 untracked fixtures it needs must be committed first (PRE-1), or the gate fails on missing files. |
| 24 | quarry-control `Q/services/quarry-control/internal/store/pg/resources.go` [C], `store.go` [C], `resources/resources.go` [C], `cmd/control/main.go` [C], `.../pg/migrations/` | WS15.B (`jobCols` refactor first and alone), WS15.C, WS6.A (none), WS4.J (`main.go` 186-215), WS6.H, WS6.K, WS15.H, epics F-t2-031/040 | Eight `SELECT` lists mirror the `Job` struct: edit in one commit. Control migration numbers in section 3.1. |
| 25 | orchestrator `Q/services/quarry-orchestrator/internal/workflows/workflows.go` [C], `jobs/dispatcher.go` [C], `activities/activities.go` [C], `signals.go` | WS6.A (`crawl-bounded-v1`), WS14.D (`crawl-scope-v1`), WS7.G(6b), WS15.D (`lifecycle-v1`, `batch-control-v1`), WS15.E, WS15.I (`monitor-preflight-v1`), WS6.H, WS6.K | Unique `GetVersion` ids per slice. D14b's order: D6 S0 and D14a first, then S0, L1, S1, L2, S2, L3, L5, S3, S4, S5. WS15.E moves the input builders to `workflows/inputs.go` once; later fields land there. |
| 26 | `QE/profile_routes.rs` [C] | WS10.A (mechanical `RequestKind::new()` at 7 sites), WS17.B, WS4.H, WS7.E, WS17.F | D16's order, kept: D10 S1b, P0, D4 S-8, D7, P2. |
| 27 | `QE/extract_routes.rs` [M], `QE/search_routes.rs` [M], `QE/answer_routes.rs` [M] | extract: WS5.D, WS7.E, WS6.C (meter 344-360), WS16.G; search: WS7.E last | In-flight hunks (extract 205-228 and 512+; search rerank and ZDR_REQUIRED work) committed first; WS7.E edits search last (D7). |
| 28 | `QE/telemetry.rs` [C], gateway `observability.rs` [C] | WS4.E, WS6.I, WS10.H | WS10.H adds span attributes only after WS4.F lint is in place. |
| 29 | `EC/browser_agent.rs`, `EC/quarry_agent.rs`, `EC/llm_planner.rs`, `EC/tool_bridge/mod.rs` [C] | WS11.A, WS11.B, WS11.D (restructure the loop, add `StepFailureClass`), then WS3.J (`challenge`), WS8.G (attach mode, `PausedByUser` arm for `USER_TOOK_OVER`), WS12.E (scrub), WS6.B/E (cost mapping), WS7.L, WS9.E | One owner or strictly sequential PRs (D11). WS11.A creates `planner_view` and `procedure` modules so others add lines there. |
| 30 | `MG/browser_run.rs` [M] | WS1.B (derive call 337-348), WS11.B (the single helper that mints every run parameter), WS6.E (`mint_budget`), WS7.K, WS8.C/G | One editor at a time; rebase on the uncommitted ZDR hunks first (307-313, 628-632, 663-669, 995, 1328). |
| 31 | `MG/http_routes.rs` [M], `MG/auth.rs` [M], `MG/grpc.rs` [M], `MG/tool_loop.rs` [M], `MG/quarry.rs` [M] | WS5.G (`quarry.rs`), WS12.E, WS16.B, WS16.I, WS10.F (route), WS3.J | Sibling (b) owns most in-flight hunks (`tool_loop.rs:3039,3115`, `quarry.rs` 546 lines): rebase, never merge blind. |
| 32 | session-core `SC/src/grpc.rs` [M], `main.rs` [M], `dreaming.rs` [M], `SC/migrations/` | WS10.F (0046), WS11.H (0047); prerequisite 0044, 0045 and `retention_receipt.rs` [U] committed | Numbers assigned at merge: renumber WS11.H if WS10.F slips. |
| 33 | org-core `CP/org-core/migrations/`, `internal/http/server.go`, `service_auth.go` [C] | WS7.H (021-023), WS17.H (024) | Re-check the next free number on the PR day. |
| 34 | `.github/workflows/quarry-v2-ci.yml` [C], `model-plane-rust-ci.yml` [M], `contract-goldens.yml` [C], `verevonv3-ci.yml` [M] | WS3.B (new job after line 211), WS5.H (route-coverage step at 201-211), WS17 (Postgres job), WS1.A (`browser-broker-ci.yml`, `browser-grant-contract.yml` NEW), WS10.B (buf), WS13.I | New jobs only; the uncommitted `session-core-receipts` and `demo-product-build` jobs commit first (PRE-1). |
| 35 | compose files: `apps/Ingestion Plane/docker-compose.yml` [M] (quarry-edge env 53-160), `Q/deploy/compose/docker-compose.yml` [C], `docker-compose.production.yml` [C], `MP/deploy/docker-compose.yml` [C], `CP/docker-compose.yml` [C] | every WS adds env lines; WS3.A first (lines 109-110) | Append-only env blocks, one editor at a time. In-flight hunks at 121-160 of the Ingestion compose (switch defaults flipped to 0, S9 H6) commit first. |

### 3.1 Allocation ledgers (numbers the designs reserve; first-come at merge, renumber only before first apply to a shared environment)

| Namespace | Allocation as the designs reserve it | Collisions found and the plan's resolution |
|---|---|---|
| Quarry-runtime migrations (`QR/migrations/`, latest `0009`) | 0010 WS2.C; 0011 WS6.B `step_receipt_cost_source`; 0011 WS8.F `step_receipt_tab_attach`; 0012 WS12.D (tentative), 0013 WS12.H (opt); 0014-0016 WS17.C, WS17.D, WS17.I (provisional) | 0011 collides (D6 and D8). Expected order by readiness: 0010 WS2.C, 0011 WS6.B, 0012 WS17.C, 0013 WS12.D, 0014 WS8.F, 0015 WS17.D, 0016 WS17.I, 0017 WS12.H. Treat as an expectation, not a reservation; the table is edited in each PR. |
| Quarry-control migrations (`.../pg/migrations/`, next free 015) | 015 WS4.J; 016 WS15.B and 016 WS6.H (`jobs_admission`); 017 WS15.C and 017 WS6.K (`job_statistics`); 018 WS15.H | Two collisions (016 and 017). Resolution: WS15.B (jobCols refactor "first and alone") takes 016, WS15.C 017, WS15.H 018; WS6.H becomes 019 and WS6.K 020 (they merge after WS15.B because both edit the `jobs` table lists). WS4.J keeps 015 and may merge late: whether the runner tolerates a gap is `[UNVERIFIED]`; check before WS15.B applies. |
| org-core migrations | 021-023 WS7.H; 024 WS17.H (provisional) | none |
| browser-broker migrations | 0001 WS1.C; 0002 WS7.K (`web_policy`); 0003 WS6.E (`budget`) | WS6.E may merge before WS7.K (WS7.K sits behind ADR topic 1). It ships 0003 and 0002 stays reserved; check the `docker-entrypoint.sh` psql loop tolerates a gap `[UNVERIFIED]`, otherwise swap numbers before first apply. |
| session-core migrations | 0044, 0045 (existing, `[U]` today); 0046 WS10.F; 0047 WS11.H | 0044/0045 must be committed first (PRE-1). |
| `browser.proto` fields | 1-12 existing messages' fields; 10-19 reserved for WS1 (it uses 10-12 plus the `DeriveRunGrant` messages 1-7); 20 WS7.K `WebPolicy`; 21-39 reserved F-t2-032; 40-44 WS6.E; 50-53 used by WS8.C, 50-59 reserved | D7's allocation sentence "D1 uses 1-12 (10-19 new)" and D1's "10-12" are consistent. D8 asks the integrating architect to confirm 50-59: confirmed here. Event ids in `events.proto`: 14-18 and 150-159 WS10.B; 160+ others. |
| Quarry error codes | `USER_TOOK_OVER` WS2.C, `Timeout.details` WS3.D, `RateLimited.details.reason` WS2.D and WS3.F (reasons `org_run_cap`, `org_interactive_run_cap`, `global_run_cap`, `browser_session_cap`, `browser_page_cap`), `UNSUPPORTED_MEDIA` WS5.A, `BUDGET_EXHAUSTED` WS6.B, 429 reasons `org_plan_cap`, `org_render_cap`, `org_backlog_full` WS6.G/H | One 429 body shape (D3-4 and D2): `details {reason, scope, limit, active, retry_after_seconds}`; WS2.D publishes it first and WS3.F, WS6.G, WS6.H reuse it. |
| GetVersion ids | `crawl-bounded-v1` WS6.A; `crawl-scope-v1` WS14.D; `lifecycle-v1`, `batch-control-v1`, `monitor-preflight-v1` WS15 | unique, no collision |

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

### 4.5 Decisions that gate slices

Gated days are the designs' own. "Proceeds without" says what a lane can still do before the decision; "plan default" is the answer the plan assumes so that dependent slices can be written and tested.

| Decision | Slices gated (days) | Proceeds without it | Plan default if the owner does not rule |
|---|---|---|---|
| ADR topic 1: policy owner and claim carrier (fail mode of the entitlement read, minter, override owner) | WS7.A (0.5), WS7.H (6.5), WS7.I (7.5), WS7.J (3), WS7.K (6.5), WS14.H (6), WS7.M optional (4) | WS7.B to WS7.G (21.5 d, owner-neutral), WS6.D in `static` mode, WS11.C interim quota-row carrier | outcome C (claim in the token plus broker snapshot) with org-core as policy store, as D7 recommends |
| ADR topic 3: human plus agent sessions, renewal and idle semantics, kinds beyond `session`/`run` | WS8.C (8.5); WS9.K and WS9.L unscheduled; WS8.K optional (5) | WS8.A, WS8.B, WS8.E, WS8.F core on wiremock, WS9 omnibox, dock and evidence slices | D8-1 to D8-3 as written (absolute 4 h, idle semantics from D2's `Interactive` class) |
| ADR topic 4: ZDR per feature | WS4.H (2.5, observe-mode first), WS4.J (5, also legal review and QB-T17 commit); WS4.I (1.5) waits for the Model Plane Z-05a/Z-05b commit instead | WS4.A to WS4.G | D4 3.3 defaults: refuse durable resources, inline-only scrape, metadata-only tombstone, no hash-only |
| ADR 5: anti-bot live triggers | WS3.I (3) | WS3.E, WS3.H, WS3.J | default stays `legacy` until ruled |
| ADR 8 part 2: Application projection and Space reads of the trace | WS10.G (4.5) | WS10.A to WS10.F, WS10.H, WS10.I | no Application projection; owner-only trace read |
| ADR 10: execution substrate | WS3.G isolation claims (optional); epics F-t2-059/060 | WS3.F with K=1 | single shared Chromium, loopback-only egress proxy unchanged |
| DR-15.1: extraction grant route (adjacent to topics 1 and 4) | WS16.C (3), then WS16.G's E2 tier | WS16.B, WS16.D to WS16.F, WS16.I | option O4 with O1 as the single accountable owner (D15's recommendation: service lane in model-gateway, owner sibling b, Control co-signs the registry) |
| Key-management ownership and master-key custody (audit 9.3 item 11) | WS17.H (8), and `REQUIRED=1` of WS17.D | WS17.A to WS17.G, WS17.I with the interim env key provider | org-core owns keys (D16-3), env provider in dev only |
| Owner decision OD13 (planner route and default) and the stage-3 prerequisites | WS11.F stage 3 | WS11.A to WS11.F stages 1 and 2 | default intent route, no subscription pin, stage 3 not enabled |
| D14b decisions: D9 results source, D14 command transport, D15 ZDR for schedules, D17 notification principal | WS15.K (3.5, optional), WS15.H (4), WS15.I (5, partly) | WS15.A to WS15.G | D14b's own recommendations per decision; for D15, refuse creation for ZDR callers (the shared call site with WS4.H) plus the fire-time posture re-check, which needs the Control Plane to grant `org:retention:read:any` to a quarry-edge principal |
| Crawl hard ceiling: 5000 (D6) versus 10,000 (D14a) pages | WS6.A value only (not a gate on the PR) | everything | 5000, env-overridable (section 7.2 item 1) |
| Receipt text policy default (`sensitive` versus `all`) and legacy-row rewrite | WS12.D default, WS12.H (2, optional) | WS12.A to WS12.C | `sensitive` for non-human steps, D8's `metadata` for human steps |

Gated totals by wave: W1 12 days (WS3.I, WS4.H, WS4.I, WS4.J); W2 63 days (WS7 24, WS8.C 8.5, WS10.G 4.5, WS14.H 6, WS15.H 4, WS15.I 5, WS16.C 3, WS17.H 8).

## 5. EFFORT ROLL-UP

### 5.1 Assumptions

- Days are the designs' own engineer-days (section 4 of each design, section 10 where the design restates them). Where a design gives a range the midpoint is used (WS2.C 5.5, WS2.D 6.5, WS9.J 9, WS5.I 13.5, WS16.J 9). T-shirt sizes use S = 1-2 d, M = 3-5 d, L = 6-12 d as the brief says; the days, not the letters, drive every total.
- Waves follow the fixed numbering: W1 = WS1 to WS5, W2 = WS6 to WS17, with two moves that the dependency graph forces: WS6.A (crawl bound, 1.5 d) is pulled into W1 as a hotfix, and WS3.K (actionability, 8 d, F-t2-015) is counted in W2 because it needs WS3.C, D and F first.
- Columns: *core* = scheduled, unconditional; *gated* = blocked by a named ADR or decision (counted because the plan schedules it once the decision lands); *opt* = optional slices; *unsched* = design-level only, never included in a total. Sibling-owned days that sit outside the Quarry figure (WS6.B +1.5 Model Plane, WS6.E +2 Model Plane and +2 execution-core) are not in the totals; they add about 5.5 days to W2.
- Team and focus: engineer-weeks = engineer-days / 5. Calendar weeks assume 80% focus (review, CI waits, rebases). W1 team 6.5 engineers (L1 x2, L2, L3 x1.5, L4, L5), W2 sensitivity at 8, 12 and 16 engineers.
- Risk adjustment: the audit raised 33 efforts after verification, and several designs say the audit understated (D2 S3 from S to M, D11 from M to L, D14a step 1 from S to 10 d, D15 from M to L, D10 from L to XL by repo scale). W1 slices are small, file-level and mostly verified (x1.15 to x1.5). W2 adds cross-plane review, ADR waits, uncommitted-diff rebases and migration renumbering (x1.3 to x1.8).

### 5.2 Per wave

| Wave | core d | gated d | opt d | unsched d (excluded) | core+gated d | eng-weeks | risk-adjusted eng-weeks | calendar weeks at team size |
|---|---|---|---|---|---|---|---|---|
| W1 | 113.75 | 12 | 10 | 13.5 | 125.75 | 25.1 | 29 to 38 | 6.5 engineers: 5.6 to 7.3 weeks |
| W2 | 447 | 63 | 19.5 | 9 | 510 | 102.0 | 133 to 184 | 8 eng: 21 to 29 wk; 12 eng: 14 to 19 wk; 16 eng: 10 to 14 wk |
| W1+W2 | 560.75 | 75 | 29.5 | 22.5 | 635.75 | 127.2 | 162 to 221 | |

The critical path (section 4.4, 50 engineer-days, 13 to 18 weeks risk-adjusted) is a floor, not the estimate: W1 is throughput-bound at 6.5 engineers (about 6 to 8 weeks), W2 at 12 engineers is about 14 to 19 weeks and is where the critical path and the throughput estimate meet; beyond about 14 engineers the critical path binds and extra people do not shorten W2. End to end, W1 plus W2 at the stated team sizes is roughly 5 to 7 months, with W2 foundation slices starting during W1 as soon as their lane is free. Epics of W3 to W5 are outside every number here.

### 5.3 Per lane

Days are core+gated unless the column says otherwise; the risk range applies the wave multipliers to each wave's days and converts to engineer-weeks.

| Lane | W1 core d | W1 gated d | W2 core d | W2 gated d | opt d | unsched d | total eng-weeks (core+gated) | risk-adjusted eng-weeks | engineers it needs (target: W2 done in about 16 weeks) |
|---|---|---|---|---|---|---|---|---|---|
| L1 GRANTS-BROKER | 26 | 0 | 5 | 15 | 4 | 0 | 9.2 | 11 to 15 | 1.5 (Go broker + model-gateway/registry) |
| L2 ADMISSION | 16 | 0 | 14 | 0 | 1 | 0 | 6.0 | 7 to 10 | 1 |
| L3 DRIVER | 22 | 3 | 34 | 0 | 5 | 0 | 11.8 | 15 to 20 | 1.5 |
| L4 ZDR-TENANT | 17.75 | 9 | 0 | 0 | 0 | 0 | 5.3 | 6 to 8 | 1 (W1 only; tail gated) |
| L5 SCRAPE | 24 | 0 | 44 | 0 | 4 | 22.5 | 13.6 | 17 to 23 | 1.5 |
| L6 SPA-GATEWAY | 6.5 | 0 | 60 | 0 | 2 | 0 | 13.3 | 17 to 24 | 2 |
| L7 CRAWL-JOBS | 1.5 | 0 | 72.5 | 15 | 3.5 | 0 | 17.8 | 23 to 32 | 2 |
| L8 PLANNER-MP | 0 | 0 | 69 | 0 | 0 | 0 | 13.8 | 18 to 25 | 2 |
| L9 CONTROL-PLANE | 0 | 0 | 0 | 25 | 0 | 0 | 5.0 | 6 to 9 | 1 (shares a Control Plane engineer with L15 billing-core) |
| L10 SHARED-BROWSER-QUARRY | 0 | 0 | 30.5 | 0 | 5 | 0 | 6.1 | 8 to 11 | 1 |
| L11 PROFILES | 0 | 0 | 37.5 | 0 | 3 | 0 | 7.5 | 10 to 14 | 1 |
| L12 TRACE | 0 | 0 | 33 | 4.5 | 0 | 0 | 7.5 | 10 to 14 | 1 |
| L13 WEB-POLICY-QUARRY | 0 | 0 | 17 | 3.5 | 0 | 0 | 4.1 | 5 to 7 | 0.5 (pairs with L14) |
| L14 REDACTION | 0 | 0 | 13 | 0 | 2 | 0 | 2.6 | 3 to 5 | 0.5 (pairs with L13) |
| L15 CAPACITY-METERING | 0 | 0 | 17.5 | 0 | 0 | 0 | 3.5 | 5 to 6 | 1 |

Lanes whose serial sum exceeds the roughly 65 working days of a 13-week W2 need a second engineer or a split: L7 (crawl branch versus jobs branch), L8 (planner versus trace persistence), L3 (WS12.B/F, WS13.C/G, WS8.E share the driver with WS3), L5 (WS5/WS13 versus WS16), L6 (SPA versus gateway halves). Staffing totals of the right-hand column are about 17.5 engineers for the W2 lanes if every lane ran flat out for the whole wave; at 12 engineers L13/L14 share one person, L9 and L15 share one, and L4 and L2 hand their W1 people to L10 and L12 once W1 closes.

### 5.4 Per workstream (cross-check against the designs)

| WS | Title | core d | gated d | opt d | unsched d | Design stated | Note |
|---|---|---|---|---|---|---|---|
| WS1 | Grants and delegated bearer | 26 | 0 | 0 | 0 | about 26 (16-18 working days with two engineers) |  |
| WS2 | Quarry run control | 16.5 | 0 | 1 | 0 | 16-18 |  |
| WS3 | Browser driver stability and actionability | 36 | 3 | 5 | 0 | about 44 | includes WS3.K (W2-timed) and sibling days of WS3.J |
| WS4 | ZDR and tenant boundary | 17.75 | 9 | 0 | 0 | about 27 |  |
| WS5 | Scrape contract and file routing | 24 | 0 | 4 | 13.5 | 24 (28 with the optional sandbox) | WS5.I (13.5 d) is unscheduled |
| WS6 | Budgets, capacity, metering | 42.5 | 0 | 0 | 0 | about 47 (includes sibling days) | plan total excludes about 5.5 sibling days |
| WS7 | Web policy / ResolvedPolicy | 24.5 | 24 | 4 | 0 | about 52 (21.5 owner-neutral + 31) |  |
| WS8 | Shared browser backend | 54.5 | 8.5 | 5 | 0 | 63 to 68 |  |
| WS9 | Shared browser frontend | 47 | 0 | 0 | 0 | about 47 scheduled |  |
| WS10 | Run trace envelope | 43 | 4.5 | 0 | 0 | about 47.5 |  |
| WS11 | LLM planner default-on and memory guard | 29.5 | 0 | 0 | 0 | F-t2-010 about 26 + F-t2-045 3.5 |  |
| WS12 | Secret redaction | 27.5 | 0 | 2 | 0 | about 20.5 text + 7 screenshots + 2 optional |  |
| WS13 | Scrape-time actions, screenshot and PDF | 33 | 0 | 2 | 0 | about 33 (+2 optional) |  |
| WS14 | Crawl scope and robots | 30.5 | 6 | 0 | 0 | about 34.5 | plan figure includes the +2 Control Plane days of WS14.H |
| WS15 | Job lifecycle and schedules | 33 | 9 | 3.5 | 0 | about 46 |  |
| WS16 | Structured extraction | 38 | 3 | 0 | 9 | 41 without S9 | WS16.J unscheduled |
| WS17 | Profile hardening | 37.5 | 8 | 3 | 0 | about 46 |  |

The epics of W3, W4 and W5 (48 items, one parked) carry their own estimates in the epic documents another agent is writing; nothing here includes them. Unscheduled design-level work listed in section 2.1 adds about 22.5 days to W1/W2 if it is pulled in, plus the D6 L-series (about 26 days) and F-t2-024 E1 to E4 (20 to 30 days).

## 6. FIRST THREE PULL REQUESTS

Selection rule: risk removed per engineer-day, then file independence (the three must merge in parallel and in any order), then whether the PR unblocks the critical path. Candidates scored (effort from the designs; risk statements from the audit and the verified designs):

| Candidate | Slice | Effort | What is closed | Files it owns | Verdict |
|---|---|---|---|---|---|
| Gateway search-cache hotfix | WS4.A (plus WS4.B evidence) | 3 d | Cross-tenant reads of cached search results and caching under ZDR: the `search_web` cache key has no org and no ZDR check (confirmed by code path, not exercised); scrape-preview keys lack org and request variant. Exposure exists only where `GATEWAY_CACHE_REDIS_URL` is set (the stock compose does not set it; whether any deployment sets it is `[UNVERIFIED]`) | gateway only | **PR 1**: only candidate that closes a data-exposure hole, gateway-only files; deployment-conditional, so its urgency depends on the WS4.B forensics |
| Crawl bound | WS6.A | 1.5 d | `max_pages: 0` means unlimited and failed pages and depth are unbounded, so one request can run to the 30-minute workflow timeout | orchestrator Go, new `limits.rs`, one gateway function | **PR 2**: cheapest cost and abuse control; gates the F-t2-018 schedules |
| Evaluate default-deny | WS2.A | 1.5 d | Raw page script reachable through REST step, interact, WebSocket and the executor, bypassing the typed action policy | `agent_routes.rs` plus three runtime files | **PR 3**: also the entry condition of the module split (WS2.B), so it unblocks the critical path |
| Chromium sandbox default | WS3.A | 2 d | `--no-sandbox` defaults on in both compose files; the production overlay has never been shown to launch Chromium | compose files, 2 lines in `chromiumoxide.rs` | PR 4 candidate; may fail on WSL2 or Docker Desktop and needs a seccomp iteration, so risk of its own |
| Cross-tenant schedule exposure | WS15.C | 4.5 d | schedule list and control are not tenant-scoped (D14b finding) | quarry-control, edge | PR 5 candidate; larger, shares `store.go` with the lifecycle work |
| Fail-open parse fixes | PRE-2 (F-t2-074 items) | about 1 d | `QUARRY_EDGE_AUTH_DEV_BYPASS=True` and `ARTIFACT_BACKEND=S3` slip past the production refusals (S9 H3, H4) | `main.rs` [M] only, two lines | PR 4 or 5 candidate; blocked only by the uncommitted `main.rs` hunks (PRE-1) |
| Content-kind sniff | WS5.A | 2 d | PDFs and Office files parsed as HTML | transform/runtime | later; correctness not exposure |

File independence of the chosen three: PR 1 touches only `apps/Frontend Plane/verevonv3/apps/gateway`. PR 2 touches the orchestrator, `quarry-edge` (`limits.rs` NEW, `lib.rs`, `main.rs` mod line, `routes.rs` crawl region, `handoff.rs`) and one gateway function (`start_crawl` in `knowledge/quarry.rs`, a different function from PR 1's `scrape_preview`; textual adjacency only). PR 3 touches `agent_routes.rs` and three runtime files; the only shared file with PR 2 is `Q/docs/openapi.yaml` (different regions: `CrawlRequest` near 4361 versus `AgentAction` near 2787 and 3042, 263-345; the owner's uncommitted hunks sit at 238-250 and 4012-4150, so stage hunks by hand). Branch each from the same base and merge in the order 1, 2, 3.

### PR 1. Gateway result cache: org scope, ZDR bypass, ZDR forwarded to Quarry (WS4.A, F-t2-003 phase 1)

- Branch: `fix/gateway-result-cache-org-scope-zdr`
- Commit title: `fix(gateway): scope the result cache by org and bypass it under ZDR`
- Before it deploys (ops, not a PR): run WS4.B steps 0-2 (read-only forensics: key counts, scan for cross-org hits) so the evidence of the old state exists; the purge (WS4.B steps 3-5) runs only after every gateway replica is on the new image. Scripts land in a second small PR `chore(gateway): cache forensics and purge scripts`.
- Files (all under `apps/Frontend Plane/verevonv3/apps/gateway/`): `src/cache_admission.rs` [NEW]; `src/cache.rs` [C] (backend enum `Disabled|Redis|Memory`, scoped lookup/store, `GATEWAY_RESULT_CACHE_CONTENT` read once); `src/zdr.rs` [C] (`OrgPosture` tri-state, `org_zdr_enabled` = not `NotZdr`); `src/domains/search.rs` [C] (`search_web` 51-293, `search_answer_stream` 589-622: admission, `zdr:true` in the `/v1/search`, `/v1/scrape` and `/v1/answer/stream` payloads, drop the `target` log field at 154-158); `src/domains/knowledge/quarry.rs` [C] (`scrape_preview` 62-203 keyed by org plus `variant_digest`; the private `CacheAdmission` 212-282 removed; logs stop printing `target`); `src/observability.rs` [C] (`record_result_cache`); `src/main.rs` [C] (one `mod cache_admission;` line); `tests/cache_admission_only.rs` [NEW] (source scan: `cache.lookup(`/`cache.store(` forbidden under `src/domains/**`).
- Tests: `scoped_keys_differ_by_org_and_variant`, `an_org_id_with_separator_or_glob_characters_is_not_a_scope`, `a_scope_tag_mismatch_is_a_miss_not_a_hit`, `org_lookup_failure_denies_both_directions`, `no_authorized_org_denies`, `request_shape_with_headers_actions_profile_or_ingest_denies`; handler tests `search_web_isolates_cached_results_by_org` (the cross-tenant test), `search_web_same_org_second_request_is_a_cache_hit`, `a_zdr_org_search_leaves_no_cache_entry_and_tells_quarry`, `explicit_zdr_header_or_body_skips_cache_for_a_non_zdr_org`, `org_core_outage_skips_the_cache_but_not_the_search`, `stale_entry_is_not_served_across_orgs_when_quarry_fails`, `stale_entry_is_served_to_the_same_org_when_quarry_fails`; `scrape_preview_key_differs_by_org`, `scrape_preview_key_differs_by_render_variant`; the six moved `CacheAdmission` tests; scan test `content_cache_is_only_reached_through_admission`.
- Commands: `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` (CI parity, `verevonv3-ci.yml` job Gateway), then the CLAUDE.md form `cd "apps/Frontend Plane/verevonv3" && cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (CI omits `--all-targets`; run both); targeted: `cargo test cache_admission`, `search_web`, `zdr`, `--test cache_admission_only`.
- Evidence (the repo counts a deployment claim only with rebuilt images, a running stack and a verified flow): rebuild the gateway image (production overlay needs `SOURCE_REVISION` and `BUILD_DATE`; `GET /version` shows the new revision) with `GATEWAY_CACHE_REDIS_URL` pointing at a scratch Redis (the stock compose has no cache, so the hole exists only where it is set). (1) As org A, `POST /api/v1/search/web {"query":"d4-probe-<uuid>","includeAnswer":true}` twice: one `search.upstream` in Quarry's log; `redis-cli --scan --pattern 'verevon:gw:search-web:o:<orgA>:*'` returns one key. (2) Same query as org B: a second `search.upstream`, one key under `o:<orgB>:*`, the two keys differ. (3) Set org B's `metadata.interactiveRetention.zdr=true`, wait 61 s (posture memo), repeat twice: two more upstream events, no new key under org B, `gateway_result_cache_total{ns="search-web",outcome="bypass_zdr"} 2` on `/metrics`, Quarry's `search.result_shape` shows `cache_hit=false`. (4) Stop org-core, repeat as org A: `bypass_zdr_unknown` increments, no key changes. Record the pre-fix behaviour of (2) from the WS4.B forensics.
- Rollback caution: reverting the image re-opens the hole. If a rollback is needed, roll back with `GATEWAY_CACHE_REDIS_URL` unset, never set. Emergency lever without rollback: `GATEWAY_RESULT_CACHE_CONTENT=off`.

### PR 2. Crawl bounds: finite page, depth and failure limits with the effective value returned (WS6.A)

- Branch: `fix/crawl-bounded-defaults`
- Commit title: `fix(crawl): bound max_pages, depth and failures and return the effective limits`
- Files (under `apps/Ingestion Plane/Quarry-v2/` unless noted): `services/quarry-orchestrator/internal/jobs/limits.go` [NEW] (`CrawlLimits`, `LoadCrawlLimits`, `Resolve`); `.../internal/jobs/dispatcher.go` [C] (`crawlInputFromJob` 432-454); `.../cmd/orchestrator/main.go` [C] (wire limits into `jobs.Config`); `.../internal/workflows/workflows.go` [C] (`CrawlJobWF` 396-512 and `crawlJobDurableFrontier` 518-610, lines 456, 474, 555, 574, behind `workflow.GetVersion(ctx, "crawl-bounded-v1", ...)`, fallback constants 1000 pages and depth 5, `max_failures` terminal reason); `crates/quarry-edge/src/limits.rs` [NEW] (`CrawlLimits::from_env`, `resolve`, boot-fails on unparsable env); `crates/quarry-edge/src/lib.rs` [M] and `main.rs` [M] (one `mod limits;` line each, away from D2's `run_limits` line); `crates/quarry-edge/src/routes.rs` [M] (`CrawlRequest` 533-553, `crawl_handoff` 590-667, `HandoffAck` 727-738); `crates/quarry-edge/src/handoff.rs` [C] (attach `limits` to the ack); gateway `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/knowledge/quarry.rs` [C] (`start_crawl` 306-376 copies `limits` from the ack); `docs/openapi.yaml` [M] (`CrawlRequest` near 4361-4374) and `docs/CONTRACTS.md` [C]. Tests: `.../workflows/workflows_test.go` [C], `.../jobs/limits_test.go` [NEW], `crates/quarry-edge/tests/crawl_limits.rs` [NEW].
- Names: env `QUARRY_CRAWL_DEFAULT_MAX_PAGES` 1000, `QUARRY_CRAWL_HARD_MAX_PAGES` 5000, `QUARRY_CRAWL_DEFAULT_MAX_DEPTH` 5, `QUARRY_CRAWL_HARD_MAX_DEPTH` 10 (same names on both sides, one compose anchor; `0` for a default disables the default, never the hard ceiling). Plan decision recorded in section 7: these replace D14a's `QUARRY_EDGE__CRAWL_DEFAULT_MAX_PAGES` and `CRAWL_MAX_PAGES_CAP` (10,000); the hard ceiling differs (5000 versus 10,000) and needs a product-owner ruling, the plan uses 5000 with env override.
- Tests: Go workflow `TestCrawlJobWF_ZeroMaxPagesFallsBackToCap`, `TestCrawlJobWF_FailuresCountTowardCap`, `TestCrawlJobWF_ZeroMaxDepthFallsBackToDepthCap`, `TestCrawlJobWF_OldVersionHistoryKeepsLegacyBehaviour` (`[UNVERIFIED]` whether the pinned Temporal SDK exposes the `OnGetVersion` hook; otherwise a `WorkflowReplayer` on a recorded history); Go jobs `TestResolve_ZeroUsesDefault`, `TestResolve_ClampsToHardAndReports`, `TestResolve_DefaultZeroDisablesDefaultNotHard`; Rust `zero_is_absent_and_takes_default`, `above_hard_is_clamped_and_reported`, `depth_resolves_like_pages`, `from_env_rejects_unparsable_naming_the_variable`; integration `crawl_limits` (axum stub as control; a ZDR caller is still refused at `routes.rs:555-588`); gateway `start_crawl_exposes_upstream_limits`.
- Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./...`; `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge crawl_limits limits` (Windows host: inside the `rust:1` container with `quarry-cargo`/`quarry-target` volumes, `bash -c`); `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" quarry`.
- Evidence: rebuild edge and orchestrator (`make up` with the deploy-recipe env chain; override the macOS `CARGO_TARGET_DIR`), test-only `QUARRY_CRAWL_DEFAULT_MAX_PAGES=7` in the last env file. (1) Through the gateway `POST /api/v1/knowledge/crawl` with only `url` of a public site with more than 7 internal links: `limits.max_pages.effective == 7`, `clamped_by == ["default"]`. (2) Direct to the edge (`127.0.0.1:8082`) with `"max_pages": 0`: identical. (3) The terminal `run.completed` event shows `pages_visited == 7`. (4) `GET /v1/jobs/{id}` in control shows `params.max_pages == 7`. (5) Pre-fix capture: repeat (1) on the old image and cancel manually after the page count passes 7. The off-site walk (scope) is not closed by this PR; it closes with WS14.D under `observe` then `enforce`.

### PR 3. Deny raw page script on every agent entry point (WS2.A, F-t2-002)

- Branch: `fix/quarry-deny-raw-script-evaluate`
- Commit title: `fix(quarry): deny raw page script (evaluate) on every agent entry point`
- Files (under `apps/Ingestion Plane/Quarry-v2/`): `crates/quarry-edge/src/agent_routes.rs` [C] (new `deny_raw_script`, called in `step` after the capability check at 2269-2270 and in `execute_ws_action` after 3102, before `sensitive_approval_for_action`, the one-shot insert and the budget check); `crates/quarry-runtime/src/observation.rs` [C] (arm at 542-544 returns the typed error); `crates/quarry-runtime/src/action_runtime.rs` [C] (arm 380-392, the 12-action test becomes 11 plus a refusal test); `crates/quarry-runtime/src/mp_client.rs` [C] (remove the `pdf` and `evaluate` example lines of `build_prompt` 382-383, adjust the two prompt tests); `docs/openapi.yaml` [M] (`EvaluateAction` deprecated, 403 text); `sdks/typescript/src/models/*` and `sdks/python/quarry_client/models/*` [C] (regenerate with `sdks/generate.sh`, commit only if the diff is the deprecation text); new `crates/quarry-edge/tests/agent_evaluate_denied.rs` [NEW]. Follow-up PR, owner sibling (b): remove `AgentAction::Evaluate { script }` at `apps/Model Plane/rust/services/execution-core/src/quarry_agent.rs:161` [C].
- Contract: 403 `FORBIDDEN`, `details {reason:"raw_script_denied", denied_action:"evaluate"}`, non-retryable; the script text is never logged; the denial consumes no budget and writes no receipt. No proto, no broker, no migration, no flag.
- Tests: `deny_raw_script_refuses_evaluate_only`, `deny_raw_script_error_is_typed_and_never_echoes_the_script`, `evaluate_action_is_refused_without_calling_the_driver`, `evaluate_action_is_refused` (action_runtime), integration `agent_evaluate_denied` (REST step, interact and the WebSocket seam: 403, `ctx.step` unchanged, zero receipts in `InMemoryStepReceiptStore`, zero driver calls); regression `sanitize_action_denies_raw_script_evaluation` (`GW/browser.rs:5001`) stays green.
- Commands: `cargo test -p quarry-runtime observation action_runtime mp_client`; `cargo test -p quarry-edge agent_evaluate_denied agent_routes`; `cargo test -p quarry-core contracts`; CI parity `cargo check --workspace && cargo test --workspace` (container on Windows).
- Evidence: rebuild the edge image; with a user-principal Quarry token obtained the way the gateway does (`get_audience_token(..., "quarry")`; record the mint command in the PR `[UNVERIFIED]`): `POST /v1/agent/runs`, `POST .../step {"action":{"type":"navigate","url":"https://example.com"}}`, then `POST .../step {"action":{"type":"evaluate","script":"document.title='pwned'"}}`. Assert HTTP 403 with `details.reason == "raw_script_denied"`; a following `get_content` step returns `Example Domain`; `GET .../receipts` lists only the navigate and get_content receipts; `docker logs quarry-edge` shows the warn line with no script text; repeat over `frames/ws` and show the `error` frame. Before merging, search seven days of edge logs and Model Plane `browser.act` traffic for `"type":"evaluate"` bodies `[UNVERIFIED: needs log access]`; a hit means an external SDK user who now gets a clear 403.
- Why it is third and not first: it carries no data exposure and has no production caller, but it is the entry condition of WS2.B (the `agent_routes.rs` split) and therefore the head of the critical path through WS2.C, WS2.D and every later `agent_routes` edit.

### Next candidates once the first three are in review (all file-independent of them)

PR 4 `WS3.A` sandbox default and seccomp profile; PR 5 `PRE-2` fail-open parse fixes (after PRE-1 commits `main.rs`); PR 6 `WS15.C` tenant-safe schedules; PR 7 `WS1.A` broker stream (Go, proto, registry row, `browser-broker-ci.yml`, `buf.yml` fix), which starts the longest chain.

## 7. ROLLOUT AND FLAGS

### 7.1 Flag and env-var register

Owner = the slice that introduces it. Defaults are the designs' committed defaults. "Removal" says when the switch can be deleted; "keep" means an emergency lever that stays. Tuning numbers are grouped in one row per slice; the design section 4 of the owning slice has the exact numbers and rationale. Service prefixes: edge vars are `QUARRY_EDGE__*` (EdgeConfig, prefix `QUARRY_EDGE`, separator `__`); compose anchors without the prefix (for example `USAGE_BROWSER_METRICS`) map onto them. An unparsable value aborts boot in the new readers (the old `EdgeConfig` loader silently falls back to defaults, S9 `config.rs:334-341`: do not copy that pattern).

| Flag / env var | Default | Owner | Removal condition |
|---|---|---|---|
| `GATEWAY_RESULT_CACHE_CONTENT` | `on` (`off` disables the two content caches only) | WS4.A | keep (emergency lever) |
| `QUARRY_ZDR_LOG_FILTER` | on | WS4.E | keep until WS10.H span processor ships, then fold into it |
| `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` | observe first, then on | WS4.H (WS17.B reads it) | after one release at on and customer notice; keep as rollback until then |
| `QUARRY_EDGE__ZDR_TOMBSTONE`, `QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS`, `QUARRY_ZDR_TOMBSTONE_DRY_RUN` | off | WS4.J | after contract and legal review, enable per environment; keep the retention knob |
| `QUARRY_UPDATE_GOLDEN` (test regeneration switch) | unset | WS4.G | keep (test tool) |
| `QUARRY_CRAWL_DEFAULT_MAX_PAGES`, `QUARRY_CRAWL_HARD_MAX_PAGES`, `QUARRY_CRAWL_DEFAULT_MAX_DEPTH`, `QUARRY_CRAWL_HARD_MAX_DEPTH` | 1000, 5000, 5, 10 (`0` on a default disables the default, never the hard ceiling) | WS6.A | keep; WS6.D `LIMITS_SOURCE=billing_core` later supplies plan values above these floors |
| `QUARRY_EDGE__TAKEOVER_ENFORCEMENT` | `1` (`0` = log-only `would_reject`) | WS2.C | after two releases with no `would_reject` noise |
| `QUARRY_EDGE__RUN_SWEEPER`, `RUN_SWEEPER_DRY_RUN`, `RUN_SWEEP_INTERVAL_S` | `1`, `0`, `15` | WS2.D | first deploy with dry-run `1` for 24 h; sweeper switch kept |
| `QUARRY_EDGE__AGENT_RUN_IDLE_TTL_S`, `AGENT_RUN_MAX_TTL_S`, `AGENT_RUN_TTL_GRACE_S`, `INTERACTIVE_RUN_IDLE_TTL_S`, `INTERACTIVE_RUN_MAX_TTL_S`, `TAKEOVER_IDLE_TTL_S` | 300, 1800 (cap 3600), 30, 900, 14400, 1800 | WS2.D | keep; WS8 may change the interactive numbers (open question 10.2.2) |
| `QUARRY_EDGE__MAX_CONCURRENT_RUNS`, `MAX_CONCURRENT_RUNS_PER_ORG`, `MAX_INTERACTIVE_RUNS_PER_ORG`, `RUN_CAP_RETRY_AFTER_S` | 32, 8, 6, 10 (`0` = unlimited); placeholders until RSS is measured at 8/16/32 contexts | WS2.D | keep; WS6.G adds plan-aware values on top |
| `VITE_FLEET_CONSOLE` | off (route hidden) | WS2.E | when a fleet producer exists |
| `QUARRY_BROWSER_NO_SANDBOX` | `0` (refused when `ENVIRONMENT` is prod) | WS3.A | keep as the explicit local opt-out |
| `QUARRY_BROWSER_DEFAULT_VIEWPORT`, `QUARRY_BROWSER_PAGE_HYGIENE` | `1280x800`, on | WS3.C | hygiene switch removed after one release |
| `QUARRY_BROWSER_DEADLINE_SCALE` | `1.0` (range (0,2]) | WS3.D | keep (calibration) |
| `QUARRY_BROWSER_MAX_SESSIONS`, `MAX_PAGES_PER_SLOT`, `SESSION_IDLE_S`, `SESSION_MAX_AGE_S`, `REAPER` (`0\|log\|1`), `REAPER_INTERVAL_S`, `PROBE_INTERVAL_S` | 12, 24, 3600, 21600, log for one week then on, 30, 15 | WS3.F | `REAPER=log` phase ends after one week; caps kept |
| `QUARRY_BROWSER_POOL_SIZE`, `QUARRY_BROWSER_RETIRE_AFTER_PAGES` | 1, 100 | WS3.G | keep; raise only after the memory budget is measured |
| `QUARRY_CHALLENGE_CLASSIFIER` (`v2\|legacy`), `QUARRY_CHALLENGE_ENFORCE` (`log` while validating), `QUARRY_BLOCK_ESCALATION` (`legacy` until ADR 5, then `honest`) | `v2`, enforce, `legacy` | WS3.H, WS3.I | `legacy` branches removed one release after `honest` is default |
| `QUARRY_BROWSER_TRUSTED_INPUT` | `off`, then `on` after the live lane is green two weeks | WS3.K | removed after GA for one release |
| `QUARRY_EDGE__DOC_ROUTING` (`off\|shadow\|skip\|on`), `DOC_PARSE_TIMEOUT_MS`, `DOC_PARSE_CONCURRENCY`, `DOC_PARSE_ISOLATION` (`process\|inprocess`) | `shadow`; isolation required for `on` in multi-tenant production | WS5.B, WS5.C | after `on` has run a release; isolation switch kept |
| `QUARRY_EDGE__SCRAPE_OPTIONS_ENABLED`, `SCRAPE_REQUEST_VALIDATION` (`lenient\|strict`), `SCRAPE_STRICT_AFTER`, `SCRAPE_INLINE_MAX_BYTES`, `SCRAPE_RECORDS` | `1`, `lenient`, unset, 1 MiB (ceiling 8 MiB), on | WS5.D, WS5.F | strict becomes the only mode after the date and a week of clean logs; others kept |
| `QUARRY_EDGE__BUDGET_ENFORCEMENT` (`off\|shadow\|on`), `BUDGET_ATTEMPT_FACTOR`, `BUDGET_BROWSER_SECOND_USD` | `shadow` first, then `on`; 2; 0.00003 (must be set from the billing-core price table before `on`) | WS6.B | `shadow` phase 24 h minimum; keep `off` as rollback |
| `QUARRY_EDGE__USAGE_BROWSER_METRICS` (compose anchor `USAGE_BROWSER_METRICS`), `QUARRY_EDGE__USAGE_LLM_METRICS` (anchor `USAGE_LLM_METRICS`) | `off`, then `shadow` a week, then `on` when Lago codes exist | WS6.C, WS16.G | the corrected existing-metric behaviour ships with `on`; keep the switch until invoices reconcile |
| `QUARRY_EDGE__LIMITS_SOURCE` (`static\|billing_core`), `LIMITS_FAIL_MODE` (`floor\|open\|closed`), `LIMITS_CACHE_TTL_S`, `LIMITS_STALE_MAX_S`, `LIMITS_BILLING_ORGS`, `QUARRY_EDGE__BILLING_CORE_URL`, `BILLING_CORE_SERVICE_TOKEN` | `static`, `floor`, 30, 900, canary list | WS6.D | canary list removed when `billing_core` is default; fail mode needs ADR topic 1 |
| `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM` (`off\|on`), `QUARRY_EDGE__GRANT_BUDGET_CLAIM` (`ignore\|enforce`) | `off`, `ignore` | WS6.E | `enforce` only after the broker deploys; removed after one release |
| `GATEWAY_RATE_CLASSES`, `GATEWAY_RATE_LIMIT_RPM_<CLASS>` (for example `_BROWSER_START`) | `on`; class numbers per design | WS6.F | keep |
| `QUARRY_EDGE__POOL_ORG_MAX_PERMITS` | `max(1, pool/2)` (`0` disables) | WS6.G | keep |
| `QUARRY_JOB_ADMISSION` (`off\|on`), `QUARRY_JOB_CAP_DEFAULT`, `QUARRY_JOB_BACKLOG_DEFAULT`, `QUARRY_JOB_BACKLOG_TTL`, `QUARRY_JOB_CLAIM_LEASE`, `QUARRY_JOB_CLAIM_BATCH` | `off`; 3, 200, 24h, 60s, 50 | WS6.H | `off` path removed after one release at `on` |
| `QUARRY_EDGE__METRICS_ENABLED`, `METRICS_ADDR`, `METRICS_TOKEN`, `DRAIN_DEADLINE_S` | on, `0.0.0.0:9092` (not published), required token, 30 (below the compose grace) | WS6.I, WS6.J | keep |
| `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` (`off\|observe\|enforce`), `WEB_POLICY_BASELINE_JSON`, `WEB_POLICY_REQUIRE_CLAIM`, `WEB_POLICY_FINAL_URL_CHECK`, `RUN_POLICY` | `enforce` (baseline equals today's behaviour); `observe` for any non-default baseline | WS7.C, WS7.G, WS7.J | `off` removed after claim-fed `enforce` is stable |
| `WEB_POLICY_CLAIM_ENABLED` (auth-core) | `1` once WS7.I is deployed | WS7.I | keep (Control disable lever) |
| `BROWSER_GRANT_ENFORCEMENT` (gateway, `off\|permissive\|strict`) | `permissive` outside prod, `strict` in prod | WS1.B | `off` removed once every stack has a broker |
| broker: `DATABASE_URL`, `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT`, `BROWSER_BROKER_GRANT_TTL_SECONDS` (900, clamp 30-3600), `BROWSER_BROKER_MAX_SESSION_DOMAINS` (8), `BROWSER_BROKER_MAX_RUN_DOMAINS` (8), `BROWSER_BROKER_MAX_ACTIVE_GRANTS_PER_OWNER` (20) | as stated | WS1.A, WS1.C | ephemeral flag dev only; production refuses to start without a database |
| `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` | empty in dev (Noop), set in prod and the contract overlay | WS1.A | keep |
| `QUARRY_EDGE__HUMAN_BROWSE` (edge), `BROWSER_BROKER_HUMAN_BROWSE_ENABLED` (broker), `BROWSER_HUMAN_BROWSE` (gateway), `QUARRY_EDGE__HUMAN_RECEIPT_MODE` (`metadata\|full\|none`), `GRANT_VALIDATION_CACHE_MS` (0), `BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS` | edge and broker switches `1` in dev compose and `0` in the production overlay; gateway default not stated in the design (instant rollback is `off`); receipt mode `metadata` | WS8.B, WS8.C, WS8.D | three switches removed together at GA; receipt mode kept |
| `QUARRY_EDGE__ATTACH` (edge), `BROWSER_ATTACH` (gateway), `QUARRY_EDGE__ATTACH_IDLE_TTL_S`, `ATTACH_REQUIRE_DELEGATION` (`0`), `BROWSER_BROKER_MAX_ATTACH_DOMAINS` | off until WS8.H and the evidence; enable edge and gateway together | WS8.F, WS8.I, WS8.K | removed at GA (after WS8.H) |
| `VITE_BROWSER_OMNIBOX_SEARCH`, `VITE_CHAT_BROWSER_DOCK` | `1` in dev and staging builds, `0` in production builds until verified | WS9.A, WS9.G | after WS4.A deployed (omnibox) and after WS8 for the interactive dock; then deleted |
| `QUARRY_EDGE__REQUEST_ID_ADOPT` | on (`0` safety valve) | WS10.A | after one release |
| `QUARRY_EDGE__TRACE_RING_EVENTS`, `TRACE_RING_MAX_BYTES`, `QUARRY_EDGE__NATS_AGENT_EVENTS` (restores raw Ingestion-bus payloads), `QUARRY_EDGE__MODEL_TRACE_NATS_{URL,USER,PASSWORD,QUEUE,SUBJECT_PREFIX}`, `QUARRY_TRACE_PRODUCER` | 512, 1 MiB; `0`; publishing enabled by presence of the URL | WS10.C, WS10.D, WS10.E | kill switches kept; `NATS_AGENT_EVENTS` removed after consumers are confirmed |
| `SESSION_CORE_TRACE_CONSUMER` (`0` stops), `SESSION_CORE_TRACE_RETENTION_DAYS` | `1` once shadow mode is healthy; 30 | WS10.F | keep |
| `QUARRY_TELEMETRY_EXPORT`, `QUARRY_TELEMETRY_ZDR_REGISTRY_CAP` | off (no exporter); 100,000 | WS10.H | export stays off until the contract golden and WS4.F lint are green |
| `QUARRY_BROWSER_AGENT_PLANNER_MODE` (`off\|allowlist\|org_opt_in\|default_on`), `QUARRY_BROWSER_AGENT_LLM_ORGS`, `QUARRY_BROWSER_AGENT_PROCEDURE` (`browse/1\|legacy/0`), `..._MAX_CONSECUTIVE_FAILURES`, `..._MAX_TOTAL_FAILURES`, `..._MAX_PLANNER_TOKENS`, `BROWSER_AGENT_MAX_TOKENS_PER_RUN`, `QUARRY_BROWSER_AGENT_DEFAULT_ON_ACK` | `off`; stage-3 guard needs `grants,budgets,redaction,evals` | WS11.B to WS11.F | legacy `QUARRY_BROWSER_AGENT_LLM` retired at stage 4 (one release at stage 3 without rollback) |
| org-core quota keys `browser_agent_llm_planner` (1 on, 2 off), `browser_agent_max_tokens_per_run` | unset | WS11.C | replaced by the WS7 claim carrier if ADR topic 1 chooses it |
| `QUARRY_EDGE__REDACTION_MODE` (compose anchor `QUARRY_REDACTION_MODE`), `QUARRY_BROWSER_FIELD_PROBE`, `QUARRY_EDGE__RECEIPT_TEXT_MODE` (`sensitive\|all`), `QUARRY_BROWSER_SCREENSHOT_MASK`, `QUARRY_BROWSER_SCREENSHOT_OOPIF` (`allow\|mask`), `EXECUTION_CORE_REQUIRE_QUARRY_REDACTION_STAMP` | `enforce` (`shadow`/`off` refuse to boot in production); `on`; `sensitive`; `on`; `allow`; `0` then `1` after WS12.C is deployed | WS12.B to WS12.F | shadow phase 3 to 5 days in staging; no production switch to turn masking off by design |
| `QUARRY_EDGE__SCRAPE_CAPTURE` (`off\|on`), `SCRAPE_ACTIONS` (`off\|user\|on`), `SCRAPE_ACTIONS_MAX`, `SCRAPE_CAPTURE_INLINE_MAX_BYTES` (6 MiB), `SCRAPE_STITCH` (`off\|on`), `QUARRY_SCRAPE_STITCH_METHOD`, `QUARRY_SCRAPE_SESSION_ISOLATION` (on), `QUARRY_SCRAPE_HEAVY_PERMITS` (1) | capture `off` in committed compose, `on` in dev first; actions `off` committed, `user` in dev, `on` only after ADR topic 6; stitch `off` | WS13.A to WS13.G | actions `on` removes the committed `off`; isolation kept |
| `QUARRY_EDGE__CRAWL_SCOPE_MODE` (`off\|observe\|enforce`), `CRAWL_ROBOTS` (`off\|observe\|enforce`), `ROBOTS_UNREACHABLE` (`disallow\|allow`), `CRAWL_REQUEST_VALIDATION` (`lenient\|strict`), `CRAWL_BATCH_MAX_URLS`, `QUARRY_CRAWL_DELAY_MAX_S` (10, ceiling 60), `QUARRY_ROBOTS_CACHE_MAX_ENTRIES` (10,000), `QUARRY_ROBOTS_FETCH_TIMEOUT_S` (8) | `observe`, `observe`, `disallow`, `lenient` | WS14.D, WS14.E, WS14.G, WS14.I | WS14.I flips scope to `enforce` after the soak; `off` removed one release later |
| `QUARRY_SCHEDULED_JOBS_ENABLED` | `0` (must stay 0 until WS14 is deployed and WS6.A is live) | WS15.E | removed after the first week of live schedules |
| `QUARRY_EDGE__JOB_CONTROL`, `QUARRY_JOB_CONTROL_LOOP`, `QUARRY_CANCEL_GRACE_S` (120), `QUARRY_EDGE__JOB_STREAM_FOLLOW`, `JOB_STREAM_MAX` (500), `JOB_STREAM_MAX_PER_ORG` (20), `JOB_STREAM_MAX_AGE_S` (1800), `QUARRY_EDGE__JOB_RESULTS`, `QUARRY_JOB_TIMEOUT_{SCRAPE,BATCH,CRAWL}_S`, `QUARRY_SCHEDULE_CAPABILITIES`, `QUARRY_SCHEDULE_MIN_INTERVAL_S`, `QUARRY_SCHEDULE_REQUESTS`, `QUARRY_EDGE__SCHEDULE_POSTURE` (`observe\|enforce`), `QUARRY_INTERNAL_HMAC_REQUIRED`, `QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG` | job control on after a staging pass; loop on; others per design | WS15.B to WS15.I | capability flags (S0) flipped by S1, S2, S3 in turn; posture `observe` until Control/Application approvals |
| `QUARRY_EDGE__AI_FORMATS` (`0`), `AI_FORMATS_ZDR` (`refuse`), `AI_FORMATS_HARVEST` (`off\|context\|prefill`, `context`), `AI_FORMATS_REPAIR` (1), model/budget keys, `MODEL_GATEWAY_AI_CHAT_METERING` (`off\|service\|all`, `service`), `MODEL_GATEWAY_SERVICE_RPM` (120), `MODEL_GATEWAY_EXTRACT_TOOL` (`0`) | as stated | WS16.B to WS16.G, WS16.I | `AI_FORMATS_ZDR=allow` only after the E2 ZDR proof; extract tool flag removed after WS16.G ships |
| `QUARRY_EDGE__PROFILE_ENCRYPTION` (`off\|compat\|on`, plus `REQUIRED=1`), `PROFILE_KEY_PROVIDER` (`env\|org_core`), `PROFILE_OWNER_ENFORCE` (`off\|observe\|enforce`, `observe`), `ORG_CORE_KEYS_ENABLED` (`0` ships dark), `QUARRY_BROWSER_PROFILE_CAPTURE_V2` (`0` one release), `QUARRY_BROWSER_PROFILE_IDB`, `QUARRY_BROWSER_PROFILE_MAX_BYTES`, `..._SESSION_COOKIE_MAX_AGE_S` (604800) | as stated | WS17.B to WS17.I | encryption moves one step per fully rolled-out deploy; no rollback to plaintext writes for sealed rows (documented) |

### 7.2 Name collisions and overlaps resolved by this plan

1. **Crawl bounds (real collision).** WS6.A defines `QUARRY_CRAWL_{DEFAULT,HARD}_MAX_{PAGES,DEPTH}` (edge and orchestrator, 1000/5000/5/10); D14a also defines `QUARRY_EDGE__CRAWL_DEFAULT_MAX_PAGES` (1000) and `QUARRY_EDGE__CRAWL_MAX_PAGES_CAP` (10,000). The plan keeps WS6.A's names, WS14.D reads the same `limits.rs`, and D14a's two names are dropped. The hard ceiling differs (5000 versus 10,000): the owner decides; the plan uses 5000 with env override. D14a keeps `CRAWL_BATCH_MAX_URLS`.
2. `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES`: one flag, defined by WS4.H; WS17.B (P0) reads it. If WS17.B merges first it adds the reader with the identical name and observe-first semantics; WS4.H then reuses it.
3. Metering switches `USAGE_BROWSER_METRICS` (WS6.C) and `USAGE_LLM_METRICS` (WS16.G): same tri-state `off|shadow|on`, same compose-anchor-to-edge-var pattern; WS16.G must not read WS6.C's switch.
4. Receipt text policy: `HUMAN_RECEIPT_MODE` (WS8.B, `metadata` default) and `RECEIPT_TEXT_MODE` (WS12.D, `sensitive` default) both govern receipts: the stricter wins for human steps; `redacted_for_record` is defined once (WS12.D) and WS8.B ships a narrow predecessor.
5. Three-layer switches `QUARRY_EDGE__HUMAN_BROWSE`, `BROWSER_BROKER_HUMAN_BROWSE_ENABLED`, `BROWSER_HUMAN_BROWSE` (and `QUARRY_EDGE__ATTACH`, `BROWSER_ATTACH`) are one logical switch per feature, one per service. Deploy order broker, model-gateway, Quarry, gateway; the feature is visible only when all layers are on; removed together at GA.
6. TTL ordering contract (D3-4): the driver reaper (idle 3600 s, max age 21600 s) sits above every run-sweeper number (agent idle 300, interactive idle 900, takeover 1800, agent ceiling 1800, interactive backstop 14400); the human-browse absolute grant TTL (4 h, D8) equals the interactive backstop. WS3.F and WS2.D must cite each other's defaults in their PRs and a boot check should assert the ordering.
7. Mode vocabularies differ across designs (`off|observe|enforce` in WS7, WS14, WS17; `off|shadow|on` in WS6; `off|shadow|skip|on` in WS5; `off|compat|on|REQUIRED` in WS17.D; `lenient|strict` in WS5 and WS14). The plan renames nothing already designed; the register above is the translation table. Semantics are common: `observe` equals `shadow` (evaluate, count, never block).
8. The `QUARRY_BROWSER_` prefix is read by two services: quarry-browser in the Ingestion compose (WS3, WS12, WS17) and, as `QUARRY_BROWSER_AGENT_*`, execution-core in the Model Plane compose (WS11). Not a clash (separate compose files); no rename. The legacy `QUARRY_BROWSER_AGENT_LLM` is retired at WS11.F stage 4 and must disappear from both composes in the same PR.
9. Single-underscore raw env (`QUARRY_EDGE_AUTH_DEV_BYPASS`, `QUARRY_EDGE_ARTIFACT_BACKEND`) versus the double-underscore config namespace: the production refusals read the raw names case-sensitively while runtime readers are case-insensitive (S9 H3, H4). Fixed by the PRE-2 PR; new flags must use only the `QUARRY_EDGE__` form.
10. `BROWSER_BROKER_GRANT_TTL_SECONDS` and `BROWSER_BROKER_MAX_RUN_DOMAINS` are named in both D1 and D8: one definition, WS1.A.
11. `ORG_CORE_SERVICE_CREDENTIALS` is a shared Control Plane compose variable edited by WS14.H (audit-core), WS15 (control principal) and WS17.H (keys): one appended entry per PR in the order the slices merge.

### 7.3 Deployment evidence recipe per wave

The repo counts a deployment claim only after rebuilt local images, a running stack and a verified end-to-end flow with a stated observable assertion. Every slice in section 1 repeats this in its design; the wave recipe below is what the plan requires at each wave exit.

**Stack recipes (from project memory and S9, not re-verified in this pass).**
- Ingestion Plane: `cd "apps/Ingestion Plane" && make up` runs `docker compose --env-file ../../.env --env-file .env -f docker-compose.yml up -d --build --wait --wait-timeout 300`; the memory recipe passes a longer env-file chain (Control and Application `.env.generated-secrets`, `Quarry-v2/.env`, the core `.env` files, `apps/Ingestion Plane/.env.generated-secrets` last) because required interpolations such as `QUARRY_CONTROL_API_KEY` may be unset with the Makefile's two files `[UNVERIFIED]`. Override the macOS `CORE_SYSTEM_CACHE_ROOT` and `CARGO_TARGET_DIR` defaults on Windows. The edge is at `127.0.0.1:8082` in this compose and `127.0.0.1:38080` in the standalone compose (the scripts' `:8080` default is wrong, S9 H7). `make test-endpoints` checks edge `:8082/health`, imports `:3025/ready`, integration `:3026/health`.
- Gateway and SPA: image swap with `swap_gateway_image.py` then frontend restart per the ingestion deploy recipe; `:5173` is the main checkout's container over a Windows bind mount, so HMR never sees host edits: restart the container (the browser also caches `global.css` across the restart); a worktree needs its own second dev server. `pnpm` inside a git worktree prunes `pnpm-lock.yaml`: restore it after the last pnpm call.
- Model Plane: `bash "apps/Model Plane/scripts/compose.sh" up -d --no-deps --build <service>` (needs `MODEL_PLANE_RUNTIME_ENV_FILE` as an absolute, canonical, regular, non-symlink file).
- Control Plane: `bash scripts/run-control-plane.sh build <svc>` then `up -d --no-deps --build <svc>`; a bare `docker compose build` exits 0 and builds nothing.
- Convex: `docker restart convex-gateway` re-pushes schema and functions; wait for "Convex functions ready!".
- Quarry Rust tests on the Windows host: inside a `rust:1` container with `quarry-cargo`, `quarry-go`, `quarry-target` volumes and `bash -c` (not `-lc`); a full `-p quarry-core -p quarry-runtime -p quarry-edge` run exceeds 10 minutes.

**Wave 1 exit evidence (all on the rebuilt local stack).**
1. WS4.A: two-org Redis flow and ZDR bypass counters (section 6, PR 1); WS4.B purge record.
2. WS6.A: 7-page crawl flow through the gateway (PR 2).
3. WS2.A: `evaluate` answered 403, then `get_content` still returns `Example Domain` (PR 3).
4. WS2.C and WS2.D: a user takeover makes the next agent step return 409 `USER_TOOK_OVER`; a viewer-less interactive run expires only after 900 s idle in dry-run logs first; a 33rd run returns 429 `org_run_cap`/`global_run_cap` with `Retry-After`.
5. WS3.A and WS3.C: `ps` shows zero `--no-sandbox` processes while a browser step succeeds, in dev and in the production overlay; two sessions with different viewports do not relaunch Chromium (process start time unchanged).
6. WS3.F: the 13th session returns 429 `browser_session_cap`; `pkill chromium` then a new run succeeds; `/ready` JSON carries `browser`.
7. WS1.A to WS1.C (production-mode contract): production-overlay quarry-edge with the validator URL set boots; an AI browser run end to end through the gateway with `BROWSER_GRANT_ENFORCEMENT=strict`; broker restart keeps an issued grant valid; `browser-grant-prod-contract.sh` green; the earlier expected-failure legs B and C turn green.
8. WS5.B and WS5.D: a PDF through `/v1/scrape` returns text with `metadata.content_kind`; an inline ZDR scrape returns markdown and writes no artifact row.
9. Contract gates: `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`, `node --test scripts/tests/zdr-contract-schema.test.mjs`.

**Wave 2 exit evidence.**
1. Shared browser (WS8, WS9): a human session opens, the agent attaches in an agent-flagged tab, the user takes over (409 on the agent's next step), gives back, and stops; the same run visible in the dock (WS9) with the evidence panels.
2. Planner (WS11): stage 1 `allowlist` run on a dev org, stage 2 `org_opt_in` production-overlay run with a grant (needs WS1, WS6.B enforcing), a budget exhaustion produces 402 `BUDGET_EXHAUSTED` and an honest completion.
3. Crawl and jobs (WS14, WS15): the fixture-site crawl denies off-site and robots-disallowed URLs with typed denials; cancel, pause and resume a running crawl; a schedule fires on its cron, and a fire that violates posture shows a `schedule_skipped` event (with `QUARRY_SCHEDULED_JOBS_ENABLED=1` only after WS14.I).
4. Scrape (WS13, WS16): a scrape with `formats:[screenshot,pdf]` and `actions` returns captures; a `json` format request over a real Model Plane grant returns schema-valid data and meters `quarry.extract.llm`.
5. Profiles (WS17): a profile saved and restored across a Quarry restart; envelope rows unreadable without the key; erasure removes them.
6. Trace and metrics (WS10, WS6): `GET /api/v1/runs/{id}/trace` after an AI run; `/metrics` on the internal listener; graceful drain finishes with a live stream.
7. Redaction and policy (WS12, WS7): a typed password never appears in receipts, events, artifacts or the planner prompt; a `ResolvedPolicy` denial shows `policy.denied` with `decision_source`.

## 8. VERIFICATION STACK

Source: `sweeps/S9-ci-and-run-commands.md` (read from the live tree at afb83928 plus uncommitted work; four read-only test scripts were run there, nothing else). Evidence tags below: `[CI]` stated by a workflow, `[RUN]` executed by S9, `[MEM]` user memory note (not re-verified), `[UNVERIFIED]` needs a real run. All commands run from the repo root `C:/dev/CoresSystem` unless a `cd` is shown. The Windows host cannot build the Quarry Rust crates natively (boring-sys2 needs cmake, clang, perl, go, protoc) `[MEM]`.

### 8.1 Commands per lane and the CI that gates them

| Lane | Commands | CI that gates it today | Notes |
|---|---|---|---|
| L1 GRANTS-BROKER | `cd "apps/Model Plane/go/services/browser-broker" && GOWORK=off go test ./...` (per service; `-race` needs cgo, unavailable on this host `[MEM]`); Docker integration `GOWORK=off go test -tags integration ./internal/grant/...`; `cd "apps/Model Plane/rust" && cargo test -p model-gateway --lib browser_grants`; `cd "apps/Control Plane/auth-core" && npx jest plane-service-principals-policy.contract`; `node --test scripts/tests/browser-grant-contract.test.mjs` [NEW]; `bash scripts/tests/browser-grant-prod-contract.sh` [NEW]; `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test`; proto regeneration in the pinned buf container (`buf` is not on the host) | `model-plane-rust-ci.yml` [M] (cargo fmt, check, `--all-targets`, `test --workspace --no-fail-fast`; golden drift script) [CI]; `contract-goldens.yml` [C]; `auth-service.yml` [C]; `buf.yml` [C] is broken as written (unquoted path, 0-byte baseline) so it gates nothing; the Go broker has no CI: WS1.A adds `browser-broker-ci.yml` and `browser-grant-contract.yml` [NEW] and repairs `buf.yml` | Go module per service, no workspace build: `GOWORK=off` for new dependencies (`go.work` exists in `apps/Model Plane/go`, S9 corrects the older memory). |
| L2 ADMISSION | inside a `rust:1` container from `apps/Ingestion Plane/Quarry-v2`: `cargo check --workspace`, `cargo test --workspace`, targeted `cargo test -p quarry-edge agent_routes agent_evaluate_denied`, `cargo test -p quarry-runtime observation action_runtime` | `quarry-v2-ci.yml` [C]: hard gates are `cargo check --workspace` and `cargo audit` (four ignores); `cargo test --workspace` (lines 201-205) runs; coverage and govulncheck are report-only (`continue-on-error`) | `rerank_behavior.rs` is [U] and so not run by CI until committed (PRE-1). |
| L3 DRIVER | `cargo test -p quarry-browser --features chromiumoxide`; the live lane `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 cargo test -p quarry-browser --features chromiumoxide -- --test-threads=1` (new job `quarry-browser-live`, WS3.B); `make bench-browser` (`quarry-browser-benchmark`); `ssrf.rs` has three `#[ignore]` tests needing a browser binary | `quarry-v2-ci.yml` lines 207-211 (no live Chromium); the live job is added by WS3.B first as `continue-on-error`, promoted to a hard gate after two weeks green | Five live tests are rotted and are repaired in WS3.B before WS3.C/D rely on them. |
| L4 ZDR-TENANT | `cd "apps/Frontend Plane/verevonv3/apps/gateway" && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`; CLAUDE.md form `cd "apps/Frontend Plane/verevonv3" && cargo test --manifest-path apps/gateway/Cargo.toml --all-targets` (CI omits `--all-targets`: run both); Quarry container tests as L2; `bash scripts/tests/zdr-contract-sync.test.sh` (90 passed, 0 failed, 4 pending on the live tree `[RUN]`); `node --test scripts/tests/zdr-contract-schema.test.mjs` (65/65 `[RUN]`) | `verevonv3-ci.yml` [M] job Gateway (fmt, clippy `-D warnings`, test); `contract-goldens.yml` [C] (`REQUIRE_CONTRACT` deliberately unset, so four posture-matrix consumers stay pending) | `cargo test` for the gateway builds natively on the host (no boring-sys2). |
| L5 SCRAPE | Quarry container tests as L2; `node --test "apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs"` (17/17 `[RUN]`, file [U]); `cd "apps/Model Plane/rust" && cargo test -p model-gateway --lib` for the consumer slices | `quarry-v2-ci.yml`; `quarry-docs-contract.yml` is [U] and not on main | OpenAPI route-coverage CI is added by WS5.H. |
| L6 SPA-GATEWAY | `cd "apps/Frontend Plane/verevonv3"`: `pnpm install --frozen-lockfile`, `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build` (the gate is `pnpm verify`); SDK prerequisite `cd "apps/Ingestion Plane/Quarry-v2/sdks" && ./generate.sh && cd typescript && pnpm install --ignore-scripts && pnpm run build`; gateway Rust as L4; Playwright `pnpm test:e2e` (27 specs, needs a live stack and storage state) | `verevonv3-ci.yml` job spa-verify (`pnpm verify`) and gateway-rust; Playwright is not in CI; `demo-product-build` job is uncommitted and cannot pass as committed | Hazard: any `pnpm` inside a git worktree prunes `pnpm-lock.yaml` `[MEM]`; restore it after the last call and never run pnpm in the live tree. |
| L7 CRAWL-JOBS | `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./...`; `.../services/quarry-orchestrator && go test ./...`; `.../pkg/quarrycontracts && go test ./...`; Temporal replay test for every new `GetVersion` id; Quarry container tests for the edge halves | `quarry-v2-ci.yml` Go jobs (govulncheck and coverage report-only: 41.6% control and 37.2% orchestrator against an 80% floor on 2026-09-08 `[CI comment]`) | `internal/jobs`, `internal/activities`, `internal/controlauth` have no test files today; WS6.A and WS15 add them. Host Go runs are `[UNVERIFIED]`. |
| L8 PLANNER-MP | `cd "apps/Model Plane/rust" && cargo test --workspace` (CI); on Windows `cargo test -p <crate> --lib`; session-core Postgres tests `DATABASE_URL=postgres://session_core:session_core@localhost:5432/session_core_test REQUIRE_CONTRACT=1 cargo test -p session-core --bin session-core zdr_receipt -- --ignored --test-threads=1`; live `#[ignore]` suites (`browser_agent_e2e.rs`, source review/recheck) stay manual | `model-plane-rust-ci.yml` [M]; the `session-core-receipts` job (lines 127-169) is uncommitted | CI comment claims 2070 passed and 24 ignored, not re-run; 304 pedantic clippy warnings so clippy is not gated. |
| L9 CONTROL-PLANE | org-core: Postgres 16 container (user and password `org_core`, port 5432), role and fixture bootstrap from `org-service.yml:74-93`, then `go test -v -p 1 ./...` with `REQUIRE_CONTRACT=1`, `TEST_DATABASE_URL`, `CONTROL_LIFECYCLE_TEST_DATABASE_URL`, `go build ./cmd/server`; auth-core: `pnpm install` once, then `npx jest <file>` and `npx tsc --noEmit -p tsconfig.json`; `REQUIRE_CONTRACT=1 pnpm test`; `bash scripts/tests/principal-registry-parity.test.sh` | `org-service.yml` [C] (setup-go 1.24 versus `go.mod` 1.25 `[UNVERIFIED]`); `auth-service.yml` [C] (`pnpm install` not frozen; build labelled "Type check") | auth-core lint dies with ENOBUFS `[MEM]`; files in the worktree are CRLF, scripted edits must preserve it; the six `*.postgres.spec.ts` files are not in CI. |
| L10 SHARED-BROWSER-QUARRY | as L2 plus L3 live lane for the driver parts; Playwright `tests/e2e/browser-workspace-*.spec.ts` against a live stack for the end-to-end attach legs | as L2/L3; Playwright not in CI | the cross-host Playwright spec contradicts the first-host session limit (D9 correction): fix it with WS8.D. |
| L11 PROFILES | Quarry container tests as L2; Postgres job added after `quarry-v2-ci.yml` line 211 for the store tests; org-core as L9 for E2 | `quarry-v2-ci.yml` | sqlx migration versions must be unique and applied files immutable (section 3.1). |
| L12 TRACE | proto: buf container; `cargo test -p mp-events`, `-p session-core`; Quarry container tests; Go `quarryotel`/`httpx` tests; convex project tests; `bash scripts/tests/zdr-contract-sync.test.sh` | `buf.yml` (repair in WS10.B), `quarry-v2-ci.yml`, `model-plane-rust-ci.yml` | the 28 untracked fixtures must be committed together (PRE-1). |
| L13 WEB-POLICY-QUARRY | Quarry container tests; `node --test scripts/tests/quarry-model-plane-switches.test.mjs` (8/8 `[RUN]`, file [U]) | `quarry-v2-ci.yml` | `quarry-token-claims.golden.json` exists in three trees; regenerate once. |
| L14 REDACTION | Quarry container tests; `cargo test -p mp-scrub`; `bash scripts/tests/zdr-contract-sync.test.sh` for `secret-shapes.v1.json` | `quarry-v2-ci.yml`, `model-plane-rust-ci.yml`, `contract-goldens.yml` | the Chromium AX-masking recipe (D12-6) runs before WS12.B merges. |
| L15 CAPACITY-METERING | Quarry container tests; billing-core `go test` (Control Plane); Prometheus scrape config check; gateway Rust as L4 | `quarry-v2-ci.yml`; no billing-core workflow was found in S9 `[UNVERIFIED]` | `/metrics` listener and `/ready` JSON are shared with WS3.F. |

### 8.2 Contract and cross-plane gates (every lane that edits a golden, manifest or registry)

- `bash scripts/tests/zdr-contract-sync.test.sh` and `--self-test` (the self-test copies to scratch repeatedly; run in CI parity, not on every save), `node --test scripts/tests/zdr-contract-schema.test.mjs`: workflow `contract-goldens.yml` [C] and `model-plane-rust-ci.yml`. With `REQUIRE_CONTRACT=1` the four pending posture-matrix consumers would fail; CI leaves it unset by design.
- `bash scripts/tests/principal-registry-parity.test.sh` for registry edits (`auth-service.yml`).
- Docs contract: `node --test "apps/Ingestion Plane/Quarry-v2/scripts/tests/docs-contract.test.mjs"` after any Quarry prod-check, `main.rs`, `config.rs` or principal-registry change (the CI workflow is untracked).
- Production-mode proof: `docker compose -f docker-compose.yml -f docker-compose.production.yml config -q` from `apps/Ingestion Plane` renders without starting anything; use it for the H2 check (the production overlay as committed cannot start quarry-edge: no validator URL and `ARTIFACT_BACKEND` defaults to memory).

### 8.3 Known pre-existing failures and traps (diff against these; do not treat as regressions)

- Quarry: govulncheck reports stdlib CVEs fixed in Go 1.26.6 with the toolchain at 1.26.5; Go coverage 41.6% and 37.2% against an 80% floor; Rust coverage never measured on a clean build; four advisories ignored in `cargo audit` (RUSTSEC-2023-0071, 2025-0134, 2026-0192, 2026-0253) `[CI comment]`.
- Model Plane: 304 pedantic clippy warnings and live dead_code warnings (clippy not gated); the earlier memory notes about Windows breaks in `inference-core` and `e2e_invoke_chain_test.rs` are stale per S9 (both look fixed) `[UNVERIFIED]`.
- auth-core: 15 failing tests in 3 suites on HEAD (`deployment-contract.spec.ts` 8, `plane-service-principal.spec.ts` 6, `model-plane-token.controller.spec.ts` 1) `[MEM]`: compare the failing sets, not exit codes; `pnpm lint` ENOBUFS in batch 1/8.
- SPA: older memory baseline (2026-09-06, worktree) of 21 typecheck errors, a dead lint and 12 failed vitest files; the live `package.json` pins `typescript` to the TS6 alias and typescript-eslint 8.69, so it is probably stale `[UNVERIFIED]`; re-measure once on a clean checkout before WS9 starts and record the baseline in the first WS9 PR.
- Gate hygiene (S9 H1, H5-H8): untracked gate files and 28 fixtures; `buf.yml` broken; the Quarry scripts' wrong default port; no CI for Model Plane Go services, integration-corev2 (112 test files), the 27 Playwright specs, `quarry-vision-sidecar` pytest and the live `#[ignore]` tests; nested workflow directories that GitHub never runs.
- `docker-compose.yml` [M] defaults the Model Plane switches `QUARRY_EDGE__AUTOPROMPT` and `SEMANTIC_RERANK` to `0` while the committed compose defaults them to `1` (S9 H6): the untracked switch test asserts `0`; do not commit only one of the two.
