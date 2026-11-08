# S5 crawl-orchestration-go

Sweep S5 (Haiku code map). Items: F-t2-016, F-t2-017, F-t2-018, F-t2-031, F-t2-040, F-t2-041.
Source: live tree `C:/dev/CoresSystem` (HEAD afb83928 plus uncommitted entries). Read-only. No product file was edited. Tags come from `tools/provenance.sh`; the worktree copy was not used for product code.

Path prefixes used below (all repo-relative, every path tagged):
- `Q:` = `apps/Ingestion Plane/Quarry-v2/`
- `INT:` = `apps/Ingestion Plane/integration-corev2/`
- `GW:` = `apps/Frontend Plane/verevonv3/apps/gateway/src/`
- `FE:` = `apps/Frontend Plane/verevonv3/src/`
- Tags: `[C]` committed and unchanged; `[M]` committed but modified in the live tree; `[U]` untracked. Every `Q/services/*`, `Q/pkg/*` and `Q/crates/*` file not listed in `live-uncommitted.tsv` is `[C]`.
- Verdicts: OK, DRIFTED (corrected range given), FALSE (what the code does), MISSING.
- Line counts were read from the live tree at the time of the sweep.

## Cross-item overlaps (merge-conflict hotspots)
1. `Q:crates/quarry-edge/src/routes.rs` [M]: 016 (CrawlRequest 532-553, crawl_handoff 590-667), 017 (routes 133-134, 231-234), 018 (schedule routes 254-276), 031 (no webhook field), 040 (BatchRequest 669-679, batch_handoff 681-725), 041 (InternalRunPageResult 771-810, internal_run_page 898).
2. `Q:services/quarry-orchestrator/internal/workflows/workflows.go` [C]: 016 (CrawlJobWF enqueue 474-485, durable enqueue 574-584), 017 (BatchJobWF has no handlers, 359-383), 018 (ChangeMonitorWF run_id 308-312), 031 (run_completed at 289-292, 350-354, 379-382, 508-511, 607-609), 040 (sequential batch loop 370-377), 041 (exact-string dedupe 479-482, durable request id 617-620).
3. `Q:services/quarry-orchestrator/internal/workflows/signals.go` [C] and `.../internal/jobs/dispatcher.go` [C]: 017 (handlers only in CrawlJobWF; tick 148-203), 040 (BatchJobWF must install handlers; batchInputFromJob 456-469), 016 (crawlInputFromJob 432-454).
4. `Q:services/quarry-control/internal/resources/resources.go` [C] (650 lines): 017 (getJob 486-501, updateJob 513-549, jobEvents 557-577), 018 (MountSchedules 44-59, createScheduleHandler 113-130), 031 (mountSimple 331-369, fanoutWebhooks 612-650), 040 (createJob 396-458).
5. `Q:services/quarry-control/internal/store/store.go` [C] and `.../store/pg/resources.go` [C]: 017 (Job 174-212), 018 (Schedule 333-356; pg insert 744-750), 031 (Webhook 376-383), 040 (FindByIdempotencyKey 145-150; pg lookup 93-94).
6. `Q:services/quarry-control/internal/store/pg/migrations/` [C]: next free sequence is **015** (001-014 exist; none uncommitted). 018, 031 and 040 each need a schema change, so they must be sequenced 015, 016, 017.
7. `Q:crates/quarry-edge/src/resource_routes.rs` [C] and `.../handoff.rs` [C]: 017 (list_job_events 1057-1130), 040 (forward_mutation random key 471-474; handoff sends no key, 59-69), 016 (crawl handoff payload).
8. `GW:domains/knowledge.rs` [C], `GW:domains/knowledge/quarry.rs` [C], `GW:domains/ingestions/schedules.rs` [C], `GW:domains/ingestions/runs.rs` [C]: 016 (normalize_crawl_body 386-416), 017 (no status or cancel routes, 179-194), 018 (create_schedule 99-155), 040 (start_crawl 306-376, normalize_batch_body 423), 041 (discover 507-538).
9. `FE:shared/actions/action-registry.ts` [C] with `GW:domains/actions/handlers.rs` [C]: `apps/Frontend Plane/verevonv3/apps/gateway/scripts/action-surface-contract.test.mjs` [C] asserts exact set equality of registry IDs and handler IDs, so any new action changes both. Touched by 016 (crawlSiteInput 568-571), 017 (ingestions.* 2563-2594), 018 (ingestionsCreateScheduleInput 887-897).
10. `Q:docs/openapi.yaml` [M] (CrawlRequest 4361-4374, BatchRequest 4375-4385) with `Q:docs/GAP.md` [C] (robots rows) and `Q:docs/ROADMAP.md` [C] (Phase 5 and 9): touched by 016, 017, 018, 031, 040, 041.

Global notes: `Q:scripts/tests/docs-contract.test.mjs` is [U]. `.github/workflows/quarry-docs-contract.yml` runs it, but `git cat-file -e afb83928:<path>` fails, so that CI gate exists only in the uncommitted tree. `.github/workflows/verevonv3-ci.yml` [M] and `apps/Frontend Plane/verevonv3/package.json` [M] carry unrelated demo changes.

---

## F-t2-016 Scoped, robots-compliant crawl

### 1. Citation check (card and JSON evidence; 22 most load-bearing of about 30)
| Citation | Tag | Verdict | Finding |
|---|---|---|---|
| Q:services/quarry-orchestrator/internal/workflows/workflows.go:396-516 (CrawlJobWF) | [C] | DRIFTED | Function is 396-512. Enqueue loop is 473-485: depth check at 474, then every `res.Links` entry, no scope or robots |
| workflows.go:518-610 (durable variant) | [C] | OK | Depth-only check at 574; enqueue of every link at 575-584 |
| workflows.go:474-486 | [C] | OK | Same as above |
| workflows.go:570-585 | [C] | OK | Durable enqueue 575-584 |
| workflows.go:75-110 (inputs) | [C] | OK | CrawlJobInput 75-87 has Seeds, MaxDepth, MaxPages, Policy only. Policy is never read (grep `in.Policy` empty) |
| workflows.go:174-202 (page events) | [C] | OK | Failure event is `cat.Event()` from category (errs.go:72-80), not a robots event |
| Q:crates/quarry-edge/src/routes.rs:533-552 (CrawlRequest) | [M] | OK | 533-553: url, max_pages, max_depth, ingest, zdr. No scope fields. Live diff is one test-module line at 1329 |
| routes.rs:553-590 (ZDR refusal) | [M] | OK | refuse_durable_job_under_zdr 563-588 |
| routes.rs:817-840 (links) | [M] | DRIFTED | `from_output` is 817-841; `links` at 823 maps every href |
| Q:crates/quarry-transform/src/links.rs:1-23 | [C] | OK | `extract` returns all `a[href]`; no host filter |
| Q:crates/quarry-edge/src/map_routes.rs:87-135 | [C] | OK | merge_and_scope 87-109; fetch_text 118-128 calls `state.driver.fetch` |
| map_routes.rs:160-260 | [C] | OK | SSRF heuristic 152-162; robots fetch 170-185; sitemap 187-216; robots allow-filter 250-259 |
| Q:crates/quarry-runtime/src/robots_cache.rs:160-205 | [C] | OK | ReqwestRobotsFetcher 165-205: plain reqwest, 10 s timeout, https then http, no DNS pin |
| Q:crates/quarry-runtime/src/crawl_frontier.rs:54-380 | [C] | OK | FrontierConfig 54-67; CrawlFrontier 178-; enqueue 238-305 |
| Q:crates/quarry-core/src/crawl_denial.rs:15-16 | [C] | OK | RobotsDisallowed at line 16 |
| GW:domains/knowledge/quarry.rs:306-331 | [C] | DRIFTED | start_crawl is 306-376; batch branch 321-325, crawl branch 326-330 |
| GW:domains/knowledge/quarry.rs:386-405 | [C] | OK | normalize_crawl_body 386-416 forwards url, max_pages (clamped to 5000, line 404), ingest only |
| FE:shared/actions/action-registry.ts:568-571 | [C] | OK | crawlSiteInput (url, maxPages) used by knowledge.crawl_site at 1320-1329 |
| Q:services/quarry-orchestrator/internal/jobs/dispatcher.go:440-460 | [C] | DRIFTED | crawlInputFromJob 432-454; batchInputFromJob 456-469 |
| Q:docs/ROADMAP.md:36-38 | [C] | OK | Phase 5 bullets |
| Q:crates/quarry-runtime/src/lib.rs (RobotsCache "only in its own module") | [M] | OK, with note | Re-exported at lib.rs:165 and never called. Re-export only |

Not checked (cap): GW public_url.rs, pipeline.rs:305 (security preflight), and reference-side claims (Firecrawl, Apify).

### 2. Affected files
- Q:services/quarry-orchestrator/internal/workflows/workflows.go [C] 640 lines. CrawlJobWF 396-512 (enqueue 473-485); crawlJobDurableFrontier 518-610; runPage 174-232 (call sites 281, 372, 463, 569). Registered in cmd/orchestrator/main.go:100-105.
- Q:services/quarry-orchestrator/internal/jobs/dispatcher.go [C] 551. crawlInputFromJob 432-454 (MaxDepth and MaxPages only). Scope fields belong here.
- Q:services/quarry-orchestrator/internal/activities/activities.go [C] 476. RunPageInput 165-181 has no scope or robots field. RunPage 211-240 posts url, org_id, user_id, ingest. FrontierEnqueue 101-110.
- Q:crates/quarry-edge/src/routes.rs [M] 1564. CrawlRequest 532-553; crawl_handoff 590-667 (payload 630-638, meter 646-659 before ack check); BatchRequest 669-679. Call sites: forward_to_orchestrator at 626 and 688; refuse at 596 and 687.
- Q:crates/quarry-edge/src/handoff.rs [C] 126. forward_to_orchestrator 26-.
- Q:crates/quarry-edge/src/map_routes.rs [C] 459. host_in_scope 78-83; merge_and_scope 87-109 (called at 251); robots 170-185 and filter 250-259; MAP_UA = "QuarryBot" (line 23).
- Q:crates/quarry-runtime/src/robots_cache.rs [C] 348. RobotsCache 48-58; TTL 24 h (66), negative TTL 1 h (67); ReqwestRobotsFetcher 165-205; fail-open at 213-217.
- Q:crates/quarry-runtime/src/crawl_frontier.rs [C] 651. FrontierConfig 54-67 (defaults max_depth Some(5) line 72, max_pages Some(1000) line 73); host_in_scope 87-101; glob_match 120; enqueue 238-305.
- Q:crates/quarry-runtime/src/pipeline.rs [C] 1503. Preflight 305-312 emits PageBlocked. This is the natural robots-preflight insertion point. links::extract at 507.
- Q:crates/quarry-runtime/src/policy.rs [C]. DiscoveryPolicy 58-80 (respect_robots, include_subdomains, use_sitemap); no consumer found.
- Q:crates/quarry-transform/src/robots.rs [C] 262 (sitemaps 14; parse 80-81; is_allowed 98) and sitemap.rs [C] 152.
- Q:crates/quarry-core/src/crawl_denial.rs [C] 141 (enum 12-35).
- GW:domains/knowledge/quarry.rs [C] 1158 (normalize_crawl_body 386-416; normalize_batch_body 423-; tests 998-1017).
- FE:shared/actions/action-registry.ts [C] 3151 (crawlSiteInput 568-571; knowledge.crawl_site 1320-1329). GW:domains/actions/handlers.rs [C] 549 (arm at line 110).
- Q:docs/openapi.yaml [M] (CrawlRequest 4361-4374 lists only url and max_pages). Q:docs/CONTRACTS.md [C] 352-368 documents include_paths, exclude_paths, allow_subdomains and ignore_robots_txt, none of which exist in code.
- No schema change needed for scope or robots. If scope is persisted on the job record, the next migration is `Q:services/quarry-control/internal/store/pg/migrations/015_*.sql` [NEW].
- Control Plane ignore-robots entitlement: [UNVERIFIED], not in this sweep's scope (`apps/Control Plane`).

### 3. Reusable but unwired
- `CrawlFrontier` (crawl_frontier.rs:178): no production caller. Proof: `grep -rn "CrawlFrontier" crates services --exclude-dir=.claude` returns lib.rs:112-113 (re-export), serp.rs:7 (doc comment) and crawl_frontier.rs. postgres_queue.rs:44 and :644 use only FrontierCheckpoint and FrontierConfigSnapshot.
- `CrawlDenialReason` (crawl_denial.rs:12): used only by the frontier and a doc comment in quarry-core event.rs:55.
- `RobotsCache`, `RobotsFetcher`, `ReqwestRobotsFetcher` (robots_cache.rs:49, 161): no caller; `grep -rn "RobotsCache\|ReqwestRobotsFetcher" crates --exclude-dir=.claude` returns only lib.rs:165 and the module.
- Live robots parsing used only by /v1/map: map_routes.rs:175 and 256 (`RobotsTxt::parse`, `is_allowed`). `robots_cache.rs:129` is the unwired caller.
- `filter_links`: absent in Verevon (grep empty). Reference-only.
- Wired and reusable: `resolve_public_url` (dns_guard.rs:104; callers fetch.rs:14, browser_egress_proxy.rs:281, agent_routes.rs:130); pinned StaticDriver client (fetch.rs:59, 105, 132, 164).

### 4. Tests
- Go: `Q:services/quarry-orchestrator/internal/workflows/workflows_test.go` [C]. TestCrawlJobWF_RespectsMaxPages (200), TestCrawlJobWF_DropsDuplicateUrls (306), TestCrawlJobWF_CancelSignalEndsCrawlEarly (235), TestCrawlJobWF_QueriesReportProgress (273). No scope or robots test.
- Rust: map_routes.rs tests same_host_in_scope (347), external_host_rejected (352), merge_dedups_and_scopes (364). crawl_frontier.rs tests seed_then_enqueue_dedup (426), external_host_rejected_when_not_allowed (474), exclude_pattern_rejects (518), include_pattern_miss_rejects (531). robots_cache.rs distinct_hosts_are_cached_independently (305). quarry-transform robots.rs sitemaps_collected (231).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./internal/workflows/ -run CrawlJobWF -count=1`; `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-runtime crawl_frontier && cargo test -p quarry-runtime robots_cache && cargo test -p quarry-transform robots && cargo test -p quarry-edge map_routes`.
- CI: `.github/workflows/quarry-v2-ci.yml` [C] runs `cargo test --workspace` (lines 204-205; default features) and `cargo llvm-cov --workspace --all-features` (304), plus Go coverage for the orchestrator with an 80% floor (315-329). Gateway tests run in `verevonv3-ci.yml` [M] (`cargo test`, line 141).

### 5. Feature flags and env
- `activities.go:67`: `DurableFrontier` is true only when `EDGE_BASE_URL`, `EDGE_AUTH_TOKEN` and `QUARRY_INTERNAL_SECRET` are all set. Compose sets all three on the orchestrator (`Q:deploy/compose/docker-compose.yml` [C] lines 179, 189, 190). So production runs the durable variant (workflows.go:413-415); the in-workflow BFS is the fallback. Scope work must cover both.
- `cmd/orchestrator/main.go:51`: `EDGE_BASE_URL` defaults to `RUNTIME_BASE_URL`.
- RobotsCache TTL 24 h (robots_cache.rs:66), negative TTL 1 h (67), fetch timeout 10 s (173).
- `DiscoveryPolicy` defaults (policy.rs:74-77): respect_robots true, include_subdomains false, use_sitemap true. No enforcement reader (grep of `.discovery.` finds only policy.rs tests at 384).
- No robots-override flag or entitlement exists in Quarry code (grep `ignore_robots` and `respect_robots` returns only policy.rs).
- User-agent tokens differ: `Quarry/2.0 (+https://triodelab.com/quarry)` (`Q:crates/quarry-edge/src/config.rs` [M] line 302); `QuarryBot` (map_routes.rs:23); `Mozilla/5.0 (compatible; QuarryBot/1.0; +https://verevon.io/bot)` (`Q:crates/quarry-edge/src/main.rs` [M] line 1263).
- Gateway clamps: max_pages at 5000 (quarry.rs:404), batch 200 (quarry.rs:424), map limit 200 (quarry.rs:469).

### 6. Surprises
- Stale claims that robots is closed: `Q:docs/GAP.md` [C] lines 178, 202, 292, 316, 419, 431, 494 mark robots or sitemap crawl "Closed". Code says no. Line 316 cites `crates/quarry-runtime/src/robots.rs`, which does not exist (ls fails), and says Redis cache; code uses an in-memory HashMap (robots_cache.rs:49-51). Also `Q:docs/Features.md` [M] line 27 and `Q:docs/gap-quarry.md` [C] line 219 claim robots respect for crawl. False.
- ReqwestRobotsFetcher fails OPEN on 5xx and transport errors (robots_cache.rs:213-217 returns `Ok(None)`). RFC 9309 says 5xx means disallow-all. The card's "fetch errors fail open" is correct for the current code.
- CrawlFrontier treats every subdomain as in scope (crawl_frontier.rs:97-99, `ends_with(".{root}")`). There is no allowSubdomains toggle; a default of same-host needs a new switch.
- Include and exclude patterns are glob, not regex (crawl_frontier.rs:119-120: "No regex/glob crate needed"). The audit's ReDoS caps assume regex. Glob is linear by construction, so the plan can keep it.
- Frontier defaults (max_depth 5, max_pages 1000) differ from the workflow semantics where 0 means unlimited (workflows.go:474, 456).
- crawl_handoff and batch_handoff run no security preflight. crawl_handoff only parses the URL (routes.rs:603). The per-page preflight runs later in run_page (pipeline.rs:305).
- CrawlRequest and BatchRequest carry no `deny_unknown_fields` (routes.rs:532-533, 669-670). Unknown scope keys are silently dropped.
- `knowledge.crawl_site` is `requiresApproval: false` (action-registry.ts:1325). The card's "the user approves in the UI" is not enforced by the registry.
- `CONTRACTS.md` [C] 352-368 and 429 document field names and denial codes (Firecrawl-style `depth_limit`, `robots_txt`) that do not match the code's CrawlDenialReason serde tags (`depth_exceeded`, `robots_disallowed`).

---

## F-t2-017 Public async job lifecycle

### 1. Citation check (card and JSON evidence; 24 entries)
| Citation | Tag | Verdict | Finding |
|---|---|---|---|
| Q:crates/quarry-edge/src/routes.rs:125-293 (route table) | [M] | OK | Crawl and batch POST only (133-134). No GET or DELETE on /v1/crawl/:id, no pause or cancel. Jobs list 192, events 231-234 |
| routes.rs:133-134 | [M] | OK | Both POST handoffs |
| routes.rs:229-232 | [M] | OK | Comment 227-230; route 231-234 |
| Q:crates/quarry-edge/src/resource_routes.rs:976 "(list_job_events)" | [C] | FALSE (label) | 976 is `list_run_events` (`/v1/runs/:id/events`). list_job_events is 1057-1130 |
| resource_routes.rs:1037-1080 and 1057-1127 | [C] | DRIFTED | Handler 1057-1130. Drains control once (1081-1107); terminal `done` frame at 1127 |
| resource_routes.rs:1040-1057 (drain comment) | [C] | OK | Comment 1036-1056 |
| resource_routes.rs:955-1130 | [C] | OK | RunEventsQuery with after_seq at 968-973 |
| Q:services/quarry-orchestrator/internal/workflows/signals.go:87-146 | [C] | OK | installControlHandlers 87-136 (queries 124-133); waitWhilePaused 140-146 |
| signals.go:80-146 | [C] | OK | Same |
| workflows.go:400 | [C] | OK | Only CrawlJobWF calls installControlHandlers |
| workflows.go:359-410 and 359-383 (BatchJobWF has no handlers) | [C] | OK | No signal channels in BatchJobWF |
| workflows.go:435-455 and 488-496 | [C] | OK | Pause and resume 435-451; checkpoint every 50 pages 488-495 |
| Q:services/quarry-orchestrator/internal/jobs/dispatcher.go:236-300 | [C] | DRIFTED | dispatch is 244-309; workflow id 246; 30-minute timeout 254 |
| dispatcher.go:246 and 254 | [C] | OK | Confirmed |
| Q:services/quarry-orchestrator/cmd/orchestrator/main.go:55-70 | [C] | OK | Edge env 51-64; `client.Dial` at 66 |
| main.go:125-160 | [C] | OK | Schedules manager 132-139; jobs manager 145-152 |
| Q:services/quarry-control/internal/temporal/client.go:91-150 | [C] | OK, with correction | NoopClient 91-109 is never instantiated (main.go passes `nil`, `Q:services/quarry-control/cmd/control/main.go` [C] line 192). Its methods return nil, although the doc at 84-88 says ErrNotConfigured |
| Q:services/quarry-control/internal/resources/resources.go:24-31 | [C] | OK | MountJobs 24-33 |
| resources.go:486-530 (GET and PUT job) | [C] | DRIFTED | getJob 486-501 is org-scoped. updateJob 513-549 has no org check and its patch takes only status, run_id, policy (521-525) |
| GW:domains/knowledge.rs:179-196 | [C] | OK | Routes 179-194: start, discover, list, events only |
| knowledge.rs:185 | [C] | OK | crawl run events 184-187 |
| GW:domains/knowledge/quarry.rs:1098-1120 (Last-Event-ID) | [C] | OK | Header read at 1119, forwarded through proxy_sse_stream (1113-1119). The edge drops it (see surprises) |
| quarry.rs:1060-1140 | [C] | OK | list_crawl_jobs 1074; crawl_run_events 1098; crawl_job_events 1132 |
| FE:shared/actions/action-registry.ts:1316-1330 and 1320 | [C] | OK | knowledge.crawl_site at 1320 |
| action-registry.ts:2558-2590 | [C] | OK | ingestions.create_run 2563, create_schedule 2574, run_schedule_action 2585. No status or cancel ID |
| GW:domains/ingestions/runs.rs:27 | [C] | OK | RUN_KINDS const |

### 2. Affected files
- `Q:crates/quarry-edge/src/routes.rs` [M] 1564. Add GET and DELETE /v1/crawl/:id (and batch aliases), pause and resume, errors, ongoing. Route table 125-293.
- `Q:crates/quarry-edge/src/resource_routes.rs` [C] 1252. list_run_events 976-; list_job_events 1057-1130 (drain 1081-1107). Events handler takes no headers (no Last-Event-ID).
- `Q:crates/quarry-edge/src/state.rs` [M] 155 (`control_base_url` at 47, `driver` at 39).
- `Q:services/quarry-control/internal/resources/resources.go` [C] 650. getJob 486-501; updateJob 513-549 (unscoped); jobEvents 557-577 (org-checked, after_seq at 569); jobHistory 582-. Caller: orchestrator markRunning and markFailed in dispatcher.go (PUT, 313-333).
- `Q:services/quarry-control/internal/store/store.go` [C] 923. Job 174-212 has no origin, desired-state or steps field. Events ForJob called from resources.go:574.
- `Q:services/quarry-control/internal/store/pg/resources.go` [C] 1097. jobs List 241-251 orders by `created_at DESC, id DESC`.
- `Q:services/quarry-control/internal/temporal/client.go` [C] 156. Client interface 75-82; NoopClient 91-109.
- `Q:services/quarry-control/go.mod` [C]: no `go.temporal.io/sdk` (grep "temporal" returns nothing). Orchestrator go.mod has sdk v1.29.1.
- `Q:services/quarry-orchestrator/internal/workflows/signals.go` [C] 146; workflows.go [C] 640.
- `Q:services/quarry-orchestrator/internal/jobs/dispatcher.go` [C] 551. Tick 148-203 marks any non-accepted job done (165-169), so the existing poll loop cannot deliver cancel or pause to running jobs without change.
- `GW:domains/knowledge.rs` [C] 196; `GW:domains/knowledge/quarry.rs` [C] 1158; `GW:domains/ingestions/runs.rs` [C] 463; `GW:domains/actions/handlers.rs` [C] 549 (arms 110, 255-267).
- `FE:shared/actions/action-registry.ts` [C] 3151 (ingestions.* 2563-2594); `FE:shared/api/ingestions-client.ts` [C] (exports 161-213; no cancel or status call); `FE:features/dashboard/home/DashboardCrawlRuns.tsx` [C] (no cancel or pause); `FE:features/ingestions/components/VerevonIngestionsPage.tsx` [C] (schedule pause and resume 882-892; trigger 895); `FE:features/knowledge/components/KnowledgePage.tsx` [C] (only a fetch-abort comment at 294).
- `Q:docs/openapi.yaml` [M]: `/v1/jobs/{job_id}/events` at 1232-1262 documents text/event-stream. No status, cancel or pause paths.
- New migration for origin, desired state and steps: `Q:services/quarry-control/internal/store/pg/migrations/015_*.sql` [NEW]. Next free sequence 015.

### 3. Reusable but unwired
- Temporal pause, resume and cancel handlers, signals.go:87-136, installed at workflows.go:400. Proof of no sender: `grep -rn "SignalWorkflow\|\.Signal(" services crates pkg` finds no production call; the only hits are test-environment callbacks in workflows_test.go.
- Query handlers `state` and `progress` (signals.go:124-133): no caller.
- Edge events route with `after_seq` (RunEventsQuery 968-973, honoured by control jobEvents 569): usable for a live tail but currently drains once (resource_routes.rs:1050-1055).
- Control `PUT /v1/jobs/{id}` (resources.go:513-549): reusable for desired state only after adding an org check and a field.
- Gateway `crawl_run_events` and `crawl_job_events` (quarry.rs:1098-1158): forward Last-Event-ID, but the edge ignores it.
- `IdempotencyKeyHandler` (cycle23.go:436-438): no-op, zero callers.

### 4. Tests
- Go: workflows_test.go TestCrawlJobWF_CancelSignalEndsCrawlEarly (235), TestCrawlJobWF_QueriesReportProgress (273). Control: `Q:services/quarry-control/internal/store/jobs_org_test.go` TestMemoryJobs_OrgScopedListAndGet (24), TestMemoryJobs_ListByKindIsOrgScoped (64); store_test.go TestMemoryDB_JobsUpdate (93); resources/job_wire_test.go (32-121).
- Rust: `Q:crates/quarry-edge/tests/sse_stream.rs` [C] sse_streams_page_fetched_and_artifact_written (49). This covers runtime SSE, not job events. No test for list_job_events found.
- Gateway: no test for crawl_run_events or crawl_job_events in the quarry.rs test module (773-1158) [UNVERIFIED for other files].
- Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./internal/workflows/ -run CrawlJobWF -count=1`; `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./internal/store/ -run Jobs -count=1`; `cd "apps/Frontend Plane/verevonv3" && pnpm vitest run src/features/dashboard/home/DashboardCrawlRuns.test.tsx`.
- Contract: `pnpm test` runs `apps/Frontend Plane/verevonv3/apps/gateway/scripts/action-surface-contract.test.mjs` [C]. Adding ingestions.crawl_status or cancel to the registry fails the test unless handlers.rs gets matching arms.
- CI: quarry-v2-ci.yml [C] (cargo test --workspace; Go coverage for control and orchestrator, 315-329); verevonv3-ci.yml [M] (gateway cargo test, 141); quarry-docs-contract.yml checks only /v1/search in openapi.

### 5. Feature flags and env
- Orchestrator: `TEMPORAL_ADDR` (default localhost:7233; compose `temporal:7233`) main.go:44; `TEMPORAL_NAMESPACE` (default `default`; not set in compose) main.go:45; `CONTROL_BASE_URL` (default http://quarry-control:8081) main.go:47; `RUNTIME_BASE_URL` (default http://quarry-runtime:8082) main.go:46; `RUNTIME_AUTH_TOKEN`, `CONTROL_AUTH_TOKEN`, `EDGE_AUTH_TOKEN` (default RUNTIME token) main.go:52-60; `QUARRY_INTERNAL_SECRET` main.go:64.
- Hard-coded: jobs tick 2 s (main.go:150); schedules 30 s (137); jobs list `limit=50` (dispatcher.go:348); workflow execution timeout 30 min (dispatcher.go:254); signal pause poll 5 s (signals.go:30); checkpoint every 50 pages (workflows.go:489, 595).
- `QUARRY_TEMPORAL_HOSTPORT`: read by no code. Referenced only in the 501 message (cycle23.go:310, 354) and compose (`docker-compose.yml` lines 78 and 177). Dead configuration.
- Edge: `postgres-queue` cargo feature (`Q:crates/quarry-edge/Cargo.toml` [C] line 86). Internal queue routes need it (queue_routes.rs:197-203). The deployed edge image enables it (`Q:deploy/compose/Dockerfile.edge` [C] line 102).
- ZDR: durable jobs refused (routes.rs:563-588).

### 6. Surprises
- Control `PUT /v1/jobs/{id}` (updateJob 513-549) has no org check and its patch struct (521-525) has no desired-state field. The approach "edge writes desired state via existing PUT" needs an org-checked route and a new field. Otherwise any HMAC-signed caller can change any org's job.
- Control `GET /v1/jobs/` (listJobs 381-390) is unscoped. The orchestrator polls it with `limit=50` (dispatcher.go:348) across all orgs, ordered by created_at DESC (pg/resources.go:251). Accepted jobs older than the newest 50 rows of any status may never be dispatched [INF, not executed].
- The jobs tick (dispatcher.go:165-169) marks any non-accepted job done. A cancel or pause loop must be a separate path.
- The card says quarry-control's client is a NoopClient. At runtime it is nil (main.go:192), and NoopClient is unused. NoopClient returns nil, which contradicts its own doc (client.go:84-88, 93-109). If wired, trigger and backfill would silently succeed.
- Job `Policy` is persisted (resources.go:408-410, resolveJobPolicy 467-) but the dispatcher's `job` struct omits it (dispatcher.go:401-407). BatchJobInput.Policy and CrawlJobInput.Policy (workflows.go:71, 86) are never read.
- Gateway start_crawl falls back to `format!("{kind}_{user_id}")` when control returns no job id (quarry.rs:357). That makes the event key non-unique.
- Edge CrawlRequest and BatchRequest have no `deny_unknown_fields`.
- Edge exposes no GET for a single job (no `/v1/jobs/:id` route; route table 125-293).
- Result pagination has no data source. Crawl pages go to events as excerpts (workflows.go:174-232) or to the Data Plane when ingest is on. A decision is needed first.

---

## F-t2-018 Scrape, crawl and batch schedules

### 1. Citation check (card and JSON evidence; 24 entries)
| Citation | Tag | Verdict | Finding |
|---|---|---|---|
| Q:services/quarry-control/internal/resources/schedule_wire.go:39-51 | [C] | OK | Comment 39-43; workflowFor 44-51 (only change_monitor); Paused = !Enabled at 70 |
| Q:services/quarry-control/cmd/control/main.go:185-193 and 177-193 | [C] | OK | MountScheduleAliases(r, db, nil) at 192; comment 176-188 |
| Q:services/quarry-control/internal/resources/resources.go:113-130 | [C] | OK | createScheduleHandler decodes body only. Never reads the `org_id` query parameter (confirmed). Validate needs body org_id |
| Q:services/quarry-control/internal/store/store.go:333-355 | [C] | OK | Schedule 333-356. No name, config, overlap or timezone field |
| Q:services/quarry-control/internal/store/schedule_validate.go:39-55 and 39-62 | [C] | OK | Validate 39-62; org_id 50-52; target_ref 53-55 |
| Q:services/quarry-control/internal/temporal/client.go:1-60 | [C] | OK | Doc and CreateOptions 49-60; Client iface 75-82 |
| Q:services/quarry-orchestrator/internal/schedules/schedules.go:104-330 | [C] | OK | reconcile 104-135; reconcileWith 136-205; create 302-326; update 328-361 |
| schedules.go:136-176 | [C] | OK | Same |
| schedules.go:158-163 | [C] | OK | Skip at 160-163 |
| schedules.go:302-327 | [C] | OK | Create and update set only CronExpressions (313, 335) |
| schedules.go:365-398 | [C] | DRIFTED | Trigger 363-379 (SKIP at 374); Backfill 381-404 (ALLOW_ALL at 398) |
| Q:crates/quarry-edge/src/schedule_routes.rs:239-340 and 45-90 | [C] | OK | CreateScheduleRequest 42-75; ControlScheduleCreate 81-96 (org in body, preset path only, 286-294); create 239-340 |
| schedule_routes.rs:307-338 (raw forward) | [C] | OK | Non-preset path forwards `&req` (330-338). No org or target fields in the body. The `?org_id` query is sent (158) but ignored |
| schedule_routes.rs:165-171 | [C] | OK | Random Idempotency-Key in forward_json (125-180), 169-171 |
| Q:services/quarry-orchestrator/internal/workflows/workflows.go:31-41 | [C] | OK | ScrapeJobInput 31-41 |
| workflows.go:75-87 | [C] | OK | CrawlJobInput 75-87 |
| workflows.go:296-320 and 305-357 | [C] | OK | ChangeMonitorWF 305-355; run_id derivation 308-312 |
| Q:crates/quarry-edge/src/routes.rs:563-588 | [M] | OK | Refusal function. Schedule routes at 254-276 |
| GW:domains/ingestions/schedules.rs:99-141 | [C] | OK | create_schedule 99-155; overlap_policy default "skip" 132-138 |
| schedules.rs:191-256 | [C] | OK | schedule_actions 191-; trigger 231-234; backfill 241-251 (overlap "allow") |
| FE:features/dashboard/home/DashboardCrawlSchedules.tsx:3 | [C] | OK | Import line. Read-only (comment 7-8) |

Verified by reading (not executed): a UI-created scrape, crawl or batch schedule is rejected at create with 400 "org_id required". The card says it "never fires". The rejection is the actual failure: the raw request has no org_id (schedule_routes.rs:330-338), and schedule_validate.go:50-52 rejects it.

### 2. Affected files
- `Q:crates/quarry-edge/src/schedule_routes.rs` [C] 527. CreateScheduleRequest 42-75; ControlScheduleCreate 81-96; create_schedule 239-340; forward_json 125-180.
- `Q:crates/quarry-edge/src/routes.rs` [M] 1564. Schedule routes 254-276 (create, delete, pause, unpause, trigger, backfill).
- `Q:services/quarry-control/internal/resources/schedule_wire.go` [C] 72. workflowFor 44-51; toScheduleWire 54-72.
- `Q:services/quarry-control/internal/resources/resources.go` [C] 650. MountSchedules 44-59 (enable and disable 56-57; runs 58); createScheduleHandler 113-130.
- `Q:services/quarry-control/internal/resources/cycle23.go` [C] 438. MountScheduleAliases 278-289; scheduleTrigger 296-323 (nil check 308-313, call 314); scheduleBackfill 325-.
- `Q:services/quarry-control/internal/store/store.go` [C] 923. Schedule 333-356 (Enabled 343, CreatedBy 350, Preset 355).
- `Q:services/quarry-control/internal/store/schedule_validate.go` [C] 63; `Q:services/quarry-control/internal/store/pg/resources.go` [C] 1097 (schedulesStore.Create 744-750 writes id, org_id, cron, target_kind, target_ref, enabled, created_at, created_by).
- `Q:services/quarry-control/cmd/control/main.go` [C] 270 (line 192).
- `Q:services/quarry-orchestrator/internal/schedules/schedules.go` [C] 404. ScheduleSpec 49-57 (no overlap or timezone); reconcile 104-135; create 302-326; update 328-361; Trigger 363-379; Backfill 381-404.
- `Q:services/quarry-orchestrator/cmd/orchestrator/main.go` [C] 187. schedules.New 132-139 (30 s interval).
- `Q:services/quarry-orchestrator/internal/workflows/workflows.go` [C] 640 (run_id derivation 308-312).
- `Q:services/quarry-control/go.mod` [C]: no Temporal SDK, so an in-control client needs a dependency.
- `GW:domains/ingestions/schedules.rs` [C] 341; `GW:domains/ingestions/shared.rs` [C] (normalize_batch_urls 307).
- `FE:shared/actions/action-registry.ts` [C] (ingestionsCreateScheduleInput 887-897 has no overlap or timezone; ingestionsRunScheduleActionInput 899-914 enumerates pause, unpause, trigger, backfill, delete). `FE:features/ingestions/components/VerevonIngestionsPage.tsx` [C] (pause and resume 882-892; trigger 895; no backfill UI). `FE:shared/api/ingestions-client.ts` [C] (createIngestionSchedule 189; runIngestionScheduleAction 197).
- Migrations: `Q:services/quarry-control/internal/store/pg/migrations/001_init.sql` [C] lines 47-55 (schedules; `enabled DEFAULT TRUE` at 52), `008_schedules_org_id.sql` [C], `009_schedules_created_by.sql` [C].
- New migration for name, config, overlap_policy, timezone, skip reasons: `Q:services/quarry-control/internal/store/pg/migrations/015_*.sql` [NEW].
- `Q:docs/openapi.yaml` [M]: /v1/schedules at 1474; the create body is not documented; pause, unpause, trigger, backfill at 1591-1700.

### 3. Reusable but unwired
- `Manager.Trigger` and `Manager.Backfill` (schedules.go:367, 385): no production caller. Proof: `grep -rn "\.Trigger(\|\.Backfill(" services/quarry-orchestrator` finds only internals (379, 386-404) and tests. Tested by reaper_isolation_test.go:231-240.
- ChangeMonitorWF run_id derivation from `workflow.GetInfo` (workflows.go:308-312): reusable for scheduled fires of the other three workflows.
- Control enable, disable, pause and unpause (resources.go:56-57; cycle23.go:282-283): live, and map to `Paused = !Enabled` (schedule_wire.go:70). This is the explicit enable mechanism.
- Orchestrator reconcile loop, single writer of Temporal schedules (schedules.go:104-135): live.
- Temporal ScheduleSpec overlap and TimeZone: not used anywhere in Quarry (grep finds only Trigger and Backfill overlap constants at 374 and 398).

### 4. Tests
- Control: `Q:services/quarry-control/internal/resources/schedules_test.go` [C] TestCreateSchedule_InvalidCron_Returns400 (36), TestCreateSchedule_Valid_Returns201 (56), TestEnableDisable_TogglesState (118), TestCreateSchedule_MissingOrgID_Returns400 (316), TestCreateSchedule_ChangeMonitor_EmitsWorkflowArgs (334), TestCreateSchedule_Scrape_EmptyWorkflow (396), ScheduleRuns tests (172-270). `cycle23_aliases_test.go` [C]: TestScheduleTrigger_NilClient_Returns501 (42), TestScheduleBackfill_NilClient_Returns501 (73).
- Orchestrator: `schedules_test.go` [C] TestFetchDesired (47), TestCreate_BuildsTemporalScheduleOptions (246), TestUpdate_ReconcilesPauseAndUnpause (283); `reaper_isolation_test.go` [C] TestReconcile_ReapsOnlyStaleOrgSchedules (109), TestCreate_StampsOrgMemo (197), TestTriggerAndBackfill_CallRealScheduleClient (231).
- Edge: `schedule_routes.rs` tests create_schedule_request_requires_cron_xor_schedule_at_check_pattern (454) and control_schedule_create_serializes_store_schedule_shape (494). Neither covers the non-preset path end to end.
- Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./internal/resources/ -run Schedule -count=1`; `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./internal/schedules/ -count=1`; `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge schedule`.
- CI: quarry-v2-ci.yml [C] (cargo test --workspace; Go coverage for control and orchestrator).

### 5. Feature flags and env
- Orchestrator reconcile interval 30 s (cmd/orchestrator/main.go:137), hard-coded.
- Schedule `Paused` derives from `!Enabled` (schedule_wire.go:70). DB default `enabled TRUE` (001_init.sql:52). The pg insert writes the Go struct value (pg/resources.go:748), and only the preset path sets `enabled: true` (schedule_routes.rs:292). Once the org gap is fixed, non-preset creates would persist `false` (the zero value) unless the edge sets it.
- `TEMPORAL_ADDR` and `TEMPORAL_NAMESPACE` (orchestrator main.go:44-45). `QUARRY_TEMPORAL_HOSTPORT` has no reader (see 017).
- `QUARRY_INTERNAL_HMAC_REQUIRED` (control main.go:98); `QUARRY_INTERNAL_SECRET` (97).
- ZDR refusal for durable jobs: routes.rs:563-588.

### 6. Surprises
- The card says "silently never fires". The actual failure is a 400 at create (see citation table).
- Even with an org fix, `config` (url, max_pages, schema; gateway schedules.rs:160-187) has no column (store.go:333-356; pg insert 746-748). `workflowFor` builds args only from TargetRef and OrgID (schedule_wire.go:47). `config.url` never becomes target_ref.
- The card says the UI "exposes overlap policy". False for the UI: no overlap control exists in FE (grep "overlap" in `FE:` finds none). Only the gateway route accepts `overlapPolicy` (schedules.rs:132-138), and the action schema omits it (action-registry.ts:887-897).
- Backfill is not in the UI (grep finds no backfill control). It exists in the registry (909-913) and the gateway route only.
- The UI Trigger button (VerevonIngestionsPage.tsx:895) always returns 501 today, because MountScheduleAliases is called with nil (main.go:192; cycle23.go:308-312).
- The 501 hint tells operators to set `QUARRY_TEMPORAL_HOSTPORT` (cycle23.go:310). No code reads it.
- `ingestions.create_schedule` (action-registry.ts:2579) and `run_schedule_action` (2590) are `requiresApproval: false`.
- `Validate` for non-change_monitor kinds requires target_ref (schedule_validate.go:53-55). The gateway never sends target_ref, so an org-only fix still fails.
- No in-flight uncommitted changes touch these Quarry Go files or the gateway/FE schedule files (provenance [C]). `Q:crates/quarry-edge/src/routes.rs` [M] changes are test-only.

---

## F-t2-031 Tenant-scoped, SSRF-guarded outbound webhooks

### 1. Citation check (card and JSON evidence; 24 entries)
| Citation | Tag | Verdict | Finding |
|---|---|---|---|
| Q:services/quarry-control/internal/dispatcher/dispatcher.go:25-33 | [C] | OK | Backoff 1 s, 5 s, 30 s, 5 m, 1 h (25-31); maxAttempts 5 (33). The card's "5s/30s/5m/1h" omits the 1 s first step |
| dispatcher.go:107-123 | [C] | OK | HMAC over `ts.body` (110-113); header `t=,v1=` (123) |
| dispatcher.go:40-41 | [C] | OK | Default `http.Client{Timeout: 30s}` at 40-42; no CheckRedirect |
| Q:services/quarry-control/cmd/control/main.go:197 | [C] | OK | `dispatcher.Run(dCtx, db, nil, ...)`: nil client |
| Q:services/quarry-control/internal/store/store.go:376-394 | [C] | OK | Webhook 376-383: no org_id; `Secret` has `json:"secret"` (379). WebhookDelivery 385-395 |
| resources.go:615-645 (fanoutWebhooks) | [C] | DRIFTED | Function 612-650. `db.Webhooks().List(1000, "")` (620) spans all orgs |
| Q:services/quarry-control/internal/resources/webhooks.go:8-10 | [C] | OK | mountSimple at `/v1/webhooks` (8-10) |
| resources.go:331-369 (mountSimple; not in card) | [C] | OK | GET returns the stored struct, including `secret` (338-339). Confirms the card's "likely echoes it" |
| Q:crates/quarry-edge/src/change_webhook.rs:23 | [C] | OK | CHANGE_WEBHOOK_PATH only; no webhook CRUD on the edge |
| Q:services/quarry-control/internal/resources/change_webhook.go:1-60 | [C] | OK | MountChangeWebhook 51-53; org read from signed query (57-60) |
| INT:internal/egress/guard.go:208 | [C] | OK | ResolveAndVet 208-236 |
| Q:crates/quarry-runtime/src/dns_guard.rs:98-112 | [C] | OK | resolve_public_url 104-; guard_url 134-136 |
| Q:services/quarry-orchestrator/cmd/orchestrator/main.go:94-151 | [C] | OK | Registrations 94-117; schedules and jobs managers 132-152 |
| Q:services/quarry-orchestrator/internal/workflows/workflows.go:124-148 | [C] | OK | emitEvent with idempotency key |
| workflows.go:289-292, 350, 379-382, 507-511, 602-610 | [C] | OK | run_completed in Scrape (289-292), ChangeMonitor (350-354), Batch (379-382), Crawl (508-511), durable Crawl (607-609) |
| Q:pkg/quarrycontracts/event.go:53-62 | [C] | OK | Event 53-62: no org field |
| Q:crates/quarry-edge/src/routes.rs:130-275 and 533-575 | [M] | OK | No webhook or delivery route. CrawlRequest has no webhook field |
| Q:crates/quarry-runtime/src/nats_event_bus.rs:1-20 | [C] | OK | Subjects `quarry.run.<run>.<type>` and `quarry.events.<type>` |
| FE:features/settings/components/WorkspaceSettingsPage.tsx:1930-1940 | [C] | OK | Placeholder panel 1933-1942 ("No webhook delivery telemetry...") |

### 2. Affected files
- `Q:services/quarry-control/internal/store/store.go` [C] 923. Webhook 376-383 (add org_id; keep Secret write-only); WebhookDelivery 385-395 (add org_id).
- `Q:services/quarry-control/internal/resources/resources.go` [C] 650. fanoutWebhooks 612-650 (filter by org; call sites 273 and `Q:services/quarry-control/internal/resources/change_webhook.go` [C] 164); mountSimple 331-369 (echoes Secret).
- `Q:services/quarry-control/internal/resources/webhooks.go` [C] 10 (generic CRUD at /v1/webhooks).
- `Q:services/quarry-control/internal/resources/webhook_deliveries.go` [C] 33 (list at /v1/webhook-deliveries; retry 16-32).
- `Q:services/quarry-control/internal/resources/change_webhook.go` [C] 221.
- `Q:services/quarry-control/internal/dispatcher/dispatcher.go` [C] 165 (Run 36-42; backoff 25-31; signing 109-123; retryOrDLQ 146-).
- `Q:services/quarry-control/cmd/control/main.go` [C] 270 (MountWebhooks 166; MountChangeWebhook 171; MountWebhookDeliveries 173; dispatcher.Run 197).
- `Q:pkg/quarrycontracts/event.go` [C] 62 (no org field; add org or derive from job).
- `Q:services/quarry-control/internal/store/pg/webhooks_blocklists.go` [C] (Webhook CRUD; line count [UNVERIFIED]).
- Migrations: `002_webhooks_blocklist.sql` [C] lines 6-15 (webhooks: id, url, secret, events, active, created_at; no org_id); `003_webhook_delivery_payload.sql` [C]; `013_change_webhook_idempotency.sql` [C]; 014 is the latest. New migration `Q:services/quarry-control/internal/store/pg/migrations/015_*.sql` [NEW] adds org_id to webhooks and webhook_deliveries.
- Edge: `Q:crates/quarry-edge/src/change_webhook.rs` [C] 190. `Q:crates/quarry-edge/src/routes.rs` [M] (any tenant webhook routes and a per-request `webhook` field on CrawlRequest 533-553 and BatchRequest 669-679 would be added here).
- Shared egress: `INT:internal/egress/guard.go` [C] 302 (Guard 109, ResolveAndVet 208-236); `INT:internal/egress/client.go` [C] 148 (SafeClient 60-108; pinnedDialContext 110; revettingCheckRedirect 138); `INT:go.mod` [C] line 1 (module `github.com/triodelab/integration-corev2`). quarry-control is module `github.com/triodelab/quarry-v2/services/quarry-control`, so it cannot import `internal/egress`.
- `Q:docs/openapi.yaml` [M]: no webhook path (grep empty).

### 3. Reusable but unwired
- `SafeClient` and `ResolveAndVet` in integration-corev2: live callers at `INT:cmd/api/main.go` [C] lines 147, 258, 259, 277 and `INT:cmd/email-worker/main.go` [C] lines 75, 93. Quarry has no import (grep for `egress` in Quarry services returns nothing).
- `resolve_public_url` / `guard_url` (dns_guard.rs:104, 134): live callers fetch.rs:14, browser_egress_proxy.rs:281, agent_routes.rs:130.
- Model Plane `safe_mcp_http_client` (`apps/Model Plane/rust/services/model-gateway/src/runtime_registries.rs` [C] 426-438): caller trace [UNVERIFIED].
- Delivery list and retry routes (webhook_deliveries.go:13-32): live, internal only.
- NATS event bus (nats_event_bus.rs:1-20): live publisher. Internal planes can subscribe without tenant webhooks.
- Capability-core Watch (`apps/Model Plane/go/services/capability-core/internal/watch/watch.go` [C] 1-14, 97): poll-only; source kind `process_output` only (line 97).

### 4. Tests
- Control: `Q:services/quarry-control/internal/dispatcher/dispatcher_test.go` [C]: TestDispatch_Success (86), TestDispatch_RetryOn5xx (123), TestDispatch_DLQOn4xx (156), TestDispatch_RetryOn429 (183), TestDispatch_InactiveWebhook (204), TestDispatch_TransportError (223), TestDispatch_DLQAfterMaxAttempts (251). No SSRF or redirect test.
- `Q:services/quarry-control/internal/resources/change_webhook_test.go` [C]: TestChangeWebhook_HappyPathPersistsDurableEvent (121), TestChangeWebhook_BadSignatureRejected401 (190), TestChangeWebhook_OrgScopingGuards (253), TestChangeWebhook_FanoutCreatesDeliveryRows (281).
- `Q:services/quarry-control/internal/store/store_test.go` [C]: TestMemoryDB_WebhooksCRUDAndList (184), TestMemoryDB_WebhookDeliveriesCRUDAndList (241).
- Edge: `Q:crates/quarry-edge/tests/change_webhook_proptest.rs` [C] changed_records_roundtrip_through_the_webhook_body (72).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./internal/dispatcher/ ./internal/resources/ -run "Dispatch|Webhook" -count=1`; `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-edge --test change_webhook_proptest`.
- CI: quarry-v2-ci.yml [C] (Go coverage for control, 80% floor; cargo test --workspace).

### 5. Feature flags and env
- Control: `NOTIFICATION_CORE_URL` and `NOTIFICATION_CORE_INTERNAL_KEY` (main.go:57-58; change notify sink only); `QUARRY_INTERNAL_SECRET` (97); `QUARRY_INTERNAL_HMAC_REQUIRED` (98); dispatcher workers 4 (main.go:197). Backoff and maxAttempts are constants, not env (dispatcher.go:25-33). Client timeout 30 s (dispatcher.go:41).
- Edge: `QUARRY_EDGE__CONTROL_BASE_URL` (`Q:deploy/compose/docker-compose.yml` [C] line 83) maps to config.rs:353 `default_control_url`.
- No tenant-facing webhook flag exists.

### 6. Surprises
- Card says the ladder is "5s/30s/5m/1h". The code starts at 1 s (dispatcher.go:25-31).
- Secret echo is confirmed (resources.go:338-339; store.go:379). Any HMAC-signed caller can read every webhook secret through `GET /v1/webhooks`.
- fanoutWebhooks runs for each newly persisted event in the events-ingest loop (resources.go:261-273; duplicates skipped by ErrConflict) and for each change record (change_webhook.go:164). It queues the full event JSON (resources.go:616-619, 643) to every active webhook, across orgs. The Event has no org (event.go:53-62). Registering any tenant webhook today would leak cross-tenant payloads.
- The card's Phase 1 item "emit run.completed at the end of CrawlJobWF" is already done (workflows.go:507-511, 607-609). The card's JSON correctly drops it.
- The Settings "Webhook delivery" panel is a placeholder with no backing API (WorkspaceSettingsPage.tsx:1933-1942).
- `Q:docs/openapi.yaml` [M] has no webhook paths.
- Application Plane follow-up automation: [UNVERIFIED]. The card says no directory scan found it; not re-checked.

---

## F-t2-040 Parallel batch, idempotent creation, wait-or-resume

### 1. Citation check (card and JSON evidence; 22 entries)
| Citation | Tag | Verdict | Finding |
|---|---|---|---|
| Q:crates/quarry-edge/src/routes.rs:131-134 (and 134) | [M] | OK | /v1/batch POST at 134 |
| routes.rs:670-726 | [M] | DRIFTED | BatchRequest 669-679 (urls, ingest, zdr only). batch_handoff 681-725 |
| routes.rs:192 and 232 | [M] | OK | Jobs list and events |
| routes.rs:563-583 (ZDR refusal) | [M] | OK | 563-588 |
| Q:crates/quarry-edge/src/handoff.rs:26-82 | [C] | OK | POST `/v1/jobs?org_id` with `{kind, params}`; no Idempotency-Key (59-69) |
| Q:crates/quarry-edge/src/resource_routes.rs:466-474 and 471-474 | [C] | OK | Random key per forwarded mutation (471-474). Not used by crawl or batch |
| Q:crates/quarry-edge/src/schedule_routes.rs:164-171 | [C] | OK | Inside forward_json (125-180); key at 169-171 |
| Q:services/quarry-orchestrator/internal/workflows/workflows.go:357-381 and 359-394 | [C] | OK | BatchJobWF 359-383; sequential loop 370-377 |
| workflows.go:396-420 | [C] | OK | CrawlJobWF prologue; installControlHandlers at 400 |
| Q:services/quarry-control/internal/resources/resources.go:395-450 | [C] | DRIFTED | createJob 396-458. Org-scoped lookup 424-429; race path 447-452; conflict 453 |
| Q:services/quarry-control/internal/resources/cycle23.go:420-437 and 436 | [C] | OK | IdempotencyKeyHandler 436-438: no-op, zero callers |
| Q:services/quarry-control/internal/store/pg/migrations/007_jobs_idempotency_key.sql:1-10 | [C] | OK | Unique index on `(idempotency_key) WHERE NOT NULL`; no org |
| Q:services/quarry-control/internal/store/store.go:145-150 | [C] | OK | FindByIdempotencyKey(orgID, key) interface |
| Q:crates/quarry-runtime/src/pipeline.rs:335-364 | [C] | OK | Global autoscale 340-343; per-host scheduler 344-348 |
| Q:crates/quarry-runtime/src/host_scheduler.rs:1-30 | [C] | OK | AIMD per host |
| GW:domains/knowledge/quarry.rs:305-345 | [C] | DRIFTED | start_crawl 306-376 |
| quarry.rs:420-445 | [C] | OK | normalize_batch_body 423-; cap 200 (424) |
| quarry.rs:386 and 423 | [C] | OK | normalize_crawl_body 386; normalize_batch_body 423 |

### 2. Affected files
- `Q:crates/quarry-edge/src/routes.rs` [M] 1564. BatchRequest 669-679 (add options, webhook, idempotency); batch_handoff 681-725 (forward 688-699; meter 705-717 runs before the ack check; match 718-724); crawl_handoff 590-667.
- `Q:crates/quarry-edge/src/handoff.rs` [C] 126. forward_to_orchestrator 26-; request build 57-69 (add `Idempotency-Key` header). Call sites routes.rs:626, 688.
- `Q:crates/quarry-edge/src/schedule_routes.rs` [C] 527 (forward_json key 169-171).
- `Q:crates/quarry-edge/src/main.rs` [M] 1569. HostScheduler installed at 1497-1498 (cited lines unchanged in the live diff).
- `Q:services/quarry-control/internal/resources/resources.go` [C] 650. createJob 396-458; createJobInput 460-; resolveJobPolicy 467-.
- `Q:services/quarry-control/internal/store/store.go` [C] 923. Job.IdempotencyKey 202-206; FindByIdempotencyKey 145-150.
- `Q:services/quarry-control/internal/store/pg/resources.go` [C] 1097. Jobs insert 70; idempotency lookup 93-94 (org-scoped); List 241-251.
- `Q:services/quarry-control/internal/resources/cycle23.go` [C] 438 (placeholder 436-438).
- `Q:services/quarry-orchestrator/internal/workflows/workflows.go` [C] 640. BatchJobInput 62-72 (no options); BatchJobWF 359-383; CrawlJobWF handlers at 400.
- `Q:services/quarry-orchestrator/internal/workflows/signals.go` [C] 146. installControlHandlers (87-136) must be installed in BatchJobWF.
- `Q:services/quarry-orchestrator/internal/jobs/dispatcher.go` [C] 551. batchInputFromJob 456-469 passes urls, ingest, user, org only.
- `Q:services/quarry-orchestrator/internal/activities/activities.go` [C] 476. RunPageInput 165-181 has no options.
- `Q:crates/quarry-runtime/src/pipeline.rs` [C] 1503; `Q:crates/quarry-runtime/src/host_scheduler.rs` [C] 577.
- `GW:domains/knowledge/quarry.rs` [C] 1158 (start_crawl 306-376; normalize_batch_body 423); `GW:domains/knowledge.rs` [C] 196; `GW:domains/ingestions/runs.rs` [C] 463 (create_batch 226-267).
- `FE:shared/api/knowledge-client.ts` [C] 933 (crawlSelectedPages 476-); `FE:features/dashboard/home/CrawlPagePicker.tsx` [C] 199 (MAX_SELECTION 8).
- `Q:docs/openapi.yaml` [M]: BatchRequest 4375-4385 says "URLs to scrape in parallel" (false).
- New migration: `Q:services/quarry-control/internal/store/pg/migrations/015_*.sql` [NEW] replaces idx_jobs_idempotency_key (007) with `(org_id, idempotency_key)`.

### 3. Reusable but unwired
- `installControlHandlers` (signals.go:87): a ready pattern for BatchJobWF. Not installed there.
- `HostScheduler` (live): applied per page in PageRunner (pipeline.rs:344-348). It bounds fan-out politeness when batch parallelises.
- Control createJob idempotency lookup (resources.go:424-429) and the org-scoped pg lookup (pg/resources.go:93-94): live, reusable.
- Gateway curated batch (quarry.rs:423-445): live, 200 cap and SSRF normalisation.
- `IdempotencyKeyHandler` (cycle23.go:436): zero callers. Placeholder only.
- Model Plane idempotency registry: [UNVERIFIED], not traced.

### 4. Tests
- Orchestrator: `workflows_test.go` [C] TestBatchJobWF_AggregatesSuccessAndFailure (173); TestCrawlJobWF_DropsDuplicateUrls (306).
- Control: `store_test.go` [C] TestMemoryDB_JobsFindByIdempotencyKey (122); `jobs_org_test.go` [C] (24, 64).
- Edge: no test for batch_handoff or crawl_handoff. Name grep of routes.rs test module finds none.
- Gateway: `quarry.rs` tests normalize_batch_body_dedups_drops_invalid_and_caps (998), normalize_batch_body_rejects_empty_after_filtering (1017); `GW:domains/ingestions/shared.rs` normalize_batch_urls_dedups_drops_invalid_and_caps (307).
- Frontend: CrawlPagePicker test file not found [UNVERIFIED]. `FE:features/knowledge/components/KnowledgePage.test.tsx` exists [C].
- Commands: `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./internal/workflows/ -run BatchJobWF -count=1`; `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-control" && go test ./internal/store/ -run Idempotency -count=1`; `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" normalize_batch`.
- CI: quarry-v2-ci.yml [C] (Go coverage); verevonv3-ci.yml [M] gateway cargo test (141).

### 5. Feature flags and env
- `DurableFrontier` (activities.go:67) does not gate batch. BatchJobWF has no durable variant, so batch always runs the sequential loop.
- Batch billing: `BATCH_URL` quantity = URL count (routes.rs:705-717).
- Gateway batch cap 200 (quarry.rs:424); UI cap 200 (CrawlPagePicker.tsx:8).
- HostScheduler is wired in production (main.rs:1497-1498) and disabled in tests (`scheduler: None`, routes.rs:1333).
- ZDR: durable jobs refused (routes.rs:563-588).

### 6. Surprises
- The handoff sends no Idempotency-Key (handoff.rs:59-69). The random keys in resource_routes.rs:471-474 and schedule_routes.rs:169-171 protect other mutations. The S quick win is a new header, not a swap.
- Control's unique index is on `idempotency_key` alone (007). Two orgs choosing the same key collide on insert. The race path then looks up by org, finds nothing, and returns 409 CodeConflict (resources.go:443-455) rather than replaying.
- batch_handoff and crawl_handoff meter usage before checking the handoff result (routes.rs:646-659 and 705-717, then `match ack` at 660 and 718). Failed handoffs still bill BATCH_URL or CRAWL_SEED.
- BatchRequest has no `deny_unknown_fields` (routes.rs:669-670). Clients sending `maxConcurrency`, `scrapeOptions` or `webhook` get no error.
- `Policy` on BatchJobInput is never read (workflows.go:71).
- `Q:docs/openapi.yaml` [M] says batch runs "in parallel" (4385). BatchJobWF is sequential (workflows.go:371-377).
- Per-host politeness already exists (pipeline.rs:344-348), so the card's "per-host politeness" is correct.

---

## F-t2-041 Sitemap, robots and index-assisted discovery, canonicalisation

### 1. Citation check (card and JSON evidence; 22 entries)
| Citation | Tag | Verdict | Finding |
|---|---|---|---|
| Q:crates/quarry-edge/src/map_routes.rs:87 | [C] | OK | merge_and_scope 87-109 |
| map_routes.rs:132-250 | [C] | OK | Map handler from 132; robots 170-185; sitemap 187-216 (nested cap `take(5)`, line 203); scope and robots filter 250-259 |
| Q:crates/quarry-transform/src/robots.rs:14 | [C] | OK | `pub sitemaps: Vec<String>` (14) |
| robots.rs:80-81 | [C] | OK | Sitemap directive pushed (80-81) |
| Q:crates/quarry-transform/src/sitemap.rs:1-40 | [C] | OK | Module doc and parse 25-33. No gzip support (grep for gzip, flate2 and GzDecoder in quarry-transform and map_routes.rs: none) |
| Q:crates/quarry-edge/src/routes.rs:533-575 | [M] | OK | CrawlRequest 533-553 has no sitemap option |
| routes.rs:776-830 (run_page output: final_url, links, fingerprint) | [M] | FALSE (final_url) | InternalRunPageResult 771-810 has links (776) and fingerprint (775) but NO final_url. from_output at 817-840 has none. `final_url` exists only in quarry-core `output.rs` [C] line 123 and in the public scrape response (routes.rs:500) |
| Q:crates/quarry-edge/src/queue_routes.rs:138-155 | [C] | OK | enqueue handler; internal_router 197-203 is cfg(feature postgres-queue) |
| Q:crates/quarry-runtime/src/postgres_queue.rs:243-260 | [C] | OK | `ON CONFLICT (org_id, request_id) DO NOTHING` at 248-256 |
| Q:crates/quarry-runtime/src/crawl_frontier.rs:1-60 | [C] | OK | Module doc and types (FrontierEntry 20, FrontierConfig 54) |
| crawl_frontier.rs:197-240 | [C] | OK | seed 197-209; seed_with_host_report 210-235; enqueue 238 |
| crawl_frontier.rs:400-407 | [C] | OK | normalize_url 400-407 |
| Q:crates/quarry-runtime/src/lib.rs:113 | [M] | OK | pub use crawl_frontier at 112-113. lib.rs is [M] (added `pub mod data_plane_http` at 42) |
| Q:services/quarry-orchestrator/internal/workflows/workflows.go:396-430 | [C] | OK | CrawlJobWF seeds 418-428 |
| workflows.go:513-535 | [C] | OK | Durable seed enqueue 524-535 |
| workflows.go:612-620 and 617-620 | [C] | OK | Queue name 612-615; request id sha256(run + url) 617-620 |
| Q:services/quarry-orchestrator/internal/activities/activities.go:55-67 | [C] | OK | DurableFrontier 55; New 59-69 |
| Q:docs/ROADMAP.md:35-41 | [C] | OK | Phase 5 bullets 35-40 |
| Q:docs/PLAN.md:232-246 | [C] | DRIFTED (stale) | Cites `crates/quarry-runtime/src/crawl.rs` (missing) and `/v1/internal/enqueue` (missing). Real route: `/v1/internal/queues/:queue_name/enqueue` (queue_routes.rs:200) |
| FE:features/dashboard/home/CrawlPagePicker.tsx:1-60 | [C] | OK | MAX_SELECTION 8; discovery to batch |
| FE:shared/api/knowledge-client.ts:455-486 | [C] | OK | discoverCrawlPages 457-474 (sends includeSubdomains, 468); crawlSelectedPages 476- |

### 2. Affected files
- `Q:crates/quarry-edge/src/map_routes.rs` [C] 459. Extract discovery into a shared module (host scope 78-83; merge_and_scope 87-109; sitemap 187-216; robots 170-185).
- `Q:crates/quarry-edge/src/routes.rs` [M] 1564. CrawlRequest 533-553 (add sitemap mode); crawl_handoff 590-667; InternalRunPageResult 771-810 (add final_url); internal_run_page 898.
- `Q:crates/quarry-edge/src/queue_routes.rs` [C] 208 (enqueue 138-155); `Q:crates/quarry-edge/src/handoff.rs` [C] 126.
- `Q:crates/quarry-transform/src/sitemap.rs` [C] 152 (add gzip and nested-index caps); `Q:crates/quarry-transform/src/robots.rs` [C] 262.
- `Q:crates/quarry-runtime/src/crawl_frontier.rs` [C] 651; `Q:crates/quarry-runtime/src/postgres_queue.rs` [C] 672 (enqueue 241-270); `Q:crates/quarry-runtime/src/pipeline.rs` [C] 1503 (run_inner 298-; links 507; body-too-large branch with final_url in event 423-428); `Q:crates/quarry-runtime/src/lib.rs` [M] 251.
- `Q:crates/quarry-core/src/output.rs` [C] (final_url at line 123).
- `Q:services/quarry-orchestrator/internal/workflows/workflows.go` [C] 640 (CrawlJobWF visited set 417 and dedupe 479-482; durable request id 617-620); `.../activities/activities.go` [C] 476 (FrontierEnqueue 101-110 posts to the edge).
- `Q:services/quarry-orchestrator/internal/workflows/workflows_test.go` [C] (tests run here).
- `GW:domains/knowledge/quarry.rs` [C] 1158 (normalize_map_body 467-; discover_crawl_pages 507-538; build_discovery 542-578); `GW:domains/knowledge.rs` [C] 196 (discover route 179-181).
- `FE:shared/api/knowledge-client.ts` [C] 933; `FE:features/dashboard/home/CrawlPagePicker.tsx` [C] 199.
- Docs: `Q:docs/PLAN.md` [C] 397; `Q:docs/OSS_PARITY_BACKLOG.md` [C] 128; `Q:docs/ROADMAP.md` [C] 83.
- No schema change required: reuse `(org_id, request_id)` uniqueness (postgres_queue.rs:248-256). Derive request_id from the canonical URL. A new `canonical_url` column would be a new migration (sequence 015 if needed). Frontier tables are not in control migrations [UNVERIFIED location].

### 3. Reusable but unwired
- `quarry_transform::sitemap::parse` and `RobotsTxt.sitemaps` (robots.rs:14). Production reader of `.sitemaps`: none. map_routes.rs uses only `/sitemap.xml` and the page links (189-216). `sitemaps` is read only in robots.rs tests (231-236).
- `CrawlFrontier::host_in_scope` and `enqueue` (crawl_frontier.rs:87, 238): no production caller (see F-t2-016).
- Postgres frontier `(org_id, request_id)` idempotency (postgres_queue.rs:248-256): live via FrontierEnqueue, behind the postgres-queue feature.
- `durableFrontierRequestID` (workflows.go:617-620): live but not canonical.
- `links::extract` callers: map_routes.rs:231 and pipeline.rs:507 (live).

### 4. Tests
- Edge: map_routes.rs tests same_host_in_scope (347), external_host_rejected (352), merge_dedups_and_scopes (364).
- Transform: sitemap.rs parses_sitemap_index (121), empty_xml_returns_empty_sitemap (133); robots.rs sitemaps_collected (231).
- Runtime: postgres_queue.rs enqueue_then_pop_roundtrips (459), reap_expired_returns_in_flight_to_queued (548). Both need Postgres and the postgres-queue feature.
- Gateway: quarry.rs normalize_map_body_defaults_and_passes_through_options (1023); build_discovery_reshapes_links_and_skips_urlless (1043) and build_discovery_reads_data_enveloped_links (1064).
- Commands: `cd "apps/Ingestion Plane/Quarry-v2" && cargo test -p quarry-transform sitemap robots && cargo test -p quarry-edge map_routes`; `cargo test -p quarry-runtime --features postgres-queue postgres_queue` (needs DATABASE_URL).
- CI: quarry-v2-ci.yml [C] runs `cargo test --workspace` (default features; postgres-queue tests skipped) and `cargo llvm-cov --workspace --all-features` (304).

### 5. Feature flags and env
- `postgres-queue` cargo feature: required by queue routes and the durable frontier. Deployed edge image enables it (`Q:deploy/compose/Dockerfile.edge` [C] line 102).
- `DurableFrontier` (activities.go:67).
- `/v1/map` request field `include_subdomains` (map_routes.rs:36), default false. Nested sitemap cap 5 (line 203). MAX_LIMIT 1000 (line 22). Gateway map cap 200 (quarry.rs:469).
- No sitemap option on CrawlRequest; no canonicalisation flag anywhere.

### 6. Surprises
- Post-redirect reconciliation needs a wire change. `final_url` is not in InternalRunPageResult (routes.rs:771-810) or the Go RunPageResult (activities.go:190-). The card's "run_page output (final_url...)" is FALSE for the wire.
- PLAN.md:233-234 cites a missing file and a missing route (see table).
- OSS_PARITY_BACKLOG.md:26 says `/map` has "no endpoint". Stale: /v1/map is routed (routes.rs:169) and implemented (map_routes.rs, 459 lines).
- /v1/map reads robots `Sitemap:` lines nowhere (robots.sitemaps is unused by map_routes.rs). The card's "ignores robots Sitemap lines" is correct.
- The crawl dedupe is an unbounded in-memory `visited` set of every link string (workflows.go:417, 479-482). That is memory and Temporal-history pressure on large sites (the card's risk).
- The UI discover step already sends includeSubdomains (knowledge-client.ts:468), but CrawlRequest has no such field (routes.rs:533-553). The UI scope option has no effect on crawl.
- `lib.rs` [M] diff adds `pub mod data_plane_http` (line 42) and reformats the rerank re-exports (around line 160). The crawl_frontier and robots_cache re-exports (112-113, 165) are unchanged.

<!-- COMPLETE -->
