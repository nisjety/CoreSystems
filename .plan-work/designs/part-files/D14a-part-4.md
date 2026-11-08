
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
