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
