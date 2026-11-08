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

