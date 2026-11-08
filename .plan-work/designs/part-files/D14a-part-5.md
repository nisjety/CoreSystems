
## 5. Dependencies and ordering

### 5.1 Ordering

| Dependency | Kind | Detail |
|---|---|---|
| Slice 1 before 2 to 6 | soft | the Rust tests need the fixture driver; the real-stack evidence needs the overlay |
| Slice 2 before 3 and 4 | hard | `CrawlContext`, `CrawlReport` and the evaluator are what both slices call |
| Slice 3 before 4 | hard for the e2e assertions, soft for the Rust code | a robots denial only appears as `page_blocked` once the workflow classifies errors through Temporal's application error (slice 3) |
| Slice 3 before 5 and 6 | hard | params plumbing, `GetVersion`, batch counters |
| Slice 4 before 5 | soft | batch robots is the same fetch-time gate |
| Slices 2 to 6 before slice 8 | hard | the flip needs the soak data |
| Slice 7 after D7 S2 and S7, a grantor decision, and a Control Plane credential PR | hard | section 8 |
| This workstream before F-t2-018 (schedules that execute) | hard | audit 9.4 note 5: recurring crawls must not repeat the off-site walk; F-t2-018 also needs its own explicit re-enable so existing schedule rows do not all start firing |
| F-t2-017 (job lifecycle) | consumer | its `/errors` and status routes read `page_blocked` events and the `run_completed` summary defined here; it owns `ContinueAsNew`, cancel, the 30-minute execution cap, and the missing `type` filter on control's `jobEvents` |
| F-t2-041 (sitemap, dedupe) | consumer | seeds from `quarry_transform::sitemap` and extends `crawl_filter` (canonicalisation); the `sitemap` and `ignore_query_parameters` request fields are reserved here |
| F-t2-055 (crawl planning, stronger `/v1/map`) | consumer | uses the shared evaluator and the gate; `map_routes.rs` conflict |
| F-t2-005 / D5 (`ScrapeOptions`) | soft, two-way | `scrape_options` embed (D5 Epic E1) is accepted by the crawl request only after D5 slice 4; this design reuses D5's validator helper and `FetchHints.max_body_bytes` if merged |
| D7 / F-t2-022 | hard for slice 7 only | `ResolvedPolicy::require`, entitlement `web.robots_override`, `robots.max_mode`; D7 S6b (durable `policy_snapshot` into `run_page`) rides the same request extension and the same HMAC change |
| D4 (ZDR matrix) | soft | S-8 adds `details.zdr_refusal` to the crawl and batch refusals; this design leaves the refusal untouched |
| D6 (budgets) | soft | clamps `max_pages` to remaining credits at the same `crawl_handoff` site |
| F-t2-067 (single product identity) | soft | replaces the `ROBOTS_PRODUCT_TOKENS` constant and unifies the three user-agent strings |

### 5.2 Merge-conflict hotspots (files and ranges shared with other workstreams)

| File | Ranges this design touches | Also touched by |
|---|---|---|
| `QE/routes.rs` [M] | `CrawlRequest` 532-553, refusal 563-588, `crawl_handoff` 590-667, `BatchRequest` 669-679, `batch_handoff` 681-725, `HandoffAck` 727-738, `InternalRunPage` 743-769, result 771-841, `internal_run_page` 898-1062, `PageRunner {` literals 449, 989, 1104 | D5 (`ScrapeRequest` 346-379, 416-461, 984-1001, 1072), D7 S2 and S6b (crawl and batch handoff 590-725, run_page body), D4 S-8 (563-588), F-t2-017 (route table 133-134, 231-234), F-t2-018 (254-276), F-t2-040 (batch 669-725), F-t2-041 (result 771-841) |
| `QR/pipeline.rs` [C] | `PageRunner` struct 36-130, `run_inner` 298-331 and the post-fetch point after 391 | D5 (`doc_routing`, `FetchHints` literal 354-360, content sniff near 421-443 and 506), D7 S6 (final-URL recheck, same function as slice 4's redirect recheck) |
| `QR/fetch.rs` [C], `QR/driver.rs` [C] | body read 376-391, `FetchHints` | D5 (`max_body_bytes`, 26 MiB cap), F-t2-047 (retries), F-t2-007 |
| `QR/host_scheduler.rs` [C] | `acquire` 150-180, `HostSlot` 98-123 | F-t2-025 and D6 (capacity) |
| `QE/main.rs` [M], `QE/state.rs` [M], `Q/crates/quarry-edge/tests/support/mod.rs` [M] | the `AppState` literal (about 1431) and one new field | every design that adds an `AppState` field (D4, D6, D7 baseline, D8); the three files carry live diffs |
| `QR/lib.rs` [M] | 42 (modules), 112-113, 160-165 (re-exports) | the Data Plane rerank work in flight |
| `ORCH/workflows/workflows.go` [C] | inputs 75-87, `runPage` 174-232, batch 359-383, crawl 396-512, durable 518-610 | F-t2-017 (handlers in `BatchJobWF`, `ContinueAsNew`), F-t2-018 (run id 308-312), F-t2-031 (`run_completed` 289-292, 379-382, 508-511, 607-609), F-t2-040 (sequential batch loop 370-377), F-t2-041 (dedupe 479-482, request id 617-620) |
| `ORCH/activities/activities.go` [C] | `RunPageInput` 165-181, body 213, `RunPage` 211-264, `runBindingSig` 472-476 | D7 S6b (`PolicySnapshot`), D6, D4 |
| `ORCH/jobs/dispatcher.go` [C] | `crawlInputFromJob` 432-454, `batchInputFromJob` 456-469 | F-t2-017, F-t2-040 |
| `GW/domains/knowledge/quarry.rs` [C], `GW/domains/actions/dispatchers.rs` [C] | `start_crawl` 306-376, `normalize_crawl_body` 386-416, `dispatch_crawl_site` 314-360 | F-t2-017 (status and cancel routes), F-t2-018, F-t2-040, F-t2-041 (`discover` 507-538) |
| `FE/shared/actions/action-registry.ts` [C] | `crawlSiteInput` 568-571, `knowledge.crawl_site` 1320-1329 | F-t2-017 (2563-2594), F-t2-018 (887-897); `action-surface-contract.test.mjs` asserts registry and handler IDs are equal, so any new action ID changes both (this design adds none) |
| `Q/docs/openapi.yaml` [M], `Q/docs/GAP.md` [C], `Q/docs/ROADMAP.md` [C] | `CrawlRequest` 4361-4374, `BatchRequest` 4375-4385, `InternalRunPage` 4387-4410; GAP rows of 2.3 | F-t2-017, 018, 031, 040, 041, D5 |
| `apps/Ingestion Plane/docker-compose.yml` [M] | quarry-edge and quarry-control env | most Ingestion changes |
| `Q/services/quarry-control/internal/store/pg/migrations/` | none: this design needs no migration, next free number remains `015` for F-t2-018, 031 and 040 | n/a |

## 6. Plane ownership, ZDR propagation and Model-proposes/Quarry-executes check

Naming caution: `quarry-control` is the Ingestion Plane's own job and event service. The Control Plane (org-core, audit-core, auth-core) is a different plane.

| Rule | Statement | Status |
|---|---|---|
| Control Plane owns identity, users, orgs, billing, sessions, audit, quotas, entitlements | the override entitlement (`web.robots_override`), the ceiling `robots.max_mode`, the claim, the admin API and the audit sink are Control Plane (D7 S7, audit-core). Quarry reads the signed claim and writes one audit event over audit-core's HTTP API (`POST /v1/audit`, a service credential pinned to plane `ingestion`); it never reads Control Plane tables | held; slice 7 needs a Control Plane credential PR |
| Data Plane owns documents, chunks, embeddings, retrieval, graph, wiki, source traces | unchanged: `ingest` still goes through the existing Data Plane client in `PageRunner`; denied URLs are never fetched and therefore never ingested; denial events and robots state are not written to the Data Plane | held |
| Ingestion Plane captures evidence and persists through Data Plane contracts only | scope, robots, denials, Crawl-delay and the batch check are Ingestion Plane (quarry-edge, quarry-orchestrator, quarry-control); frontier logic stays in Rust (the evaluator), Go owns only the loop and counters | held |
| Model Plane owns reasoning, sessions, grants, cost | no BrowserBroker grant is involved: crawl and batch carry none (`QE/auth.rs:504-545` fail closed for service principals); no Model Plane code or table is touched | held |
| Application Plane | no change | n/a |
| Frontend Plane owns UI and same-origin gateway normalisation | gateway builds a fresh body from an allowlist (UI surface and Model surface), clamps and caps; UI shows the denial summary | held |
| No direct database crossing | every cross-plane or cross-service call is HTTP with the existing HMAC or service-token auth; scope rides in `jobs.params` through the existing control API | held |
| Model proposes browser actions; Quarry executes or rejects | the Model proposes a crawl through `knowledge.crawl_site`; the gateway executes it under the **user's** token; Quarry rejects what it must: the Model-visible schema has no external-links or robots key, the gateway's Model surface drops them, the edge refuses `ignore_robots_txt` for service principals and for any caller without the policy ability, and typed denials come back for anything out of scope | held; one risk below |
| ZDR must propagate through every content-persisting boundary | crawl and batch stay refused under ZDR before any new field is parsed (`refuse_durable_job_under_zdr` runs first; test pins it). The five durable boundaries a lifted refusal would have to cover are listed in 2.1 item 12 and are not touched. The new in-memory robots cache holds only public robots.txt keyed by origin, is never persisted, is bypassed (`check_uncached`) for ZDR callers on `/v1/map`, and no robots log line or metric carries a URL or host. Denial events go through `emit_page_event` (ZDR-aware sink). Audit details carry `seed_host` only on non-ZDR durable jobs | held |

Rules at risk:
1. **`knowledge.crawl_site` has `requiresApproval:false`** (action-registry.ts:1320-1329), so a Model-proposed crawl executes without a user click. This design reduces the blast radius (same-host default, page cap 1000, no external links from the Model) but does not add approval; that belongs to sibling (b) (F-t2-033 and F-t2-042). Flagged, not solved here.
2. **The `crawl` object on `run_page` is not covered by the HMAC until slice 7**, so a holder of the runtime token and a captured valid signature could widen scope on a replay. Same capability they already have (fetch that URL), no new bypass of robots (robots mode is not widened before slice 7 because `ignore` is refused at the handoff), and slice 7 covers it. Low.
3. **`ZDR` and `/v1/map`** now touch a shared in-memory cache for non-ZDR callers: public data only, bounded, in memory.

## 7. Cross-session interfaces

### (a) Verevon Space (shared workspaces, agent PC and browser inside Spaces, ZDR-safe telemetry)

| Shared piece | Proposed owner | What the user must reconcile |
|---|---|---|
| Space-level restriction of crawl (for example "agents in this Space may not follow external links") | D7 / WS7: policy key `crawl.external_links` (D7 section 3.2 lists it as "not in v1, F-t2-016"); enters only through D7's tighten-only `RequestAsk`, built by the gateway from the Space's own settings. Proposal: add it to catalogue v1.1 as an ability, default allowed, so `allow_external_links:true` requires it | whether a Space may disable crawl entirely (not just external links); the rule "Space access never widens an owner-plane ACL" holds because the Space can only tighten |
| Ownership of crawl jobs and results | unchanged: org + initiating user (`params.user_id`, Data Plane private-by-default). No Space id is added to job params | sharing a crawl run with Space members is F-t2-072 and needs its resource-taxonomy ADR (topic 8); nothing here pre-empts it |
| Agent PC or browser inside a Space starting a crawl | the action path runs under the launching user's token through the gateway | whether an agent-initiated crawl must carry the Space id for audit only (not for access); coordinate with D7 audit rows |
| ZDR-safe telemetry | F-t2-004 / D4 | denial and robots metrics are content-free counters (`crawl_links_denied_total{code}`, `robots_decision_total{decision}`, `robots_would_deny_total`), no host, URL or org label; F-t2-004's log lint must allow these names and forbid the robots log fields it would otherwise flag |
| Presence or progress of a crawl in a Space UI | Application Plane projection (F-t2-014 / D10) | the denial summary lives in `run_completed.payload.denials`; if a Space panel shows it, it reads the projection, not quarry-control |

### (b) Model Plane and chat/agents (BrowserBroker grants, agent browser path, delegated bearer, approvals)

| Shared piece | Proposed owner | What the user must reconcile |
|---|---|---|
| Model-visible crawl schema | Frontend Plane action registry (edited here): `maxPages`, `maxDepth`, `includePaths`, `excludePaths`, `allowSubdomains` only | Model Plane tool definitions and F-t2-042 (Quarry jobs as gated tools) must reuse this allowlist and the gateway `Surface::Model` builder; they must not add external-links or robots keys |
| Approval before a Model-proposed crawl | sibling (b), F-t2-033 / F-t2-042 | `requiresApproval:false` today; suggest approval above a page count or for any non-default scope. This design does not decide it |
| Delegated bearer and service principals | unchanged | crawl and batch stay unreachable for service principals (auth.rs:504-545); if an agent must start crawls it goes through the gateway's user token, not a new service scope. Recommend against adding `crawl:write` to a service principal |
| BrowserBroker grants and the agent browser path | sibling (b), F-t2-001 / 010 / 013 | robots on Model-proposed browser navigations (audit R-t2-04-01 item 3) is out of scope; `RobotsGate::check` is the API they can call from `validate_navigation`. Whether a human's own navigation in the browser pane is exempt is open question 11 and ADR topic 3 |
| Run trace vocabulary | D10 / F-t2-014 | `page_blocked` payload fields (`code`, `message`, `remedy`, `source`) and the terminal `denials` object become part of the trace vocabulary; keep field names stable |
| Typed error for the Model | shared | the 403 `details.denial` object (with `remedy`) is what a Model sees when its proposed crawl hits robots; the remedy text must not suggest the override to the Model (it says the site disallows the agent and that only an org policy can change it) |

## 8. What can proceed WITHOUT an ADR/decision, and what is blocked by which decision

Proceeds now, no decision needed: slice 0 (docs and tests), slice 1 (harness), slice 2 (Rust filter, inert), slice 3 in `observe` mode (no behaviour change for crawls, adds visibility and the classification fix), slice 4 in `observe` mode, slice 5 (batch validation), slice 6 (vocabulary; the override key is refused). These deliver everything in the audit's steps 1 to 3 except the override and the flip.

Blocked or gated:
- **Flipping `observe` to `enforce` (slice 8)** needs the DECISION of D2 (default scope, `www.` equivalence, whole-host default, page-cap default and the rejection of `max_pages:0`) and, for robots, ADR topic 5 (unreachable posture, token set). Both can be answered from the soak data.
- **Slice 7 (override and audit)** is blocked by ADR topic 1 (who owns web policy: org-core claims or capability-core grants; D7 recommends org-core claims and ships an owner-neutral slice first), D7 S2 and S7 (the ability and the entitlement), the grantor question (open question 11 in the audit, D7 Q3), and the Control Plane credential PR for audit-core.
- **Lifting the ZDR refusal** is gated by ADR topic 4 and is not recommended; nothing in this plan depends on it.
- **Agent and human browsing robots enforcement** is gated by ADR topic 3 and belongs to sibling (b).

## 9. Open questions for the user (only real blockers)

1. **Approve the default-scope change before the flip (blocks slice 8, not slices 2 to 7).** Same host with `www.` treated as equal, whole host rather than the seed's directory, default page cap 1000 (was unlimited), `max_pages:0` rejected. Existing crawls lose off-site pages; the soak in `observe` quantifies it first.
2. **Robots posture when robots.txt is unreachable (5xx or timeout): disallow (RFC 9309, recommended) or allow (Firecrawl's behaviour)?** Plus confirmation of the interim token set `quarrybot` and `quarry` until F-t2-067. Blocks the robots `enforce` flip only (ADR topic 5).
3. **Who may hold the ignore-robots override: platform operators only (D7's proposed default) or org admins, and is it a standing org setting plus a per-crawl opt-in?** Blocks slice 7 only (open question 11, ADR topic 1).
4. **Keep the ZDR refusal for crawl and batch?** Recommended yes (ADR topic 4); a no here means an XL redesign that this plan does not cost.

## 10. INTERFACE SUMMARY (for the integrating architect)

**Files by plane.** Ingestion, Rust: `quarry-core` (`crawl_scope.rs` new, `crawl_denial.rs`), `quarry-runtime` (`crawl_filter.rs` and `robots_gate.rs` new; `robots_cache.rs`, `host_scheduler.rs`, `pipeline.rs`, `driver.rs`, `fetch.rs`, `crawl_frontier.rs`), `quarry-transform` (`robots.rs`), `quarry-edge` (`routes.rs`, `queue_routes.rs`, `map_routes.rs`, `crawl_request.rs` and `crawl_policy.rs` new, `state.rs`, `main.rs`, tests). Ingestion, Go: orchestrator (`workflows.go`, `activities.go`, `errs.go`, `dispatcher.go`, `crawl_denials.go` new), quarry-control (`resources.go`, `audit/client.go` new, slice 7). Frontend: gateway (`knowledge/quarry.rs`, `actions/dispatchers.rs`), SPA (`knowledge-client.ts`, `action-registry.ts`, `KnowledgeComposer.tsx`). Control Plane: audit-core credential (compose) and test; entitlement is D7. Docs: GAP, Features, CONTRACTS, POLICY, ARCHITECTURE, openapi, new `CRAWL_SCOPE_ROBOTS.md`.

**New contracts.** `run_page` request `crawl` and response `crawl` (CrawlContext, CrawlReport); 403 `details.denial`; `HandoffAck.effective` and `rejected`; job params `scope`, `scope_mode`, `robots`; route `/v1/internal/queues/:q/enqueue_batch`; `FetchHints.truncate_at_cap`; `HostScheduler::set_min_interval`; denial variant `robots_unreachable`; no new event types, no migrations, no protos. Flags: `QUARRY_EDGE__CRAWL_SCOPE_MODE`, `QUARRY_EDGE__CRAWL_ROBOTS`, `QUARRY_EDGE__ROBOTS_UNREACHABLE`, `QUARRY_CRAWL_DELAY_MAX_S`, `QUARRY_EDGE__CRAWL_DEFAULT_MAX_PAGES`, `QUARRY_EDGE__CRAWL_MAX_PAGES_CAP`, `QUARRY_EDGE__CRAWL_BATCH_MAX_URLS`, `QUARRY_EDGE__CRAWL_REQUEST_VALIDATION`; Temporal `GetVersion` id `crawl-scope-v1`.

**Dependencies.** Hard: slice 2 before 3 and 4; slice 7 on D7 S2/S7, ADR topic 1, an audit-core credential. Soft: D5 (`ScrapeOptions`, `max_body_bytes`), F-t2-017 (errors route), F-t2-041, F-t2-067. Must precede F-t2-018.

**Effort.** S0 1.5, S1 3, S2 4, S3 6, S4 7, S5 2, S6 6, S7 4 (+2 Control Plane), S8 1: about 34.5 engineer-days; the audit's "step 1 is S" is 10 here.

**ADR gates.** Topic 4 (keep ZDR refusal), topic 5 (unreachable posture, tokens), topic 1 (override owner, slice 7), topic 3 (human navigation, out of scope).

**Parallelisable with** D2, D3, D6, D8 and the browser items. **Conflicts with** D5 and D7 on `routes.rs`, `pipeline.rs`, `fetch.rs`, the `AppState` literal and the run-binding HMAC; F-t2-017, 018, 040 and 041 on `workflows.go` and `dispatcher.go`.

<!-- COMPLETE -->
