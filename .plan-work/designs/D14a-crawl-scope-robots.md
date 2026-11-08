# D14a crawl-scope-robots: scoped, robots-compliant crawl and batch (WS14a, wave W2)

Audit source: `C:/dev/verevon-audit-2026-10-14/02-quarry-and-browser-audit.md`. Design date 2026-10-15. Read-only pass over the live tree `C:/dev/CoresSystem` (HEAD afb83928 plus uncommitted entries). Nothing was built or run; every "fails in production" statement is [INF] unless marked [READ].

Path shorthand used throughout (every path is relative to the repo root once expanded):

| Short | Expands to |
|---|---|
| `Q/` | `apps/Ingestion Plane/Quarry-v2/` |
| `QE/` | `Q/crates/quarry-edge/src/` |
| `QR/` | `Q/crates/quarry-runtime/src/` |
| `QC/` | `Q/crates/quarry-core/src/` |
| `QT/` | `Q/crates/quarry-transform/src/` |
| `ORCH/` | `Q/services/quarry-orchestrator/internal/` |
| `CTL/` | `Q/services/quarry-control/internal/` |
| `GW/` | `apps/Frontend Plane/verevonv3/apps/gateway/src/` |
| `FE/` | `apps/Frontend Plane/verevonv3/src/` |

Tags: `[C]` committed and unchanged, `[M]` committed but modified in the live tree, `[U]` untracked, `[NEW]` this plan would create it. Tags come from `tools/provenance.sh`.

## 1. Items resolved and wave

| Item | Title | Tier | Wave | Verified effort | This design's estimate |
|---|---|---|---|---|---|
| F-t2-016 | Scoped, robots-compliant crawl: wire CrawlFrontier scoping, robots enforcement and typed denials into the live crawl path | Priority | W2 | L | about 34.5 engineer-days across 9 slices, 0 to 8, plus about 2 in the Control Plane for slice 7 (section 4). The audit's "step (1) is S" holds only for the Rust-side filter; the Go workflow change is M because the workflow has no replay gating, the production (durable-frontier) loop has no test, and the typed-denial classification in the workflow is dead code today (section 2, items 2 and 11). |

Resolves in the same workstream, without owning them: the Control Plane half of the ignore-robots entitlement (shared with F-t2-022 / D7), the ZDR decision for the durable orchestrator path (ADR topic 4, recommendation: keep the refusal), the GAP.md and stale-doc corrections the card asks for. Consumed from siblings: `ScrapeOptions` (F-t2-005 / D5), the errors and status routes (F-t2-017), sitemap seeding and URL canonicalisation (F-t2-041), the single product user-agent token (F-t2-067).

Wave position: W2. It must land before F-t2-018 (schedules that execute) so recurring crawls do not repeat the off-site walk (audit 9.4 note 5), and before F-t2-041 and F-t2-055 which extend the same filter.

## 2. Verified current state

### 2.1 What the code does today (path:line, confirmed by opening the files)

1. **Both enqueue loops follow every link.** `ORCH/workflows/workflows.go:396-512` (`CrawlJobWF`, in-workflow BFS) enqueues every `res.Links` entry after only the depth test at 474 (loop 473-485, dedupe by exact string at 479-482). `crawlJobDurableFrontier` (518-610) does the same: depth test at 574, then one `FrontierEnqueue` activity per link at 575-584 with no in-workflow dedupe (the Postgres queue dedupes on `request_id`). Production runs the durable variant: `DurableFrontier` is true when `EDGE_BASE_URL`, `EDGE_AUTH_TOKEN` and `QUARRY_INTERNAL_SECRET` are set (`ORCH/activities/activities.go:67`); the Ingestion compose sets `QUARRY_INTERNAL_SECRET` as required and the other two default to the runtime values (`Q/services/quarry-orchestrator/cmd/orchestrator/main.go:51-64`) [READ]. `CrawlJobInput` (workflows.go:75-87) carries Seeds, MaxDepth, MaxPages and an opaque `Policy` that nothing reads.
2. **[CLAIM-FALSE] The card's "reuse the per-page `page_failed` / `page_blocked` events (workflows.go:174-202)".** `runPage` classifies the activity error with `errs.Classify` (workflows.go:192-195), which does `errors.As(err, &*errs.Error)`. Errors returned by an activity cross Temporal as `*temporal.ActivityError` wrapping `*temporal.ApplicationError`; the `*errs.Error` struct does not survive, only the application error `Type()` string and `Details`. In a real worker `Classify` therefore never matches, `cat` stays `CategoryUnknown` and every failure is emitted as `page_failed` with `category:"unknown"` (the Temporal test mocks return `*errs.Error` directly, which is why no test shows it) [INF from reading `errs.go:132-151` (`Temporal()`), `191-198` (`Classify`) and the SDK semantics; confirm in slice 3 with a replayed real history]. Independently, `CategoryBlocklist` (the only category that maps to `page_blocked`, `errs.go:72-80`) has no producer: `FromHTTPStatus` (errs.go:161) maps 401/403 to `CategoryAuth`, and the edge answers a security block with HTTP 403 (`QC/error.rs:49`, `SecurityBlocked`). So a robots or SSRF block would surface as `page_failed`, category `auth` or `unknown`. The edge's own `PageBlocked` events (`QR/pipeline.rs:305-321`) go to the edge event sink, not into control's per-job event log that `/v1/jobs/{id}/events` serves. Adjacent finding for F-t2-017: after Temporal retries are exhausted the retryable categories still emit `page_retried` (errs.go:72-80), never `page_failed`.
3. **`links::extract` keeps everything.** `QT/links.rs:1-23` returns every `a[href]` resolved against the final URL, including `mailto:`, `javascript:`, `tel:` and external hosts. `InternalRunPageResult::from_output` (`QE/routes.rs:817-841`, links at 823) returns all hrefs as strings; `rel` and link text are dropped on the wire.
4. **`CrawlRequest` and `BatchRequest` are four-field structs** (`QE/routes.rs:532-553`: url, max_pages, max_depth, ingest, zdr; `669-679`: urls, ingest, zdr) with no `deny_unknown_fields`, so unknown scope keys are silently dropped. `max_pages` and `max_depth` absent or 0 mean unlimited (`ORCH/jobs/dispatcher.go:432-454` `uintFrom(nil)=0`; workflows.go:456 and 474 guard on `> 0`). A crawl with no cap is bounded only by the 30-minute `WorkflowExecutionTimeout` (`dispatcher.go:254`).
5. **The Model-proposable crawl action bypasses even the gateway normaliser.** `knowledge.crawl_site` (`FE/shared/actions/action-registry.ts:1320-1329`, input `crawlSiteInput` 568-571 = url, maxPages; `requiresApproval:false`) dispatches through `dispatch_crawl_site` (`GW/domains/actions/dispatchers.rs:314-360`), which builds `{url, org_id, max_pages, ingest}` by hand: no `normalize_public_http_url`, no 5000 clamp, so a Model-proposed crawl without `maxPages` is unbounded and unscoped. The UI path uses `normalize_crawl_body` (`GW/domains/knowledge/quarry.rs:386-416`, clamp 404) and forwards only url, max_pages, ingest. `/v1/crawl`, `/v1/batch` and `/v1/map` are unknown routes for service principals and fail closed (`QE/auth.rs:504-545`), so the Model reaches crawl only through the user's token via the gateway [READ]. That is the "Model proposes, Quarry executes" seam this design must keep.
6. **Dead scaffolding, proven.** `git grep` over `crates services pkg` finds no production caller of `CrawlFrontier` (only the `QR/lib.rs:112-113` re-export; `postgres_queue.rs:44,644` use only the checkpoint/config snapshot types, in a test), of `RobotsCache`/`ReqwestRobotsFetcher` (only `lib.rs:165` and their own module), of `CrawlDenialReason` (only the frontier and a doc comment, `QC/event.rs:55`), or of `SecurityEngine::check_discovered` (`quarry-security/src/preflight.rs:150-171`, tests only; it treats cross-origin as `Escalate`). `quarry_transform::robots::RobotsTxt` is used in exactly one production place, `QE/map_routes.rs:175,256`.
7. **`RobotsCache`/`ReqwestRobotsFetcher` are not safe to wire as written** (`QR/robots_cache.rs`): the fetcher is a plain `reqwest::Client` with a 10 s timeout (165-186), no SSRF guard, no DNS pinning, no redirect re-validation; it tries https then http (192-217) and treats every non-404 non-2xx and every transport error as `Ok(None)` = allow all (212-217): fail-open on 5xx, contrary to RFC 9309 (5xx or unreachable = complete disallow). The cache key is the host only (no scheme or port), the `in_flight` coalescing map is never pruned (101-107), the entry map has no size cap (51), and negative TTL is 1 h for both 404 and errors (66-67). [CLAIM-FALSE nuance] The card says "fetch errors fail open" as if deliberate; it is a side effect of `Ok(None)`.
8. **The guarded path the card points at is real but partly guarded.** `/v1/map` fetches through `state.driver.fetch` (`QE/map_routes.rs:118-128`); the default driver is `StaticDriver`, which has no unpinned client: every direct request is preflighted with `resolve_public_url` (`QR/dns_guard.rs:104`) and pinned (`QR/fetch.rs:151-197`), redirects are followed manually with per-hop re-validation up to 5 hops (`fetch.rs:38, 199-232`) [READ]. Gaps: (a) `map` itself pre-checks only `quarry_security::heur::check` (scheme, userinfo, literal private host; no DNS) at `map_routes.rs:152-162`; (b) the body is read with `resp.bytes()` and no cap (`fetch.rs:387`), so a robots.txt of any size is read into memory; (c) with default `FetchHints` the call is anonymous (`org_id:""`, default privacy) and can pick proxy slot 0 when a pool exists; (d) robots rules are matched with `p.path()` only (`map_routes.rs:256`), dropping the query string, so `Disallow: /*?session=` never matches; (e) the token matched is `QuarryBot` (`map_routes.rs:23`) while the static driver advertises `Quarry/2.0 (+https://triodelab.com/quarry)` (`QE/config.rs:302`, default) and the HTTP/3 driver `QuarryBot/1.0` (`QE/main.rs:1263`). Three user-agent strings, one robots token. `RobotsTxt::rules_for` matches the whole lowercased token exactly (`QT/robots.rs:91-94`), so a full UA string never matches a named group.
9. **The frontier's matching is glob/substring, not regex.** `QR/crawl_frontier.rs:104-146` (`pattern_matches`: `*`/`?` glob, otherwise substring; linear-time two-pointer matcher). The audit's ReDoS caps assume regex; the existing code is linear by construction. The frontier also treats every subdomain as in scope unconditionally (87-101), unlike `/v1/map` where subdomains need `include_subdomains` (`map_routes.rs:78-83`).
10. **`DiscoveryPolicy.respect_robots` is a determinism stamp that lies.** `QR/policy.rs:58-80` defaults `respect_robots: true`; no code reads it during execution (D7 section 2 item 4 confirms) yet it is hashed into the artifact policy fingerprint, so artifacts claim a robots posture that was never enforced. Control's `fast` preset sets `Robots: RobotsIgnore` (`CTL/resources/presets.go:69`, per D7) and the stored job policy is never forwarded to `run_page`.
11. **Workflow replay safety and history budget.** There is no `workflow.GetVersion` and no `ContinueAsNew` anywhere in the orchestrator (`grep` empty). Each activity costs at least 3 history events; the durable loop runs a `FrontierEnqueue` activity per link, so a page with 200 links costs 600 or more events. Temporal's default per-workflow limits are 51,200 events and 50 MB (warn at 10,240) [UNVERIFIED for this deployment's dynamic config]; a durable crawl of a link-dense site would reach the event limit after roughly 80 pages [INF, arithmetic only]. `RunPageResult.Links` (all hrefs) is also recorded in history for every page. No workflow test sets `DurableFrontier = true` (workflows_test.go has no reference), so the production loop has zero test coverage; the unit tests exercise only the in-workflow BFS.
12. **ZDR.** Crawl and batch are refused: `refuse_durable_job_under_zdr` (`QE/routes.rs:563-588`, called at 596 and 687). [DRIFT] The card says "`run_page` carries no zdr field": the edge's `InternalRunPage` does accept `zdr: Option<bool>` (`routes.rs:751-752`, read at 969) and `PageRunner` honours it; what is missing is the orchestrator side (`RunPageInput` activities.go:165-181 has no zdr, the body at 213 sends none). The refusal comment at 555-562 is accurate. Lifting the refusal is much larger than carrying the flag: the control job record stores `params` (URLs, `CTL/store/store.go:174-212`), Temporal history stores URLs, excerpts and summaries (`pageExtractedPayload`, workflows.go:234-267), control's event log stores `page_extracted` excerpts, the Postgres frontier queue stores URLs, and checkpoints and idempotency keys derive from URLs.
13. **Batch is unchecked at the edge.** `batch_handoff` (`QE/routes.rs:681-725`) forwards `req.urls` verbatim: no count cap, no URL parse, no scheme or heuristic check, no dedupe. The gateway does all of it for UI callers (`GW/domains/knowledge/quarry.rs:423-455`: `normalize_public_http_url`, dedupe, cap 200), so only direct API callers see the hole. Per-URL preflight runs later in `run_page` (`QR/pipeline.rs:305`).
14. **Docs claim all of this is closed.** See 2.3.

### 2.2 What is built and what is wired

| Piece | Where | Production caller today | After this design |
|---|---|---|---|
| `CrawlDenialReason` (11 typed codes + `message()`) | `QC/crawl_denial.rs:12-83` | none | yes: every denial in run_page, events, summaries |
| `CrawlFrontier`, `FrontierConfig`, `pattern_matches` | `QR/crawl_frontier.rs` | none (checkpoint types only in a test) | `pattern_matches`, `normalize_url` and host rules move into a shared evaluator that `CrawlFrontier::enqueue` also calls; the stateful `CrawlFrontier` itself stays unused until Rust crawl workers exist (ROADMAP Phase 5) or is deleted by that decision |
| `RobotsCache` | `QR/robots_cache.rs` | none | yes, behind `RobotsGate`, with a guarded fetcher |
| `ReqwestRobotsFetcher` | same | none | removed (replaced by `DriverRobotsFetcher`); leaving it invites a second SSRF hole |
| `RobotsTxt` parser | `QT/robots.rs` | `/v1/map` only | also the gate; `Crawl-delay` consumed for the first time |
| `quarry_transform::sitemap` | `QT/sitemap.rs` | `/v1/map` only | unchanged here (F-t2-041 seeds crawls from it) |
| `HostScheduler` (AIMD per host) | `QR/host_scheduler.rs:130-290` | wired in every `PageRunner::run_inner` (pipeline.rs:344-348, 392-400) | gains a min-interval so `Crawl-delay` is honoured |
| `SecurityEngine::check_discovered` | `quarry-security/src/preflight.rs:150-171` | none | stays unused (its escalate-on-cross-origin semantics do not match a typed denial); noted so nobody wires it by accident |

### 2.3 Stale docs and rows to correct (located)

All in `Q/docs/` unless stated; verified by opening each line.

| File (tag) | Lines | Claim | Reality |
|---|---|---|---|
| `GAP.md` [C] | 176 | "Include / exclude paths ... Go orchestrator filters sitemap + discovered URLs with path glob, path regex (`re:`) and full-URL (`re_url:`) rules - Closed" | [CLAIM-FALSE] no such Go code; the `re:`/`re_url:` grammar exists only in this row (this design adopts it, section 3 D3) |
| `GAP.md` [C] | 177, 201 | sitemap-guided crawl via "runtime-backed sitemap activity" | [CLAIM-FALSE] no sitemap activity; crawl never reads a sitemap |
| `GAP.md` [C] | 178, 202 | robots respected, `page.blocked` on denial | [CLAIM-FALSE] no robots check on crawl or batch |
| `GAP.md` [C] | 179, 180 | "blocks cross-host links by default", "`allow_subdomains=true`" | [CLAIM-FALSE] external links followed; the fields do not exist |
| `GAP.md` [C] | 292 and 419 (duplicate rows) | "`CrawlJobWF` now seeds from sitemap, checks robots before `RunPage`, emits `page.blocked`... targeted workflow tests pass" | [CLAIM-FALSE]; no such test exists in `workflows_test.go` |
| `GAP.md` [C] | 316 | "mirrors ... in `crates/quarry-runtime/src/robots.rs`, wired into the live orchestrator crawl path through quarry-edge internal endpoints" | `robots.rs` does not exist in quarry-runtime (only `robots_cache.rs`, unwired); no such endpoints |
| `GAP.md` [C] | 431 | "Redis-backed robots cache exercised through the internal robots check endpoint" | in-memory `HashMap` (robots_cache.rs:49-51), no endpoint, no Redis |
| `GAP.md` [C] | 494 | Phase 5 gate says "the now-closed sitemap/robots/path-policy slice" | not closed |
| `GAP.md` [C] | 725 | lists `/v1/internal/fetch_sitemap`, `/v1/internal/check_robots` as live | neither route exists (`git grep` empty) |
| `Features.md` [M] | 27-31 | "bounded crawl with robots.txt respect, sitemap discovery ... powered by `crawl_frontier`, `robots_cache`" | neither is wired. The live diff in this file touches lines 39 and 179 only, no overlap |
| `gap-quarry.md` [C] | 219 | "Quarry enforces SSRF, robots, allowed domains ..." | robots not enforced outside `/v1/map` |
| `ARCHITECTURE.md` [C] | 173-175 | `HostSlot` has a "robots crawl-delay floor"; `DenialReason` "structured reason for skipped links" as live | no delay floor exists (`host_scheduler.rs:105-123`); `CrawlDenialReason` unused |
| `CONTRACTS.md` [C] | 352-368, 427-431 | documents `include_paths`, `exclude_paths`, `allow_subdomains`, `ignore_robots_txt`, and denial codes `depth_limit`, `robots_txt`, `exclude_pattern`, `external_link` ... | none accepted by code; the code's serde tags differ (`depth_exceeded`, `robots_disallowed`, `exclude_pattern_hit`, `external_host_disabled`). Decision D6 picks the code's vocabulary and documents a mapping |
| `POLICY.md` [C] | 23 | `DiscoveryPolicy` pins `respect_robots` as if enforced | stamp only (2.1 item 10) |
| `openapi.yaml` [M] | 4361-4385 | `CrawlRequest` lists url and max_pages; `BatchRequest` urls; `InternalRunPage` 4387-4410 omits `user_id`, `privacy`, `render`, `prefer_http3` | incomplete; 78 lines of live diff elsewhere in this file |
| `ROADMAP.md` [C] | 36-38 | Phase 5 bullet is the plan | accurate; keep as the pointer, add a status line |

Add all rewritten docs to `apps/STALE_DOC_DELETION_REGISTER.md` as `update` (audit open question 15).

### 2.4 Uncommitted ([M]/[U]) files in the area and conflicts

| File | Tag | In-flight diff | Conflict with this work |
|---|---|---|---|
| `QE/routes.rs` | [M] | 1 line (test `AppState` literal, `model_plane: Default::default()` at about 1329) | low: this design edits `CrawlRequest` 532-553, `crawl_handoff` 590-667, `batch_handoff` 681-725, `InternalRunPage` 743-769, result 771-841, `internal_run_page` 898-1062 and the three `PageRunner {` literals (449, 989, 1104); only the test literal is adjacent |
| `QE/state.rs` | [M] | 21 lines (`model_plane` field and doc rewrites) | additive field `robots`; rebase, do not merge blind |
| `QE/main.rs` | [M] | 188 lines (model-plane gating, prod checks) | additive: gate construction before the `AppState` literal at about 1431; the literal gains one line |
| `QE/config.rs` | [M] | 187 lines | none: crawl knobs are read by a new `crawl_policy.rs` from env and fail boot on garbage (D7 rationale: `EdgeConfig` silently falls back to defaults on a deserialize error), so this file is not touched |
| `QE/lib.rs` | [M] | 1 line (`pub mod model_plane_gate;`) | adjacent: new `pub mod crawl_request; pub mod crawl_policy;` |
| `QR/lib.rs` | [M] | 6 lines (`data_plane_http`, rerank re-exports) | adjacent at 42 and 160-165: new `pub mod crawl_filter; pub mod robots_gate;` and a changed `robots_cache` re-export |
| `Q/crates/quarry-edge/tests/support/mod.rs` | [M] | 1 line (`model_plane: Default::default()`) | additive `robots: None` line |
| `Q/docs/openapi.yaml` | [M] | 78 lines (rerank/data-plane) | additive schemas; keep the `/v1/search` block the docs-contract test pins |
| `Q/docs/Features.md` | [M] | lines 39 and 179 | none |
| `FE/features/dashboard/home/KnowledgeComposer.tsx` | [M] | 10 lines | possible: slice 7 edits the job-summary text near the crawl job row |
| `Q/scripts/tests/docs-contract.test.mjs` and `.github/workflows/quarry-docs-contract.yml` | [U] | whole files | only `/v1/search` is asserted for openapi; run `node scripts/tests/docs-contract.test.mjs` after the openapi edit |
| `apps/Ingestion Plane/integration-corev2/internal/controlplane/clients.go` | [M] | in-flight | read-only reference for the audit client contract (slice 8) |

Everything under `ORCH/`, `CTL/`, `QT/`, `QC/`, `QE/map_routes.rs`, `QE/queue_routes.rs`, `GW/domains/knowledge/quarry.rs`, `GW/domains/actions/dispatchers.rs` and `FE/shared/actions/action-registry.ts` is [C]: no in-flight diff, so Go and gateway work has no merge risk from uncommitted changes (the cross-workstream hotspots are in section 5).

## 3. Design decisions

Format: decision, options, recommendation, rationale. "ADR-GATED" names the audit section 9.3 topic. "DECISION" marks a product choice that is not one of the ten ADR topics but needs a user sign-off before the slice that depends on it ships.

### D1. How the Go workflow reaches Rust filter and robots logic (edge endpoint vs activity helper)

Options:
- **O1. Filter inside the Go workflow.** Rejected: duplicates frontier logic that ROADMAP Phase 5 keeps in Rust; Go `net/url` and Rust `url` disagree on host normalisation (case, trailing dot, IDNA, userinfo), which is exactly where a scope check is bypassed; Go cannot see the robots cache or the scheduler.
- **O2. New edge endpoint `POST /v1/internal/crawl/filter` plus a Go `FilterLinks` activity.** Works, but costs a second activity (3 history events) and a second HMAC-signed round trip per page, and `RunPageResult.Links` (unfiltered, up to hundreds of URLs) would still be recorded in Temporal history.
- **O3. Extend the existing `/v1/internal/run_page` call.** The request gains an optional `crawl` object (scope, depth, robots mode, seed hosts); the edge fetches the page, then filters the extracted links through a pure Rust evaluator and returns only allowed links plus a typed denial report. No new activity, no new endpoint, and the Temporal payload shrinks (denied links are never recorded).
- **O4. Apply scope at the queue-enqueue route** (`/v1/internal/queues/:q/enqueue`, already Rust). Covers only the durable loop, not the in-workflow fallback, and cannot carry the denial report back.

**Recommendation: O3**, with the evaluator written as a pure library function (`quarry_runtime::crawl_filter`) so the future Rust crawl workers (ROADMAP Phase 5) call it directly. Robots enforcement is a separate, authoritative check at fetch time inside `PageRunner` (D4), so a URL that never passed through link filtering (seeds, batch URLs, links enqueued by an older orchestrator) is still checked. Rationale: smallest change to the production hot path, fixes the history-size problem, keeps frontier logic in Rust, and degrades safely (D7).

### D2. Default scope, and how existing crawls change deliberately  (DECISION)

Today: no scope. Recommended default for a request that sends no scope fields:
- **Same host, `www.`-insensitive.** A link is in scope when its host (lowercased, trailing dot removed, IDNA-normalised by the `url` crate) equals a scope host after stripping one leading `www.` from both sides. Rationale: apex-to-www redirects are ubiquitous; without this every link on `www.example.com` is "external" to a seed of `example.com`.
- **Redirect adoption at depth 0 only.** If a seed page's final URL lands on a different host, that final host joins the scope hosts (the edge returns `hosts_added`; the workflow carries them). A later redirect to another host is not adopted. The redirect target already passed the SSRF re-validation in `StaticDriver::do_fetch`.
- **Subdomains opt-in** (`allow_subdomains`), and **external links opt-in** (`allow_external_links`). `allow_external_links:true` reproduces today's reach (any host, subdomains included; `allow_subdomains` is then moot), so it is the user-level legacy opt-out. The scheme and extension denials and the page cap below still apply to it; only `QUARRY_EDGE__CRAWL_SCOPE_MODE=off` (the operator rollback) reproduces the old behaviour byte for byte.
- **Whole host, not directory prefix.** Firecrawl defaults to the seed's directory; the card warns that a directory-prefix default misses sibling sections users expect. Offer it as `crawl_entire_domain:false` (Firecrawl name; default `true` here, the opposite of Firecrawl's default, stated in the docs).
- **Non-web and binary links are denied by default**: schemes other than http(s) (`not_crawlable`), fragment-only links, and a default extension denylist (images, video, audio, archives, fonts, css, js; `content_type_rejected`). PDF, DOCX and similar stay allowed because D5 now routes them. This also stops D5's new 415 on images from inflating `pages_failed`.
- **Scope never filters seeds.** Seeds are the user's explicit input (robots and SSRF still apply to them).

Rollout is explicit, not silent: one operator switch at the edge, `QUARRY_EDGE__CRAWL_SCOPE_MODE=off|observe|enforce`.
- `off`: the edge stamps `scope.legacy=true` on the job, sends no `crawl` context, behaves as today (the rollback switch; needs an edge restart, no orchestrator change).
- `observe`: the filter runs, denial counts are reported in `run_completed`, but links are not dropped. This is how a team measures how many existing crawls would change before flipping.
- `enforce`: links are dropped. First release ships `observe`; a config flip, not a code change, moves to `enforce` after the soak; a later PR changes the code default.

Jobs already accepted in control before the deploy carry no `scope` param: the dispatcher treats an absent `scope` as `legacy` (they were accepted under the old contract). Every job created after the deploy carries an explicit effective scope stamped by the edge, so the orchestrator never decides a default. The effective scope is echoed to the caller in `HandoffAck.effective` and in the `run_started` event payload.

Other deliberate default changes, listed so none is a surprise: an absent `max_pages` becomes `QUARRY_EDGE__CRAWL_DEFAULT_MAX_PAGES` (default 1000, the existing `FrontierConfig` default) instead of unlimited; `max_pages:0` becomes a 400 instead of unlimited; hard cap `QUARRY_EDGE__CRAWL_MAX_PAGES_CAP` (default 10,000). `max_depth` absent stays unlimited (bounded by the page cap). Budget clamping to remaining credits is D6's (budgets design), not this one.

### D3. Pattern grammar and ReDoS caps

Options: glob only (current frontier, linear by construction); regex only (Firecrawl, matches the audit text); both with an explicit grammar. **Recommendation: both, using the grammar the stale GAP.md row already promised**, so no new vocabulary is invented:
- plain entry: glob on the URL **path and query** (`*` any run, `**` same as `*`, `?` one char), anchored; an entry with no metacharacter is a path prefix (not the frontier's substring match; the frontier keeps substring for its legacy tests);
- `re:<regex>`: Rust `regex` crate on the path+query;
- `url:<glob>` and `re_url:<regex>`: same, on the full normalised URL.

ReDoS control: the `regex` crate has no backtracking (linear-time matching), and is built with `RegexBuilder::size_limit(256 KiB)` and `dfa_size_limit(1 MiB)`; the glob matcher is the existing two-pointer one. Caps enforced at the edge before the job exists, 400 `invalid_pattern` with `fields[i]`: **at most 100 entries across include and exclude, 512 characters each, 8 KiB total**. This is deliberately tighter than the audit's Firecrawl numbers (1000 entries, 2000 each, 100k total): the scope travels in every `run_page` activity input and Temporal records every activity input in history, so a 100k scope times a few thousand pages is tens of megabytes. Evidence of the budget: slice 3 asserts history bytes per page. If real users need Firecrawl's numbers, the follow-on is scope-by-reference (D15), not raising the caps. Compiled patterns are validated at handoff and again at first use (a bad pattern from an older job record fails the page with a typed 400, not a panic).

### D4. Robots semantics, placement, cache and identity  (ADR-GATED, topic 5 for the unreachable posture and the identity token; open question 11 for manual browsing)

Placement. Two layers, one source of truth (`RobotsGate`, section 4 slice 4):
1. **Authoritative: fetch time, inside `PageRunner::run_inner`**, after `security.preflight` (pipeline.rs:305) and before DNS resolution and the scheduler slot (327-348). Covers seeds, every crawl child and every batch URL, and any URL an older orchestrator enqueued. A denial returns a typed `SecurityBlocked` error with `details.denial` and emits `PageBlocked` through `emit_page_event` (ZDR-aware). It must be before the scheduler slot because the scheduler maps unknown error codes to `BadKind::Server` (pipeline.rs:371-377) and would halve the host's concurrency on a policy denial.
2. **Discovery time, in the link filter, against the cache only** (`RobotsGate::peek`, never a network call): children whose host's robots rules are already cached are denied before they are enqueued, which keeps blocked URLs out of the queue and out of Temporal history. In the default same-host scope the page itself was just fetched under the gate, so the cache is warm and this costs nothing; for external-link crawls the fetch-time layer is the guarantee.

Semantics (RFC 9309), with the audit's request to make the failure mode configurable:
- 2xx: parse, honour. Body capped at 512 KiB (the RFC minimum is 500 KiB); the rest is ignored. The cap is enforced by `FetchHints.max_body_bytes` (D5 slice 1 introduces it; today `resp.bytes()` is unbounded, fetch.rs:387) plus a `truncate_at_cap` flag so the robots fetch keeps the first 512 KiB instead of failing.
- 4xx (including 401, 403, 404, 410): "unavailable", allow everything, cached 1 h.
- 5xx, network error, timeout, oversize redirect chain: "unreachable". Two attempts within 8 s, then **disallow** with typed code `robots_unreachable`, cached for 5 minutes only. `QUARRY_EDGE__ROBOTS_UNREACHABLE=disallow|allow` selects the posture; default `disallow` (RFC). Firecrawl fails open and ignores Crawl-delay; this design is stricter on the first and follows the references on neither, which is the compliance posture the audit risk 7 calls for. The recommendation needs a sign-off because a flaky robots endpoint will now stop a crawl; the soak in `observe` shows how often.
- Matching: the path-and-query of the URL, not path only (fixes map_routes.rs:256); the token set is **`["quarrybot", "quarry"]`** (the union of the two product tokens the fetchers advertise today), evaluated most-restrictive-wins, held in one constant that F-t2-067 will replace. `rules_for` (robots.rs:91) already falls back to `*`.
- Cache: keyed by origin (`scheme://host:port`), positive TTL 24 h, bounded to 10,000 entries (`QUARRY_ROBOTS_CACHE_MAX_ENTRIES`), the in-flight coalescing map pruned after each fetch, in memory only. robots.txt is public data shared across orgs; nothing about an org's content enters it. For a ZDR caller (only `/v1/map` can be one, since crawl and batch are refused) the gate uses `check_uncached`: fetch, evaluate, do not insert, so a ZDR org's hostnames are never held in process memory beyond the request.
- **The fetch is guarded by construction**: `DriverRobotsFetcher` calls `Driver::fetch_conditional` on the edge's default `StaticDriver`, so DNS pinning, redirect re-validation and the public-address check are inherited, not re-implemented. It passes the caller's `org_id` and a default-deny privacy policy in `FetchHints` so it never uses a third-party proxy. `ReqwestRobotsFetcher` is deleted.
- `Crawl-delay` feeds the existing scheduler: `HostScheduler::set_min_interval(host, delay)` with the delay clamped by `QUARRY_CRAWL_DELAY_MAX_S` (default 10, ceiling 60). It spaces request starts per host in this edge process; with several edge replicas each applies it independently (document).
- Human browsing (the Verevon browser pane) is out of scope here. `RobotsGate::check` is the API the agent path (`validate_navigation`, agent_routes.rs:109-133) can call later; whether a human's own navigation is exempt is open question 11 and ADR topic 3.

### D5. Typed denials: transport, classification, events

- **Fetch-time denial** (robots, or later policy): the edge returns HTTP 403 with the standard error envelope and `error.details.denial = {code, message, remedy, user_agent?, path?}` (the serialised `CrawlDenialReason` plus remedy). 403 keeps today's non-retry behaviour for `SecurityBlocked` (QC/error.rs:49).
- **Discovery-time denial**: returned in the 200 response of `run_page` as `crawl.denials` (counts per code, plus up to 20 sample URLs per code per page, plus the robots-class denials as a separate capped list).
- **Go side**: a new `errs.FromHTTPResponse` parses the envelope; a body with `details.denial` yields `CategoryBlocklist` (non-retryable) and the denial rides in the `ApplicationError` details. A new `errs.CategoryOf(err)` reads `ApplicationError.Type()` and details so the **workflow can classify** (fixing 2.1 item 2). `runPage` then emits `page_blocked` with `{url, category:"blocklist", code, message, remedy}` and the loops count `pages_blocked`, not `pages_failed`.
- **No new event types.** The event list is frozen (`CONTRACTS.md:55-80`; adding one changes Go, Rust, TS and openapi). Per-URL robots denials use `page_blocked` (idempotency key already `runID:url:page_blocked`, so one URL denied from many parents emits once; the workflow also keeps a bounded `deniedSeen` set to skip the activity call). Scope-class denials are counted, not emitted per URL: the aggregate goes into the terminal event payload `denials: {counts, samples}` (samples capped at 10 per code, 12 codes) and `pages_blocked`, `links_denied` counters. The per-run cap on per-URL `page_blocked` events is 200.
- **Status and errors routes are F-t2-017's.** This design fixes the data they read: `page_blocked` events and the `run_completed` summary. Control's `jobEvents` has no `type` filter (resources.go:557-577), so F-t2-017's `/errors` route must page and filter; until it ships the UI reads the summary from the SSE stream it already consumes (`knowledge-client.ts` `normalizeCrawlEvent`).

### D6. Denial code vocabulary

The code's serde tags are canonical (`out_of_scope`, `robots_disallowed`, `depth_exceeded`, `max_pages_reached`, `duplicate`, `external_host_disabled`, `security_rejected`, `include_pattern_miss`, `exclude_pattern_hit`, `content_type_rejected`, `not_crawlable`), plus one additive variant `robots_unreachable { host }`. They are typed, round-trip tested and unique-tested; the `CONTRACTS.md` names are not implemented anywhere. `CONTRACTS.md:427-431` is rewritten with a mapping table (`robots_txt` to `robots_disallowed`, `external_link` to `external_host_disabled`, `exclude_pattern` to `exclude_pattern_hit`, `depth_limit` to `depth_exceeded`, `file_type` to `content_type_rejected`, `url_parse_error` and `non_web_protocol` to `not_crawlable`, `section_link` to `out_of_scope`; `backward_crawling` and `social_media` not implemented). A new `CrawlDenialReason::remedy() -> Option<&'static str>` names the option the caller can change, correcting Firecrawl's flaw that no message names `allowSubdomains`: `external_host_disabled` names `allow_external_links`, `out_of_scope` for a subdomain names `allow_subdomains`, section scope names `crawl_entire_domain`, include/exclude name the pattern and field, `robots_disallowed` says the site disallows the user agent and that the only override is the audited org policy (never the Model).

### D7. Making the workflow change safe (replay, version skew, history)

- **Replay.** All new workflow behaviour (new activity inputs, batch enqueue, `crawl` context, new payload fields, new counters) sits behind `workflow.GetVersion(ctx, "crawl-scope-v1", workflow.DefaultVersion, 1)`. Executions started before the deploy replay the legacy branch. The 30-minute `WorkflowExecutionTimeout` means old executions drain within half an hour, after which the legacy branch can be deleted in a follow-up PR. Verified by a replay test (`worker.NewWorkflowReplayer`) against a history exported from the pre-change build on the running stack.
- **Version skew, orchestrator ahead of edge.** An old edge ignores the unknown `crawl` field (no `deny_unknown_fields`) and returns every link unscoped. The new response always carries `crawl.scope_applied:true`; if scope was requested and the field is absent the workflow does not enqueue children and ends the run `run_failed` with `code:"edge_scope_unsupported"`. Fail closed, loud, brief (compose rolls both together). Edge ahead of orchestrator is the status quo (no `crawl`, legacy links).
- **Batch enqueue.** The durable loop replaces N `FrontierEnqueue` activities with one `FrontierEnqueueBatch` (new edge route `/v1/internal/queues/:q/enqueue_batch`, same `X-Quarry-Queue-Sig` binding, at most 500 items, returns the count the queue accepted), cutting history by roughly 3 events per link and making the denial path affordable.
- **No `ContinueAsNew`** here: it belongs to F-t2-017 (job lifecycle); this design only reduces per-page cost and records the history-bytes-per-page assertion so the next owner has a baseline.

### D8. Request vocabulary on `CrawlRequest` (the ScrapeOptions-backed shape)

`CrawlRequest` v2 (snake_case, camelCase aliases via `serde(alias)` because the gateway and Firecrawl-style clients send camelCase; same lenient-then-strict validation pattern as D5's scrape request: handler takes `Json<Value>`, collects `FieldError {path, code, message}`, unknown keys are warnings until `QUARRY_EDGE__CRAWL_REQUEST_VALIDATION=strict`; reuse D5's helper if merged, do not fork it):

| Field | Meaning | Default | Notes |
|---|---|---|---|
| `url` | seed | required | unchanged |
| `max_pages` (alias `limit`) | page cap | 1000 | 0 is 400; cap 10,000 |
| `max_depth` (alias `max_discovery_depth`) | discovery depth | unlimited | |
| `include_paths`, `exclude_paths` | pattern lists (D3) | empty | caps in D3 |
| `allow_subdomains` | subdomains in scope | false | |
| `allow_external_links` | follow other hosts | false | UI-only; not in the Model-visible schema |
| `crawl_entire_domain` | false = seed directory only | true | |
| `sitemap` | `skip` or `include` or `only` | `skip` | only `skip` accepted until F-t2-041; others 400 `unsupported_option` naming F-t2-041 |
| `ignore_query_parameters` | canonicalisation | reserved | 400 `unsupported_option` until F-t2-041 |
| `ignore_robots_txt` | request override | false | policy-gated (D9); 403 `policy_denied` otherwise |
| `scrape_options` | embedded `ScrapeOptions` (D5 Epic E1) | absent | accepted only once D5's type exists and only with `formats` restricted to refs (D5 D18: inline content must never enter Temporal history); 400 until then |
| `ingest`, `zdr` | unchanged | | `zdr` still refused |
| rejected from day one | `headers`, `proxy`, `proxy_url`, `skip_tls_verification`, `stealth`, `mobile`, `webhook` (F-t2-031), `delay`, `max_concurrency` | | D5's reserved denylist plus fields that would otherwise be silently ignored |

The Model-visible action schema (`crawlSiteInput`) gains only `maxDepth`, `includePaths`, `excludePaths`, `allowSubdomains` (and the existing `maxPages`). It never gains an external-links or robots key; `dispatch_crawl_site` is rewritten to go through the same normaliser as the UI path, which builds a fresh JSON from an allowlist, so an injected key cannot reach the edge. The edge refuses `ignore_robots_txt` for any service principal as a second line.

### D9. Ignore-robots as an audited Control Plane entitlement  (ADR-GATED, topic 1 owner of web policy; open question 11 who may hold it)

Reuse D7 rather than add a second mechanism. D7 already defines the ceiling `robots.max_mode` (`strict < respect < ignore`, default `respect`), the org-core entitlement `web.robots_override` folded into the signed `web_policy` claim, org-core audit rows on every change, and the rule that a Model can only ask to tighten (`RequestAsk` is tighten-only and unrepresentable otherwise). D14a adds:
- **Enforcement without override first.** Slices 2 to 7 ship with `ignore_robots_txt` always refused (403 `policy_denied`, `details.capability:"robots.max_mode"`). Nothing depends on an override today, because robots is not enforced at all.
- **Use is audited, fail closed.** When D7's `ResolvedPolicy` allows `ignore`, the edge stamps `params.robots = {mode:"ignore", authority:"web.robots_override", rev}` and `params.user_id`; quarry-control `createJob` writes an `audit-core` event (`plane:"ingestion"`, `producer:"quarry-control"`, `event:"crawl.robots_ignored"`, `resource_id: job_id`, details `{seed_host, authority, rev}`) **before** acknowledging the job; if the audit write fails the job is not created (503). Rationale: an override that cannot be audited must not run. Producer is quarry-control, not the internet-facing edge, so the edge never holds an audit-writer credential. audit-core already pins one ingestion writer (`integration-corev2`, `audit-core/internal/api/service_auth.go:147-155`); a second principal pinned to `planes:["ingestion"]` is a Control Plane config change plus a test.
- **Grantor.** Proposed default (D7 Q3): platform operators only; an org admin cannot self-grant. Not decided here.
- **The Model never gets it.** The action schema has no key, the gateway allowlist drops it, service principals cannot call `/v1/crawl` (auth.rs:504-545), and the edge refuses it for service principals. Tests pin all three.

### D10. ZDR for the durable orchestrator path  (ADR-GATED, topic 4; recommendation: keep the refusal)

Options: (a) keep refusing crawl and batch under ZDR; (b) carry `zdr` to `run_page` and run durable crawls with no durable artifacts or events; (c) a client-driven inline crawl (the caller holds the frontier). **Recommendation (a).** The orchestrator side of (b) is a few lines, but the durable path would still retain the ZDR caller's data in five places that have no ZDR mode: the control job record (`params` hold URLs), Temporal history (URLs, excerpts, summaries), control's event log, the Postgres frontier queue, and run checkpoints (2.1 item 12). Making those hash-only or ephemeral is an XL redesign that is also F-t2-018's problem. Do not loosen the refusal; D4's S-8 adds `details.zdr_refusal.code = zdr_durable_mutation_forbidden` without changing status or text. What a ZDR user can do today stays: `/v1/map` (read-only, inline) and single scrapes. New fields (`scope`, `robots`, `scrape_options`) are never parsed under ZDR because the refusal happens first (test pins it). `RobotsGate` uses `check_uncached` for ZDR callers (D4) and logs hosts never. Option (c) is the only credible ZDR crawl and is out of scope.

### D11. The batch check

Edge `batch_handoff` gains static validation only (no network, so no amplification and no latency): cap `QUARRY_EDGE__CRAWL_BATCH_MAX_URLS` (default 500; the gateway keeps 200), parse each URL, http or https only, `quarry_security::heur::check` (literal private hosts, userinfo), dedupe on the normalised form, and a typed `rejected: [{url, code, message}]` in the ack (`not_crawlable`, `security_rejected`, `duplicate`). All rejected is a 400; some rejected is a 200 with the list. Robots for batch URLs is the fetch-time gate in `run_page` (D4 layer 1) with `crawl.kind:"batch"`, `crawl.robots:"respect"` and no scope, so batch pages come back with an empty `links` list (smaller history). A handoff-time robots fetch for up to 500 arbitrary hosts is rejected: it makes the edge fetch 500 hosts inside one HTTP request (latency, amplification) and races robots changes anyway. `BatchJobWF` counts `pages_blocked` and reports it. Hook for D7's `check_url` (domain deny) is left at the same place.

### D12. Crawl-delay and the 30-minute workflow cap

Honouring a 10-second delay caps a crawl near 180 pages per 30 minutes, then the `WorkflowExecutionTimeout` (dispatcher.go:254) terminates it with no `run_completed`. The run_page response carries `crawl.crawl_delay_ms`; the workflow adds it to the `page_fetched` payload so ops and UI can see why a crawl is slow. Raising or removing the 30-minute cap is F-t2-017's decision. Interim mitigation in this design: the clamp default of 10 s and a documented note; no automatic page-cap adjustment.

### D13. What happens to `CrawlFrontier` and `check_discovered`

`CrawlFrontier` is stateful in-memory (VecDeque, seen set, visited counter, unbounded `denials` Vec) and the production frontier is the Postgres queue plus Go BFS state, so wiring it per request would give it an empty seen set and a wrong page counter. Recommendation: extract its pure rules (host scope, `pattern_matches`, `normalize_url`) into the shared evaluator and make `CrawlFrontier::enqueue` call it, with a differential test proving the two agree; leave `CrawlFrontier` in place for the Phase 5 Rust workers and mark the decision "wire when crawl workers move to Rust, otherwise delete" in the ROADMAP status line. `SecurityEngine::check_discovered` stays unused.

### D14. How to prove it end to end when the SSRF guard blocks every local fixture

The static driver's guard is unconditional: `with_allow_private_hosts(true)` configures only the preflight engine, not the connection-level `resolve_public_url` (`Q/crates/quarry-edge/tests/sse_stream.rs:13-27` documents this, and why that test stubs the driver). So an e2e crawl cannot target `127.0.0.1` or a Docker bridge address (RFC 1918, TEST-NET documentation ranges, benchmarking `198.18.0.0/15` and CGNAT are all blocked, `quarry-security/src/heur.rs`, `ipv4_blocked_reason` and `ipv6_blocked_reason`). Recommendation, two layers and no hole in the guard: (1) hermetic Rust test with an in-memory `FixtureSiteDriver` (a `Driver` serving a site graph: seed page, internal pages, an external link, a subdomain link, `/private/` disallowed in `robots.txt`, a `Crawl-delay`), the established pattern; (2) a compose overlay `deploy/compose/docker-compose.crawl-fixture.yml` [NEW] that creates a Docker network with subnet `192.88.99.0/24` (the deprecated 6to4 anycast block, not in the blocked table, never routed off the bridge), attaches the edge to it, and runs two nginx fixture containers with DNS aliases `fixture-a.crawl.test` and `fixture-b.crawl.test`. The real `StaticDriver` resolves the alias via Docker DNS to a "public" address and the real guard passes. Negative assertions come from the fixture containers' access logs (zero requests to fixture-b, zero to `/private/`). [UNVERIFIED] that Docker Desktop on Windows accepts that subnet; fallback: any other unblocked, non-overlapping /24.

### D15. Scope by reference (not now)

If the 8 KiB scope budget proves too tight, the activity input carries only `job_id` and a scope digest and the edge resolves the compiled scope from control's job record (`GET /v1/jobs/{id}?org_id`, HMAC-signed, cached per edge replica for 30 minutes). Costs a control dependency on the page hot path (fail closed on outage). Recorded so nobody raises the caps instead.

## 4. Slices (PR-sized, in build order)

Order: 0 docs and characterization, 1 fixture harness, 2 Rust filter and `run_page` context, 3 Go scope default (the audit's "step 1") with denial plumbing, 4 robots gate (the audit's "step 2"), 5 batch check, 6 vocabulary (the audit's "step 3"), 7 ignore-robots entitlement and audit (blocked), 8 rollout closeout. Slices 2 and 4 are Rust-only and inert until the orchestrator or the request opts in, so they can merge ahead of the behaviour change.

### Wire contracts introduced (referenced by the slices)

`/v1/internal/run_page` request, new optional `crawl` object (Rust `quarry_core::crawl_scope::CrawlContext`; Go mirror `activities.CrawlContext`):

```json
"crawl": {
  "v": 1,
  "kind": "crawl",                     // "crawl" | "batch"
  "depth": 2,                          // depth of the URL being fetched
  "max_depth": 0,                      // 0 = unlimited
  "seed_hosts": ["example.com"],       // lowercase, from the job; grows with hosts_added
  "scope": { "allow_subdomains": false, "allow_external_links": false,
             "crawl_entire_domain": true, "include_paths": [], "exclude_paths": [],
             "deny_extensions": null,  // null = built-in default list
             "legacy": false },
  "scope_mode": "enforce",             // "enforce" | "observe"
  "robots": "respect"                  // "respect" | "ignore" (ignore is policy-gated, slice 7)
}
```

`/v1/internal/run_page` response, new optional `crawl` object (`CrawlReport`); `links` carries only allowed, normalised links when `crawl` was sent and `scope_mode` is `enforce`, an empty list for `kind:"batch"`, and is unchanged when `crawl` is absent:

```json
"crawl": {
  "v": 1, "scope_applied": true, "mode": "enforce",
  "links_seen": 143, "links_allowed": 12, "links_denied": 131,
  "denials": {
    "counts":  { "external_host_disabled": 118, "exclude_pattern_hit": 9, "not_crawlable": 4 },
    "samples": [ { "url": "https://other.test/x", "code": "external_host_disabled",
                   "message": "...", "remedy": "...", "depth": 3 } ],   // at most 3 per code, 20 per page
    "robots":  [ { "url": "https://example.com/private/a", "code": "robots_disallowed",
                   "user_agent": "quarry", "path": "/private/a" } ]      // at most 50 per page
  },
  "hosts_added": [], "crawl_delay_ms": 2000
}
```

Fetch-time denial (HTTP 403, standard envelope): `error.code:"SECURITY_BLOCKED"`, `error.retryable:false`, `error.details.denial = {code, message, remedy, user_agent?, path?, host?}`.

Job params stamped by the edge (control stores `params` as opaque JSON, so no control migration): `scope` (the `CrawlScope`), `scope_mode`, `robots`, `max_pages`, `max_depth`; `HandoffAck.effective` echoes `{scope, scope_mode, robots, max_pages, max_depth, legacy, warnings[]}`.

Events (no new types): `run_started.payload` gains `scope`, `scope_mode`, `robots`; `page_blocked.payload = {url, category:"blocklist", code, message, remedy, source:"fetch"|"discovery", user_agent?, path?}`; `page_fetched.payload` gains `crawl_delay_ms` when above zero; terminal payloads (`run_completed`, `run_cancelled`, `run_failed`) gain `pages_blocked`, `links_denied`, `hosts_added`, and `denials = {mode, counts, samples}` (samples at most 10 per code).

### Slice 0. Truth-telling and characterization tests

**Goal.** Stop the docs from claiming robots, sitemap, include/exclude and external-link controls are done, record the decisions in one document, and pin today's behaviour with tests so slice 3's change shows up as a deliberate diff.

**Changes.**

| Path | Tag | Change |
|---|---|---|
| `Q/docs/GAP.md` | [C] | rows 176-180, 201-202, 292, 316, 419, 431, 494, 725: status "Open - see WS14a", delete the false mechanism text, remove the two non-existent internal routes at 725 |
| `Q/docs/Features.md` | [M] | lines 27-31: describe crawl as unscoped today; no overlap with the live diff (lines 39, 179) |
| `Q/docs/gap-quarry.md` | [C] | line 219 |
| `Q/docs/ARCHITECTURE.md` | [C] | lines 173-175 (`HostSlot` delay floor, `DenialReason`) marked planned |
| `Q/docs/POLICY.md` | [C] | line 23: `respect_robots` is a stamp, not enforcement |
| `Q/docs/CONTRACTS.md` | [C] | lines 352-368 and 427-431: mark fields planned; add the code mapping table of D6 |
| `Q/docs/CRAWL_SCOPE_ROBOTS.md` | [NEW] | the contract of this workstream: scope defaults, pattern grammar and caps, robots semantics, denial vocabulary and events, flags, legacy opt-out, ZDR decision (keep refusal), what the Model may and may not request |
| `apps/STALE_DOC_DELETION_REGISTER.md` | [C] | add the rewritten files as `update` |
| `ORCH/workflows/workflows_test.go` | [C] | `TestCrawlJobWF_CharacterizesExternalLinkFollowing_Legacy`: a result link on `https://other.example/` is fetched today |
| `ORCH/workflows/workflows_durable_test.go` | [NEW] | `TestCrawlJobDurable_EnqueuesEveryLinkWithOneActivityEach` with `a.DurableFrontier = true` and mocked `FrontierEnqueue`, `FrontierPop`, `FrontierAck`: first test of the production loop |

**API / data model.** None.

**Flags and rollout.** None. Rollback: revert the PR.

**Tests.** `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./internal/workflows/ -run "CrawlJob" -count=1`. CI: `.github/workflows/quarry-v2-ci.yml` [C] runs the orchestrator Go tests with the 80% coverage floor (lines 315-329).

**Definition of done.** No row in `GAP.md` marked Closed without a code citation in the row; the two characterization tests pass against unmodified code; `CRAWL_SCOPE_ROBOTS.md` reviewed by the Ingestion owner.

**Deployment evidence.** Not applicable (docs and tests only). State that explicitly in the PR so nobody reads it as a deployment claim.

**Effort.** S, 1.5 engineer-days. **Risks.** Rewording a gap table other planning documents cite: grep the repo for the row titles before editing.

### Slice 1. Hermetic fixture site and the real-stack e2e harness

**Goal.** Give every later slice a failing test first (Rust, in-memory) and a real-stack assertion (compose), without opening a hole in the SSRF guard (D14).

**Changes.**

| Path | Tag | Change |
|---|---|---|
| `Q/crates/quarry-edge/tests/support/fixture_site.rs` | [NEW] | `FixtureSiteDriver: Driver`: a configurable in-memory site (pages with links, `robots.txt` with groups and `Crawl-delay`, optional `sitemap.xml`, redirects, status overrides such as 503 on robots), recording every request (`Vec<(Url, Instant)>`) so tests assert on what was and was not fetched |
| `Q/crates/quarry-edge/tests/support/mod.rs` | [M] | one line `pub mod fixture_site;` (the file has a 1-line live diff) |
| `Q/crates/quarry-edge/tests/fixture_site_selftest.rs` | [NEW] | the driver serves what it is told and logs requests |
| `Q/deploy/compose/docker-compose.crawl-fixture.yml` | [NEW] | overlay: network `crawl-fixture` (`ipam` subnet `192.88.99.0/24`), edge attached to it, services `crawl-fixture-a` and `crawl-fixture-b` (nginx:alpine) with network aliases `fixture-a.crawl.test`, `docs.fixture-a.crawl.test`, `fixture-b.crawl.test`, access log format with millisecond timestamps and user agent |
| `Q/deploy/compose/crawl-fixture/a/` | [NEW] | `index.html` (links: `/docs/a`, `/blog/b`, `/private/secret`, `/img/logo.png`, `mailto:`, `http://fixture-b.crawl.test/ext`, `http://docs.fixture-a.crawl.test/`), `docs/a.html`, `blog/b.html`, `private/secret.html`, `robots.txt` (`User-agent: *`, `Disallow: /private/`, `Crawl-delay: 2`), `nginx.conf` |
| `Q/deploy/compose/crawl-fixture/b/` | [NEW] | `ext.html`, `nginx.conf` |
| `Q/scripts/e2e/crawl-scope-robots.sh` | [NEW] | bring-up, token, crawl, poll events, assertions via `curl` and `jq`, reads fixture logs with `docker logs`; flag `--expect legacy`, `--expect observe` or `--expect enforce` selects the assertion set; initially only `legacy` is green |

**API / data model.** None.

**Flags and rollout.** Test-only. The overlay must never be referenced by production compose files; a header comment and a CI grep guard (`! grep -r crawl-fixture deploy/compose/docker-compose.yml`) enforce it.

**Tests.** `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge --test fixture_site_selftest`; `docker compose -f deploy/compose/docker-compose.yml -f deploy/compose/docker-compose.crawl-fixture.yml config -q`.

**Definition of done.** The harness run with `--expect legacy` against the unmodified stack is green and proves the baseline: `docker logs crawl-fixture-b` contains `GET /ext` and `docker logs crawl-fixture-a` contains no `GET /robots.txt`.

**Deployment evidence.** Rebuilt local images (`cd "apps/Ingestion Plane" && make up`, then the overlay `up -d`), running stack, the script prints the baseline table (pages fetched per fixture host). Flow: POST `/v1/crawl {url:"http://fixture-a.crawl.test/", max_pages:20}` with the dev bearer the stack accepts; observable: fixture-b received a request.

**Effort.** M, 3 engineer-days. **Risks.** Docker Desktop may reject or collide with the chosen subnet (fallback: another unblocked, non-overlapping /24); the dev auth path for `/v1/crawl` is [UNVERIFIED] (the edge test suite uses `QUARRY_EDGE_AUTH_DEV_BYPASS`, which production refuses at `main.rs:133`); the harness must run with `ENVIRONMENT=dev`.

### Slice 2. Rust crawl filter and `run_page` crawl context (inert until used)

**Goal.** The edge can return scoped links and a typed denial report. Behaviour for callers that do not send `crawl` is byte-identical.

**Changes.**

| Path | Tag | Change |
|---|---|---|
| `QC/crawl_scope.rs` | [NEW] | `CrawlScope`, `CrawlContext`, `CrawlKind`, `ScopeMode`, `RobotsMode`, `CrawlReport`, `DenialCounts`, `DenialSample`, `RobotsDenial`, cap constants (`MAX_PATTERNS=100`, `MAX_PATTERN_CHARS=512`, `MAX_PATTERN_TOTAL_CHARS=8192`, `MAX_LINKS_EVALUATED=5000`, sample limits), serde with `deny_unknown_fields` on `CrawlScope` |
| `QC/crawl_denial.rs` | [C] | add `RobotsUnreachable { host }` (additive tag `robots_unreachable`), `remedy()`, extend the unique-code and message tests |
| `QC/lib.rs` | [C] | `pub mod crawl_scope;` |
| `QR/crawl_filter.rs` | [NEW] | `CompiledScope::compile(&CrawlScope) -> Result<CompiledScope, Vec<FieldError>>` (glob and `re:`/`url:`/`re_url:` grammar of D3, regex `size_limit` and `dfa_size_limit`); `host_matches(root, host, allow_subdomains)` (www-insensitive, trailing-dot, IDNA via `url`); `evaluate(&CompiledScope, &CrawlContext, page_final: &Url, links: &[Link], robots_peek) -> FilterOutcome { allowed, report, hosts_added }`; check order: parse, scheme, fragment-only, depth, host scope, section scope, exclude, include, extension, robots-peek (cached rules only); dedupe within the response on the normalised URL; stops at `MAX_LINKS_EVALUATED` and counts the rest as `not_crawlable { reason: "link limit" }` |
| `QR/crawl_frontier.rs` | [C] | `normalize_url` becomes `pub(crate)`; `CrawlFrontier::enqueue` calls `crawl_filter::host_matches(.., true)` so the two share one implementation while the frontier keeps its documented subdomain behaviour; existing tests unchanged |
| `QR/lib.rs` | [M] | `pub mod crawl_filter;` and re-exports (live diff at lines 42 and 160-165; additive) |
| `Q/crates/quarry-runtime/Cargo.toml` | [C] | `regex = "1"` (already a workspace-level dependency of `quarry-transform`) |
| `QE/routes.rs` | [M] | `InternalRunPage` gains `crawl: Option<CrawlContext>` (743-769); `InternalRunPageResult` gains `crawl: Option<CrawlReport>` and its `links` is produced by `from_output(run_id, &out, ctx)` which, when `crawl` is present, evaluates `out.formats.links` (keeping `rel` for future use) instead of mapping every href (817-841); both return sites in `internal_run_page` (cache hit 1029-1038, fresh 1041-1055) go through one `apply_crawl_filter`; a malformed `crawl` is a 400 with `fields`; `kind:"batch"` returns an empty `links` |
| `Q/docs/openapi.yaml` | [M] | `InternalRunPage.crawl`, `InternalRunPageResult.crawl`, `CrawlContext`, `CrawlScope`, `CrawlReport`, `CrawlDenial` schemas (additive; 78 lines of unrelated live diff) |

**API / data model.** The two JSON shapes above. No migration, no proto.

**Flags and rollout.** None needed: nothing sends `crawl` until slice 3. Rollback: revert.

**Tests.**
- Unit, `QR/crawl_filter.rs`: same host, `www.` equivalence both directions, trailing dot, uppercase, IDN (`bücher.example` vs `xn--bcher-kva.example`), userinfo trick `http://example.com@evil.test/`, port, subdomain on and off, external on and off, depth boundary, `mailto:`, `javascript:`, `tel:`, `data:`, fragment-only, extension denylist, `crawl_entire_domain:false` directory prefix, include miss, exclude hit, exclude beats include, duplicate collapse, sample and robots caps, 6,000-link page, pattern caps and the exact error paths, **catastrophic pattern `re:(a+)+$` against a 10,000-character path completes in under 250 ms**, a pattern over the regex size limit is a field error not a panic, `hosts_added` only at depth 0.
- Differential: proptest that `CrawlFrontier::enqueue` and `evaluate` agree on host and pattern decisions over random candidates (modulo dedupe and `max_pages`).
- Edge, `Q/crates/quarry-edge/tests/crawl_scope_robots.rs` [NEW] (harness: `support::base_state` with an `internal_signer`, runtime token env, `FixtureSiteDriver`): `run_page` with `crawl` returns only fixture-a links and the right counts; `observe` returns all links plus counts; redirect at depth 0 adds the host; no `crawl` field returns the golden response recorded from the unmodified handler; malformed `crawl` is 400.
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-core crawl_ && cargo test -p quarry-runtime crawl_filter crawl_frontier && cargo test -p quarry-edge --test crawl_scope_robots && cargo test --workspace` (CI: `quarry-v2-ci.yml` 204-205; `cargo llvm-cov --workspace --all-features` at 304).

**Definition of done.** All of the above green; evaluating 5,000 links with 100 patterns is under 250 ms in a debug build; `cargo clippy --workspace -- -D warnings` clean; the golden-response test proves no change without `crawl`.

**Deployment evidence.** Rebuild the edge image (`apps/Ingestion Plane`: `make up` rebuilds), overlay up. Compute `X-Quarry-Run-Sig = hex(HMAC-SHA256(QUARRY_INTERNAL_SECRET, org_id + "\n" + url))` with `openssl` and call `POST /v1/internal/run_page` with the runtime bearer for `http://fixture-a.crawl.test/` and `crawl:{kind:"crawl",depth:0,seed_hosts:["fixture-a.crawl.test"],scope_mode:"enforce",robots:"respect",scope:{...defaults}}`. Assertions with `jq`: `.links` has exactly the fixture-a internal pages (no `fixture-b`, no `docs.fixture-a`, no `mailto:`, no `.png`), `.crawl.denials.counts.external_host_disabled == 1`, `.crawl.denials.counts.out_of_scope == 1` (subdomain), `.crawl.scope_applied == true`; the same call without `crawl` returns all links (compatibility).

**Effort.** M, 4 engineer-days. **Risks.** A link-dense page costs CPU (capped at 5,000 evaluated links, measured); `normalize_url` drops fragments and sets an empty path to `/`, so strings differ from the raw hrefs (only when `crawl` is present, documented); the `Link` struct has `rel` and `text` which are unused here (non-goal: honouring `rel=nofollow`).

### Slice 3. Go scope default, typed denials, batch enqueue, replay safety, minimal UI summary

**Goal.** The audit's "step 1", done safely: scope applies in both enqueue loops of the Go workflow by deliberate switch, denials are classified and summarised, and existing crawls change visibly, not silently.

**Changes.**

| Path | Tag | Change |
|---|---|---|
| `ORCH/workflows/workflows.go` | [C] | `CrawlJobInput` gains `Scope *CrawlScope`, `ScopeMode string`, `Robots string` (75-87); `runPage` takes and forwards a `*activities.CrawlContext` (174-232), classifies errors with `errs.CategoryOf`, emits `page_blocked` with the denial payload; the in-workflow BFS (396-512) and `crawlJobDurableFrontier` (518-610) both: build the context per URL (depth, `seed_hosts` = seeds plus `hosts_added`), refuse children when `scope_applied` is missing (D7), merge `crawl.denials` into the aggregator, enqueue `res.Links` (already filtered); the durable loop enqueues once per page with `FrontierEnqueueBatch`; counters `pagesBlocked`, `linksDenied`; terminal payloads per the contract; **all new behaviour under `workflow.GetVersion(ctx, "crawl-scope-v1", workflow.DefaultVersion, 1)`**, the legacy branch kept verbatim; the Go depth guard (474, 574) stays as belt and braces |
| `ORCH/workflows/crawl_denials.go` | [NEW] | `denialAgg`: counts map, samples (10 per code, 12 codes), bounded `deniedSeen`, 200-event per-URL cap, deterministic iteration order for payloads (sorted keys) |
| `ORCH/workflows/signals.go` | [C] | `crawlProgress` gains `Blocked`, `Denied` (additive query JSON) |
| `ORCH/activities/activities.go` | [C] | `CrawlContext`, `CrawlReport`, `CrawlScope` structs; `RunPageInput.Crawl`, `RunPageResult.Crawl`; `RunPage` posts `crawl` and uses `errs.FromHTTPResponse` (254-256); new `FrontierEnqueueBatch(ctx, FrontierEnqueueBatchInput{Queue, OrgID, Items[]{RequestID, URL, Depth}})` posting `/v1/internal/queues/{q}/enqueue_batch` with the existing `X-Quarry-Queue-Sig` |
| `ORCH/errs/errs.go` | [C] | `FromHTTPResponse(op, status, body)` parses the error envelope, a `details.denial` body becomes `CategoryBlocklist` with a `Denial` attached to the application-error details; `CategoryOf(err) (Category, map[string]any, bool)` reads `*temporal.ApplicationError` `Type()` and details so the **workflow** can classify; `IsNonRetryable` unchanged |
| `ORCH/jobs/dispatcher.go` | [C] | `crawlInputFromJob` (432-454) reads `params.scope`, `params.scope_mode`, `params.robots`; **absent `scope` becomes `Scope{Legacy:true}`** (jobs accepted before the deploy); `batchInputFromJob` (456-469) reads `params.robots` (used in slice 5) |
| `QE/queue_routes.rs` | [C] | `enqueue_batch` handler and route: body `{org_id, items:[{request_id,url,depth}]}`, at most 500 items, same `auth_and_scope`, returns `{accepted: n}` |
| `QE/crawl_policy.rs` | [NEW] | latched, boot-validated reader of `QUARRY_EDGE__CRAWL_SCOPE_MODE`, `QUARRY_EDGE__CRAWL_DEFAULT_MAX_PAGES`, `QUARRY_EDGE__CRAWL_MAX_PAGES_CAP` (same latch idiom as `runtime_token()`, routes.rs:847-858; an unparsable value fails boot, unlike `EdgeConfig`) |
| `QE/lib.rs` | [M] | `pub mod crawl_policy;` |
| `QE/routes.rs` | [M] | `crawl_handoff` (590-667) normalises the two scope switches now (`allow_subdomains`, `allow_external_links`), applies the page-cap default and cap, stamps `scope`, `scope_mode`, `robots:"respect"`, `max_pages`, `max_depth` into the `forward_to_orchestrator` payload (630-638), and returns `HandoffAck.effective` (727-738); `CrawlRequest` gains the two fields |
| `Q/docs/openapi.yaml` | [M] | `CrawlRequest` fields, `HandoffAck.effective` |
| `GW/domains/knowledge/quarry.rs` | [C] | `normalize_crawl_body` (386-416) forwards the two switches (camel and snake); tests 998-1017 extended |
| `GW/domains/actions/dispatchers.rs` | [C] | `dispatch_crawl_site` (314-360) builds its body through `normalize_crawl_body` (SSRF normaliser, 5000 clamp) and stops sending `org_id` in the body |
| `FE/shared/api/knowledge-client.ts` | [C] | `normalizeCrawlEvent` (about 716-760) reads `payload.pages_blocked`, `links_denied`, `denials`; `CrawlWorkflowEvent` gains `pagesBlocked?`, `linksDenied?`, `denials?`; `summarizeCount` (about 800) text |
| `FE/features/dashboard/home/KnowledgeComposer.tsx` | [M] | job row shows the summary, bilingual via `i18n.tr`: nb "Hentet 12 sider. 118 lenker utenfor nettstedet hoppet over. 2 sider blokkert av robots.txt." and en "Fetched 12 pages. Skipped 118 links outside this site. 2 pages blocked by robots.txt." |

**API / data model.** Contracts above. Next free control migration is `015`; **this slice needs none** (scope rides in `jobs.params`, `Params map[string]any`, store.go:174-212). Event types unchanged.

**Flags and rollout.** `QUARRY_EDGE__CRAWL_SCOPE_MODE` default `observe`. Stage A (this PR): deploy edge and orchestrator together, soak in `observe`, read `denials.counts` across real crawls. Stage B (slice 8): flip to `enforce` by config. Rollback at any stage: set `off` and restart the edge (legacy stamping), or redeploy the previous orchestrator (the legacy branch is intact). Version skew handled by D7.

**Tests.**
- Go (Temporal test environment, `testsuite.WorkflowTestSuite`), `workflows_test.go` and `workflows_durable_test.go` [NEW]: `TestCrawlJobWF_SendsCrawlContextOnEveryRunPage`; `TestCrawlJobWF_RefusesChildrenWhenEdgeDoesNotApplyScope` (result without `scope_applied` gives `run_failed` code `edge_scope_unsupported`); `TestCrawlJobWF_LegacyScopeSendsNoContext`; `TestCrawlJobWF_DenialSummaryInRunCompleted`; `TestCrawlJobWF_RobotsBlockedPageEmitsPageBlockedAndCountsBlocked` (mock returns `temporal.NewNonRetryableApplicationError(msg, "blocklist", nil, map{code:"robots_disallowed", ...})`); `TestCrawlJobWF_DeniedUrlEventsAreCappedAndDeduped`; durable variants of each with `a.DurableFrontier = true`, one `FrontierEnqueueBatch` per page (replaces the characterization test of slice 0); `TestCrawlJobWF_AggregatesHostsAdded`.
- Go replay, `ORCH/workflows/workflows_replay_test.go` [NEW]: `worker.NewWorkflowReplayer` replays `testdata/crawl_pre_scope_history.json` [NEW], a history exported from the pre-change build on the running stack (`temporal workflow show --output json`), and a second one from the new build; both replay without non-determinism.
- Go: `errs_test.go` [C] (`FromHTTPResponse` envelope parsing, `CategoryOf` through a real `temporal.ApplicationError`); `ORCH/jobs/dispatcher_scope_test.go` [NEW] (absent scope is legacy; malformed params safe).
- Rust: `Q/crates/quarry-edge/tests/crawl_scope_robots.rs` extended with `enqueue_batch` (signature, cap, accepted count) and handoff stamping (`crawl_handoff` with the orchestrator double records `params.scope`, `scope_mode`; an `off` mode records `legacy:true`).
- Gateway and FE: `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`; `pnpm vitest run src/shared/api/knowledge-client.test.ts`; `pnpm lint && pnpm typecheck` (baseline failures exist in the worktree, diff failing sets against HEAD).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./... -count=1`; `cd ../quarry-control && go test ./... -count=1`; `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge`.

**Definition of done.** Tests green in CI; the replay tests pass; with `enforce` on the fixture stack no request reaches fixture-b; with `observe` the counts appear in `run_completed` and nothing changes in fetched pages; the UI row shows the summary; Go coverage floor holds.

**Deployment evidence.** Rebuild orchestrator, edge, control and gateway images and the SPA, stack up with the fixture overlay. Flow and assertions (`scripts/e2e/crawl-scope-robots.sh --expect observe`, then `--expect enforce` after setting the edge env and restarting it):
1. `observe`: POST `/v1/crawl` on fixture-a; the job's `run_completed.payload.denials.counts.external_host_disabled >= 1`, fixture-b access log **does** show `GET /ext` (observe drops nothing).
2. `enforce`: same crawl; fixture-b log shows **zero** requests; `run_completed.payload.pages_visited == 3` (index, docs/a, blog/b) and `links_denied` equals the sum of counts; `docs.fixture-a` host not fetched (subdomain off).
3. Opt-out: POST with `allow_external_links:true, allow_subdomains:true`; fixture-b log shows `GET /ext` (legacy reproduced on request).
4. Compatibility: stop the edge image, start the previous edge image (old binary) with the new orchestrator; a crawl ends `run_failed` with `code:"edge_scope_unsupported"` and fetched no child (fail closed).
5. History: `docker exec <temporal> tctl --ns default wf show -w quarry-job-<job_id> --output json | jq length` for the same fixture crawl before and after this slice: events per page drop because enqueues are batched (record both numbers in the PR).
6. UI: the Knowledge composer job row shows the nb or en summary line for the finished crawl (screenshot).

**Effort.** M, 6 engineer-days. **Risks and mitigations.** Replay non-determinism for in-flight executions: `GetVersion` plus the replay test plus the 30-minute execution bound. Changing a default alters existing crawl results: `observe` first, explicit opt-out, effective scope echoed, summary visible. In-flight edge diffs in `main.rs` and `routes.rs`: single-line additive hunks, rebase. `knowledge.crawl_site` now clamps and normalises (behaviour change for the Model path: stated in the PR and in `CRAWL_SCOPE_ROBOTS.md`). The production loop has had no test; the new durable tests may expose latent bugs (budget one extra day).

### Slice 4. Robots gate: guarded fetch, RFC 9309 semantics, Crawl-delay into the scheduler

**Goal.** The audit's "step 2": crawl and batch pages are checked against robots.txt at fetch time through the guarded driver chain, discovery filters against warm cache, `Crawl-delay` slows the existing scheduler, and `/v1/map` shares the same gate.

**Changes.**

| Path | Tag | Change |
|---|---|---|
| `QR/robots_cache.rs` | [C] | `RobotsFetcher::fetch` returns `RobotsFetch { Found(String), Absent, Unreachable(String) }` instead of `Option<String>`; cache entries hold `RobotsState { Rules(Arc<RobotsTxt>), Absent, Unreachable }`; key is the origin `scheme://host:port`; TTLs 24 h (rules), 1 h (absent), 5 min (unreachable); entry map bounded (default 10,000, oldest-first eviction) and the `in_flight` map pruned after each fetch; `peek(origin)` (no network) and `get_uncached(origin)` added; `ReqwestRobotsFetcher` (165-219) deleted; the six in-module tests updated |
| `QR/robots_gate.rs` | [NEW] | `RobotsGate { cache, tokens, cfg }`; `DriverRobotsFetcher { driver: Arc<dyn Driver> }` (two attempts within the configured timeout, `fetch_conditional` with `FetchHints { org_id, privacy: deny-third-party, max_body_bytes: 512 KiB, truncate_at_cap: true }`, 2xx to `Found`, 4xx to `Absent`, 5xx and errors to `Unreachable`); `RobotsDecision { Allowed { crawl_delay }, Disallowed { token, path }, Unreachable { host }, NotApplicable }`; `check(url, org, zdr)`, `peek(url)`, `rules_for_origin(url, zdr)`; `ROBOTS_PRODUCT_TOKENS: [&str; 2] = ["quarrybot", "quarry"]` (single constant, to be replaced by F-t2-067); `RobotsGateConfig::from_env()` failing boot on garbage |
| `QT/robots.rs` | [C] | `RobotsTxt::is_allowed_any(&[&str], path_and_query)` (most restrictive wins), keep `is_allowed`; tests for query matching (`Disallow: /*?session=`), `$` anchors with query, the token set |
| `QR/driver.rs` | [C] | reuse `FetchHints.max_body_bytes` (D5 slice 1 introduces it with a streamed read that aborts at the cap; if D5 has not merged, this slice adds it with identical name and semantics) and add `FetchHints.truncate_at_cap: bool` (default false) so the robots fetch can keep the first 512 KiB instead of failing, as RFC 9309 allows |
| `QR/fetch.rs` | [C] | `StaticDriver` streams `chunk()`s up to `max_body_bytes` instead of `resp.bytes()` (387), truncating when `truncate_at_cap` is set; other drivers ignore it (documented on the field); same hunk as D5's body cap (`fetch.rs:376-391`, section 5) |
| `QR/host_scheduler.rs` | [C] | `HostSlot { min_interval, next_start }`; `HostScheduler::set_min_interval(host, Duration)`; `acquire` reserves a start time under the lock and `sleep_until` it outside the lock after the semaphore permit; zero interval is the unchanged fast path |
| `QR/pipeline.rs` | [C] | `PageRunner.robots: Option<RobotsEnforcement { gate, mode }>`; in `run_inner` after `preflight` (305-321) and before `resolve_public_url` (327): check, on deny emit `PageBlocked` through `emit_page_event` with `{url, reason:"robots_disallowed" or "robots_unreachable", code, user_agent, path}` and return `QuarryError::new(SecurityBlocked, denial.message()).with_details({"denial": ...})`; on allow call `scheduler.set_min_interval(host, delay)`; after the fetch, if `resp.final_url`'s origin differs from the requested one, re-check robots for the final URL before anything is persisted (shared hook with D7 S6's final-URL recheck: one function, not two); the two test `PageRunner {` literals (1177, 1318) gain `robots: None` |
| `QE/routes.rs` | [M] | the three `PageRunner {` literals (449, 989, 1104): `robots: None` for scrape and scrape/stream, `Some(..)` in `internal_run_page` when `crawl.robots == respect` and the gate exists and `QUARRY_EDGE__CRAWL_ROBOTS != off`; the `crawl` report gets `crawl_delay_ms` from `gate.peek` after the run; link filtering passes `robots_peek` into `crawl_filter::evaluate` |
| `QE/state.rs` | [M] | `pub robots: Option<Arc<quarry_runtime::RobotsGate>>` (the file has a 21-line live diff; additive) |
| `QE/main.rs` | [M] | build the gate with `default_driver.clone()` before the `AppState` literal (about 1431); log the effective robots mode at boot |
| `Q/crates/quarry-edge/tests/support/mod.rs` | [M] | `robots: None` |
| `QE/map_routes.rs` | [C] | robots via the gate (`rules_for_origin`, uncached for ZDR claims), path and query, shared token set; the SSRF heuristic pre-check at 152-162 stays; `MAP_UA` constant removed |
| `ORCH/workflows/workflows.go` | [C] | `page_fetched` payload gains `crawl_delay_ms` (under the slice-3 version gate) |
| `Q/docs/openapi.yaml` | [M] | `details.denial` on the 403 error schema |

**API / data model.** Fetch-time denial contract above; `RobotsDecision` is internal. No migration.

**Flags and rollout.** `QUARRY_EDGE__CRAWL_ROBOTS=off|observe|enforce`, default `observe` in the first release (evaluates, counts `would_deny` in a content-free metric and a log line carrying no URL, never blocks); `QUARRY_EDGE__ROBOTS_UNREACHABLE=disallow|allow` (default `disallow`, ADR topic 5); `QUARRY_CRAWL_DELAY_MAX_S` (10, ceiling 60); `QUARRY_ROBOTS_CACHE_MAX_ENTRIES` (10,000); `QUARRY_ROBOTS_FETCH_TIMEOUT_S` (8). Rollback: `off`.

**Tests.**
- `robots_cache`: found, 404, 403, 410 (absent, allow), 500 and timeout (unreachable), TTLs with `tokio::time::pause`, coalescing under 50 concurrent callers (one fetch), eviction at the cap, `in_flight` pruned, origin keys differ by scheme and port, `get_uncached` inserts nothing.
- `robots_gate` with a stub `Driver`: it calls `fetch_conditional` (not `fetch`), carries the org and a deny-third-party privacy policy, appends no query, truncates at 512 KiB; a stub that returns `SecurityBlocked` (private target) yields `Unreachable`, never `Allowed`; `check` honours most-restrictive-wins across the token set.
- SSRF, `Q/crates/quarry-runtime/tests/robots_gate_ssrf.rs` [NEW]: a real `StaticDriver` pointed at `http://127.0.0.1:<port>/robots.txt` is refused before any request (the pattern of `a_direct_fetch_to_a_loopback_target_is_refused_before_any_request`); a redirect from a public stub to `169.254.169.254` is refused at the hop.
- `host_scheduler`: with `set_min_interval(2s)`, three concurrent `acquire`s start at 0, 2, 4 seconds (`tokio::time::pause`); another host is unaffected; zero interval adds no latency; AIMD tests unchanged.
- `pipeline`: a disallowed URL never reaches the driver (call counter), emits exactly one `PageBlocked`, does not call `record_bad`; observe mode fetches; `ignore` mode (no enforcement) fetches; redirect-to-disallowed-final-URL persists nothing.
- Edge `Q/crates/quarry-edge/tests/crawl_scope_robots.rs`: typed 403 body shape; `internal_run_page` with `robots:"respect"` and a disallowed URL; `crawl_delay_ms` reported; `/v1/map` with a ZDR token does not populate the cache (`gate.len() == 0`) and with a non-ZDR token does; map drops `/private/` using query-aware rules.
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-transform robots && cargo test -p quarry-runtime robots_cache robots_gate host_scheduler pipeline && cargo test -p quarry-runtime --test robots_gate_ssrf && cargo test -p quarry-edge --test crawl_scope_robots map_routes && cargo test --workspace`.

**Definition of done.** All tests green; `ReqwestRobotsFetcher` has no remaining reference (`git grep`); the `observe` mode produces no behaviour change in the e2e run; one `AppState` field and two `PageRunner` field additions reviewed against D5 and D7's concurrent additions (section 5).

**Deployment evidence.** Rebuilt edge and orchestrator images, overlay up, edge env `QUARRY_EDGE__CRAWL_ROBOTS=enforce`, `QUARRY_EDGE__CRAWL_SCOPE_MODE=enforce`. Flow: `scripts/e2e/crawl-scope-robots.sh --expect enforce`:
1. Crawl `http://fixture-a.crawl.test/` (robots: `Disallow: /private/`, `Crawl-delay: 2`). Assert in the fixture-a access log: exactly one `GET /robots.txt` with a user agent containing `Quarry`; **no** `GET /private/secret`; the page requests carry timestamps at least 2.0 s apart (three pages, at least 4 s span).
2. Job events (`GET /v1/jobs/{id}/events`): a `page_blocked` with `payload.code == "robots_disallowed"`, `payload.path == "/private/secret"`; `run_completed.payload.pages_blocked >= 1`, `pages_failed == 0`.
3. Fetch-time layer: POST `/v1/batch` with `["http://fixture-a.crawl.test/private/secret"]`; a `page_blocked` event, still no fixture request for that path.
4. Unreachable: switch the fixture robots to return 503; crawl; every page including the seed is `page_blocked` with `code:"robots_unreachable"` and the log shows the two robots attempts; set `QUARRY_EDGE__ROBOTS_UNREACHABLE=allow`, restart the edge, crawl again: pages are fetched.
5. SSRF: a fixture host whose `/robots.txt` answers `302 http://169.254.169.254/robots.txt`: the crawl treats it as unreachable and the edge log shows the refused hop; no packet to the link-local address (assert by the refusal log line, a network capture is not part of the stack).
6. Map: `POST /v1/map` for fixture-a omits `/private/secret` and the ZDR-token variant leaves no cache entry (edge debug endpoint or log line `robots cache entries=0`).

**Effort.** L, 7 engineer-days. **Risks and mitigations.** False-positive denials from the token set or a quirky robots file: `observe` soak first, `ignore` is not available to users, the typed `remedy` explains it. A 5xx robots endpoint now stops a crawl: configurable, ADR-GATED topic 5, soak shows the rate. The scheduler `acquire` is the hot path of every fetch: zero interval is a fast path and the new lock work is measured with the existing `host_scheduler` tests plus a micro-benchmark. Crawl-delay against the 30-minute cap (D12). In-flight `main.rs` and `state.rs` diffs: additive hunks, rebase. Two other designs add `PageRunner` fields (D5 `scrape`, D7 declines); if D5 lands first, fold `robots` into one `RunExtras` struct with `Default` instead of a third literal edit across five sites.

### Slice 5. Batch check

**Goal.** Close "batch accepts any URL unchecked" without any network work at handoff.

**Changes.**

| Path | Tag | Change |
|---|---|---|
| `QE/crawl_request.rs` | [NEW] | `validate_batch_urls(urls, max) -> BatchCheck { accepted, rejected: Vec<{url, code, message}> }`: count cap (`QUARRY_EDGE__CRAWL_BATCH_MAX_URLS`, default 500), `url::Url::parse`, http(s) only, `quarry_security::heur::check`, dedupe on normalised URL; also the shared home of the field-error type used by slice 6 |
| `QE/routes.rs` | [M] | `batch_handoff` (681-725): validate before `forward_to_orchestrator`; all rejected is 400 with `details.rejected`; partial is 200 with `HandoffAck.rejected`; `BatchRequest` unchanged on the wire (rejected is response-only); the meter uses the accepted count; `HandoffAck` gains `rejected` and `effective` |
| `ORCH/jobs/dispatcher.go` | [C] | `batchInputFromJob` stamps `Robots` from params (default `respect`) |
| `ORCH/workflows/workflows.go` | [C] | `BatchJobWF` (359-383): pass `CrawlContext{Kind:"batch", Robots}` to `runPage`, count `pages_blocked`, report it in `run_completed` (version-gated like slice 3) |
| `Q/docs/openapi.yaml` | [M] | `HandoffAck.rejected`, batch 400 shape |

**API / data model.** `rejected: [{url, code, message}]` with codes from the existing vocabulary (`not_crawlable`, `security_rejected`, `duplicate`). No migration.

**Flags and rollout.** Validation is always on (it only rejects input that would fail later anyway); the robots part follows `QUARRY_EDGE__CRAWL_ROBOTS`. Rollback: revert.

**Tests.** Rust unit for `validate_batch_urls` (file scheme, ftp, userinfo, `http://127.0.0.1`, `http://[::1]`, `localhost`, duplicates differing in fragment, 501 URLs); edge test with an orchestrator double: nothing forwarded when all are rejected; partial forwards only accepted; Go `TestBatchJobWF_CountsBlockedAndSendsBatchContext`. Commands: `cargo test -p quarry-edge crawl_request batch`, `cd ".../quarry-orchestrator" && go test ./internal/workflows -run Batch -count=1`.

**Definition of done.** Direct API callers cannot enqueue a `file://` or private-literal URL; the gateway path is unchanged.

**Deployment evidence.** Rebuilt edge and orchestrator; `POST /v1/batch` with `["http://fixture-a.crawl.test/docs/a","http://fixture-a.crawl.test/docs/a#x","file:///etc/passwd","http://fixture-a.crawl.test/private/secret"]`: ack `rejected` contains `not_crawlable` (file) and `duplicate`; events show `page_blocked` robots for the private URL; `run_completed.payload == {pages_visited:1, pages_blocked:1}`; fixture-a log lacks `GET /private/secret`.

**Effort.** S, 2 engineer-days. **Risks.** A direct caller that today sends 1,000 URLs now gets a 400 (cap 500, configurable); announce in the changelog.

### Slice 6. Scoping vocabulary on the crawl request, gateway, action schema and UI

**Goal.** The audit's "step 3" minus the entitlement: the full `CrawlRequest` v2 of D8 with strict-but-staged validation, an effective-options echo, and the Model-visible subset.

**Changes.**

| Path | Tag | Change |
|---|---|---|
| `QE/crawl_request.rs` | [NEW] | `CrawlRequestV2` parsing from `serde_json::Value` with aliases, `FieldError {path, code, message}` collection, caps of D3, unknown-key warnings (lenient) or errors (`QUARRY_EDGE__CRAWL_REQUEST_VALIDATION=strict`), the reserved-key denylist, `sitemap` and `ignore_query_parameters` and `scrape_options` handling (400 `unsupported_option` naming F-t2-041 and D5), `ignore_robots_txt` always refused until slice 7, effective-scope construction; reuse D5's validator helper if it has merged |
| `QE/routes.rs` | [M] | `crawl_handoff` takes `Json<serde_json::Value>`, calls the parser, maps `FieldError`s to a 400 `details.fields`, stamps the full scope into the job params; `HandoffAck.effective.warnings` |
| `ORCH/jobs/dispatcher.go`, `ORCH/workflows/workflows.go` | [C] | already carry `Scope` (slice 3); parse and forward `include_paths`, `exclude_paths`, `deny_extensions`, `crawl_entire_domain` |
| `Q/docs/openapi.yaml` | [M] | `CrawlRequest` v2, field table, examples; `Q/docs/CONTRACTS.md` [C] map-section reuse note |
| `GW/domains/knowledge/quarry.rs` | [C] | `normalize_crawl_body` (386-416) becomes an allowlist builder for the UI surface (`max_pages` clamp 5000, `max_depth`, `include_paths`, `exclude_paths`, `allow_subdomains`, `allow_external_links`, same caps as D3 enforced again so the gateway fails fast with its own envelope) and a `Surface::Model` variant used by the action path that drops `allow_external_links` and any key containing `robots`; `ignore_robots*` is never forwarded on either surface until slice 7 |
| `GW/domains/actions/dispatchers.rs` | [C] | `dispatch_crawl_site` uses `Surface::Model` |
| `FE/shared/actions/action-registry.ts` | [C] | `crawlSiteInput` (568-571) gains optional `maxDepth`, `includePaths`, `excludePaths`, `allowSubdomains`; **no** external-links or robots key; a vitest asserts no key of the schema contains the substring `robots` or `external` (case-insensitive) (`apps/gateway/scripts/action-surface-contract.test.mjs` is unaffected because no action ID is added) |
| `FE/shared/api/knowledge-client.ts` | [C] | `startCrawl` (435-456) and the `CrawlRequest` type gain the options; effective-scope echo read from the ack for display |
| `FE/features/dashboard/home/KnowledgeComposer.tsx` | [M] | crawl options: "Stay on this site" (default), "Include subdomains", "Follow links to other sites", collapsible include and exclude path lists with the caps shown; nb and en copy |
| `QE/map_routes.rs` | [C] | `MapRequest` accepts the same scope fields through the shared evaluator (soft, only if F-t2-055 has not rewritten the file) |

**API / data model.** `CrawlRequest` v2 per D8; the 400 body `error.details.fields[] = {path, code, message}` (for example `include_paths[3]`, `invalid_pattern`); `HandoffAck.effective`. No migration.

**Flags and rollout.** `QUARRY_EDGE__CRAWL_REQUEST_VALIDATION=lenient|strict` (default `lenient`, same semantics and naming as D5's scrape request). The new fields are optional and additive; rollback: revert.

**Tests.** Rust: table-driven request parsing (every field, alias, cap boundary at 100/101 entries, 512/513 characters, 8,192/8,193 total, reserved keys, `sitemap:"include"`, `scrape_options` present, `ignore_robots_txt:true`); end-to-end through the edge router with the orchestrator double: stamped params contain the full scope. Gateway: `model_surface_drops_external_and_robots_keys`, `ui_surface_forwards_scope_fields`, `crawl_site_action_goes_through_the_ssrf_normalizer`, `max_pages_is_clamped_on_the_action_path`. FE: vitest for the options and the schema guard; Playwright `tests/e2e/*.spec.ts` optional. Commands: `cargo test -p quarry-edge crawl_request`, `cargo test --manifest-path apps/gateway/Cargo.toml --all-targets`, `cd "apps/Frontend Plane/verevonv3" && pnpm test && pnpm typecheck && pnpm lint`.

**Definition of done.** A Model-proposed crawl cannot carry an external-links or robots key by any path; patterns are capped identically at gateway and edge; the OpenAPI document and `CRAWL_SCOPE_ROBOTS.md` list every field.

**Deployment evidence.** Rebuilt edge, orchestrator, gateway and SPA images. Flows (each asserted against fixture access logs and job events): (a) `include_paths:["/docs/*"]` fetches only `/` and `/docs/a`; (b) `exclude_paths:["re:^/blog/"]` skips `/blog/b` with `exclude_pattern_hit` in `denials.counts`; (c) `include_paths:["re:(a+)+$"]` is accepted and the crawl completes in normal time (no ReDoS); (d) 101 patterns returns 400 `fields[100]`; (e) `ignore_robots_txt:true` returns 403 `policy_denied`; (f) `sitemap:"include"` returns 400 `unsupported_option`; (g) through the gateway, invoke the `knowledge.crawl_site` action with `{url, allowExternalLinks:true, ignoreRobots:true}` and assert the edge-side job params contain neither key and fixture-b receives zero requests; (h) the SPA crawl options produce the expected params (inspect the job).

**Effort.** M-L, 6 engineer-days. **Risks.** Strict validation can break existing clients that send stray keys: lenient first, warnings echoed. Gateway and edge cap drift: one shared constant table in `CRAWL_SCOPE_ROBOTS.md` and a test in each tree that reads the documented numbers. Overlap with F-t2-055 on `map_routes.rs` and F-t2-005 on request types (section 5).

### Slice 7. Ignore-robots entitlement and audited use (blocked, section 8)

**Goal.** An org that has been granted the override can run a crawl with `ignore_robots_txt:true`; every use is audited before the job exists; the Model can never ask for it.

**Changes.**

| Path | Tag | Change |
|---|---|---|
| `QE/crawl_request.rs` | [NEW from slice 5] | when `ignore_robots_txt` is true: `resolved_policy.require(RobotsIgnore, Principal::Human)` (D7), refuse for any service principal (`claims.is_service()`), stamp `params.robots = {mode:"ignore", authority:"web.robots_override", rev}` and `params.user_id` |
| `QE/routes.rs` | [M] | `crawl_handoff` wiring; `HandoffAck.effective.robots` |
| `QE/internal_auth.rs` and `ORCH/activities/activities.go` | [C] | extend the run-binding canonical string (`verify_run_binding`, internal_auth.rs:194-209; Go `runBindingSig`, activities.go:472-476) to cover a sha256 of the canonical-JSON `crawl.robots` so a captured `(org, url, signature)` cannot be replayed with `robots:"ignore"`; **one change shared with D7 S6b's `policy_snapshot` hash** (both edits land in the same PR or the second rebases onto the first), with the two-sided golden test the file already has |
| `CTL/audit/client.go` | [NEW] | audit-core client following `integration-corev2/internal/controlplane/clients.go` `AuditClient.RecordAudit` (`POST /v1/audit`, service token, producer pinned to `ingestion`) |
| `CTL/resources/resources.go` | [C] | `createJob` (396-458): when `params.robots.mode == "ignore"`, write the audit event (`event:"crawl.robots_ignored"`, `resource_id: job_id`, details `{seed_host, authority, rev}`, `user_id` from params) **before** `Jobs().Create` returns 201; an audit failure returns 503 and creates nothing |
| `Q/services/quarry-control/cmd/control/main.go` | [C] | read `AUDIT_CORE_URL` and `AUDIT_CORE_SERVICE_TOKEN`; override jobs are refused when unset |
| `apps/Control Plane/docker-compose.yml` | [C] | `AUDIT_CORE_SERVICE_CREDENTIALS` (line 829-830): add `{"principal":"quarry-control","audience":"audit-core","token":"${QUARRY_CONTROL_AUDIT_CORE_SERVICE_TOKEN:?...}","scopes":["audit:write"],"planes":["ingestion"]}` |
| `apps/Control Plane/audit-core/internal/api/service_auth_test.go` | [C] | a second ingestion writer is accepted; a writer without `planes` is still rejected |
| `apps/Ingestion Plane/docker-compose.yml` | [M] | quarry-control env `AUDIT_CORE_URL`, `AUDIT_CORE_SERVICE_TOKEN` (live diff exists, additive) |
| `FE` crawl options | [M] | the "Ignore robots.txt" control is rendered only when the org's effective web policy permits it (D7 S7 `GET /orgs/:id/web-policy` through the gateway), with a warning copy |

The entitlement itself (`web.robots_override`, the `robots.max_mode` ceiling, org-core audit rows on change, the signed `web_policy` claim) is D7 slices S2 and S7; this slice only consumes it.

**API / data model.** `params.robots` object; one audit event type; no control migration. **Flags.** None beyond D7's `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT`. **Rollback:** the override request path returns 403 again by reverting the `require` call; existing audit rows stay.

**Tests.** Edge: refused without entitlement, refused for a service principal, allowed with policy and stamps params; quarry-control Go: `TestCreateJob_RobotsIgnore_WritesAuditBeforeAck`, `TestCreateJob_RobotsIgnore_AuditFailureCreatesNoJob`, `TestCreateJob_Respect_WritesNoAudit`; audit-core Go as above; gateway: the Model surface still drops the key. Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./... -count=1`; `cd "apps/Control Plane/audit-core" && go test ./internal/api/ -count=1`; `cargo test -p quarry-edge crawl_request`.

**Definition of done.** Every override job has exactly one audit event written before acknowledgement; no path lets the Model set it; documented grantor.

**Deployment evidence.** Control Plane (audit-core, org-core) and Ingestion stacks both running. With org A granted `web.robots_override` through the D7 admin API: crawl fixture-a with `ignore_robots_txt:true`; the fixture-a log shows `GET /private/secret` and **no** `GET /robots.txt` for that run; `GET /v1/audit` (audit-core, read scope) returns the `crawl.robots_ignored` event with `resource_id == job_id`. Stop audit-core and repeat: the create call returns 503 and the fixture log shows no request. Org B (not granted): 403 `policy_denied`. Through the gateway action path with `ignoreRobots:true`: the key is absent from the job params.

**Effort.** M, 4 engineer-days in Ingestion plus about 2 engineer-days of Control Plane config and tests. **Risks.** A cross-plane credential and a new coupling (quarry-control to audit-core): fail closed, documented. Entitlement semantics are D7's open question 3.

### Slice 8. Rollout closeout

**Goal.** Make the safe behaviour the default and remove the migration scaffolding.

**Changes.** `apps/Ingestion Plane/docker-compose.yml` [M] and `QE/crawl_policy.rs` [NEW from slice 3]: default `QUARRY_EDGE__CRAWL_SCOPE_MODE` and `QUARRY_EDGE__CRAWL_ROBOTS` to `enforce`; `ORCH/workflows/workflows.go` [C]: delete the legacy branch and the `GetVersion` call once `temporal workflow list --query 'ExecutionStatus="Running" and WorkflowType="CrawlJobWF"'` shows no pre-gate executions (30-minute bound, wait one hour); remove the characterization tests of slice 0; `Q/docs/ROADMAP.md` [C] Phase 5 status line with the `CrawlFrontier` keep-or-delete note; `apps/STALE_DOC_DELETION_REGISTER.md`.

**Tests and evidence.** The harness `--expect enforce` green on rebuilt images after the flip; the soak summary from the `observe` period (percentage of crawls that would have lost pages to scope, number of robots would-denies, `robots_unreachable` rate) attached to the PR; a final replay test with histories from both builds.

**Effort.** S, 1 engineer-day. **Risks.** Removing the version gate while a stuck old execution exists: check the running list first; the gate removal is a separate commit that can be reverted alone.

### Effort summary

| Slice | Effort | Engineer-days |
|---|---|---|
| 0 docs and characterization | S | 1.5 |
| 1 fixture harness | M | 3 |
| 2 Rust filter and run_page context | M | 4 |
| 3 Go scope default, denials, batch enqueue, UI summary | M | 6 |
| 4 robots gate and Crawl-delay | L | 7 |
| 5 batch check | S | 2 |
| 6 vocabulary, gateway, action, UI | M-L | 6 |
| 7 override entitlement and audit (blocked) | M | 4 (+2 Control Plane) |
| 8 closeout | S | 1 |
| **Total** | **L overall, XL if slice 7 is counted with D7** | **about 34.5** |

The audit's "step 1 is S" corresponds to slices 2 and 3 together, 10 engineer-days here, because of replay gating, the untested production loop and the dead typed-classification path. Without slice 7 the shippable compliance posture (scope, robots, batch, vocabulary) is about 30.5 engineer-days; slice 7 adds 4 in Ingestion and about 2 in the Control Plane.

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
