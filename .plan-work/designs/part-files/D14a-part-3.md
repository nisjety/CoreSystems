
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
