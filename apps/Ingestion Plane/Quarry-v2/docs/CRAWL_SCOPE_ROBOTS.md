# Crawl scope and robots.txt: the WS14 contract

Status: written 2026-10-23 by WS14.S0 (plan `docs/plans/quarry-browser-build-plan-2026-10-14.md`, slice WS14.S0). This document is the contract for the scoped, robots-compliant crawl and batch workstream (WS14). It records what the code does today, what each later slice will change, and the decisions that govern those changes. Where the text says "planned", no code in the tree does it yet.

## 1. Status today (2026-10-23)

| Behaviour | Today | Built by |
|---|---|---|
| Crawl follows links to other hosts | Yes. The in-workflow loop (`CrawlJobWF`, legacy branch) hands every link to `RunPage`. Pinned by `TestCrawlJobWF_CharacterizesExternalLinkFollowing_Legacy`. | WS14.S3 (scope, `crawl-scope-v1`) |
| Durable frontier loop | Enqueues every link once per page. No test ran it before WS14.S0 (`TestCrawlJobDurable_EnqueuesEveryLinkWithOneActivityEach`). | WS14.S0 test; S3 changes its enqueue to a batch |
| Include and exclude patterns | Not wired. The Rust crawl frontier has pattern fields with no production caller; the orchestrator input has no pattern field. | WS14.S2 (filter), S3 (wiring), S6 (request fields) |
| Sitemap discovery | Not wired into the crawl path. No sitemap route is served by quarry-edge; `/v1/map` reads `sitemap.xml` itself. | WS14.S6 (`sitemap: skip` only) |
| robots.txt in the crawl path | Not enforced. `/v1/map` applies a path-only matcher (token `QuarryBot`). `quarry-runtime/src/robots_cache.rs` has no production caller. The static driver advertises `Quarry/2.0 (+https://triodelab.com/quarry)`, the HTTP/3 driver `QuarryBot/1.0`. | WS14.S4 (observe first) |
| Cross-host and subdomain rules | None. | WS14.S2 and S3 |
| Batch URL validation | `batch_handoff` forwards `req.urls` without a cap or parse. Only the gateway validates (200 URLs). | WS14.S5 |
| ZDR on crawl and batch | Refused (`refuse_durable_job_under_zdr`, `quarry-edge/src/routes.rs`). | Kept (section 8) |

Stale claims removed by WS14.S0: `docs/GAP.md` rows for robots, sitemap, include and exclude, external links and subdomains were marked Closed. They are now Open, and the code citations were checked against the tree on 2026-10-23.

## 2. Decisions in force

These are the approved defaults of the build plan (its open questions were approved at the recommended defaults on 2026-10-23). Items marked "gate" need a further decision before the step that uses them.

- **Default scope (X-10, approved default).** Same host, with `www.` equivalence in both directions. Whole host (`crawl_entire_domain` is true). Page cap 1000. Existing crawls lose off-site pages when scope is enforced, so scope goes out in `observe` first.
- **Page and depth bounds (WS6.A).** Absent or `0` takes the default: `QUARRY_CRAWL_DEFAULT_MAX_PAGES` (1000) and `QUARRY_CRAWL_DEFAULT_MAX_DEPTH` (5). Above the hard ceiling the value is clamped to `QUARRY_CRAWL_HARD_MAX_PAGES` (5000; the owner may still rule 10,000) and `QUARRY_CRAWL_HARD_MAX_DEPTH` (10). The plan's `QUARRY_EDGE__CRAWL_DEFAULT_MAX_PAGES` and `QUARRY_EDGE__CRAWL_MAX_PAGES_CAP` names are dropped.
- **`max_pages: 0` (X-10).** Treated as absent in `lenient` mode. In `strict` mode it is a `400 invalid_option` (WS14.S6 and S8). The gateway keeps its existing `400` for `maxPages` of `0`. Test name for the pair: `zero_pages_is_default_in_lenient_and_400_in_strict`.
- **Robots when unreachable (X-10, ADR-5 / OQ-5).** Recommended default: `disallow` (RFC 9309 treats a 5xx or timeout on `/robots.txt` as "disallow all"). The enforce flip waits for X-10 and ADR-5 and for a soak. Flaky robots endpoints will stop crawls; that is the cost, and it is accepted only after the soak.
- **Ignore-robots override (X-10, ADR-1 and OQ-11).** Only an audited Control Plane entitlement (`web.robots_override`, ceiling `robots.max_mode` with `strict < respect < ignore`, default `respect`). Grantor: platform operators (default), not org admins, unless OQ-11 is ruled otherwise. The audit event is written by quarry-control before the job is acknowledged, and the job fails closed if the audit write fails. WS14.S7 is gated on this and on a Control Plane credential PR. It is not built.
- **ZDR (X-10, ADR-4).** Keep the refusal. Lifting it would touch five retention surfaces (control job `params`, Temporal history, the control event log, the Postgres frontier queue, checkpoints). The refusal text and status are unchanged (R-25).
- **Robots token.** The robots group is matched with the token `QuarryBot` (see section 4). The user agent sent on the wire is the driver's own string; the two strings are not yet one identity (WS14.S4 and ADR-5 decide this).

## 3. Scope grammar and caps (planned, WS14.S2)

- Patterns: a glob by default. Prefixes `re:` (path regex), `url:` (full-URL glob) and `re_url:` (full-URL regex) select the other forms.
- Regex limits: `size_limit` 256 KiB and `dfa_size_limit` 1 MiB. An over-size regex is a field error, not a panic.
- Caps: at most 100 patterns (`MAX_PATTERNS`), 512 characters each (`MAX_PATTERN_CHARS`), 8192 characters in total (`MAX_PATTERN_TOTAL_CHARS`). A page evaluates at most 5000 links (`MAX_LINKS_EVALUATED`).
- Check order for one link: parse, scheme (`http` and `https` only), fragment-only, depth, host scope, section scope, exclude, include, extension denylist, robots (cached rules only). The first failing check gives the denial code.
- Dedupe is on the normalised URL (lowercase host, trailing dot removed, IDN compared in ASCII form, userinfo removed).
- `exclude` beats `include`.
- The Model may send the scope fields that the registry-governed action schema allows (WS14.S6). `allow_external_links` is UI-only and never Model-visible.

## 4. Robots semantics (planned, WS14.S4)

- Fetch `/robots.txt` for each origin over `https`, then `http` only if `https` fails. The fetch goes through the same SSRF guard and DNS pinning as page fetches. The current `ReqwestRobotsFetcher` is not used as written: it has no SSRF guard and fails open on 5xx and transport errors.
- Status codes: 2xx parses the body. 404, 403 and 410 mean no rules (allow all). 5xx and timeouts are "unreachable" and follow `QUARRY_EDGE__ROBOTS_UNREACHABLE` (`disallow` by default).
- Group selection follows RFC 9309: the most specific user-agent group, then `*`. `Allow` and `Disallow` use longest-match.
- The parser strips a leading UTF-8 BOM, keeps a user-agent list open across blank lines and `Crawl-delay` lines, and ends it at an empty `Allow:` like any rule (RFC 9309 sections 2.2 and 2.2.4).
- `Crawl-delay` raises the per-host interval up to `QUARRY_CRAWL_DELAY_MAX_S` (10 s by default, ceiling 60 s).
- Cache: `QUARRY_ROBOTS_CACHE_MAX_ENTRIES` (10,000) and `QUARRY_ROBOTS_FETCH_TIMEOUT_S` (8).
- Modes: `QUARRY_EDGE__CRAWL_ROBOTS` is `observe` first. In `observe`, denials are counted and reported and no page is dropped.

## 5. Denial vocabulary

The codes below are the ones in `crates/quarry-core/src/crawl_denial.rs`. Their wire form is the snake_case code.

| Code | Meaning | Source |
|---|---|---|
| `out_of_scope` | Section link outside the scope (old name `section_link`) | scope |
| `robots_disallowed` | robots.txt disallows the path (old name `robots_txt`) | robots |
| `depth_exceeded` | Link deeper than the max depth (old name `depth_limit`) | scope |
| `max_pages_reached` | Page cap reached | scope |
| `duplicate` | URL already visited (frontier dedupe) | scope |
| `external_host_disabled` | Other host, with external links off (old name `external_link`) | scope |
| `security_rejected` | Rejected by the SSRF guard on discovery | security |
| `include_pattern_miss` | No include pattern matched | scope |
| `exclude_pattern_hit` | An exclude pattern matched (old name `exclude_pattern`) | scope |
| `content_type_rejected` | Content type not allowed (old name `file_type`) | fetch |
| `not_crawlable` | Unparseable URL or non-web scheme (old names `url_parse_error`, `non_web_protocol`) | scope |

Planned additions: `robots_unreachable` (WS14.S2, `RobotsUnreachable { host }`). At fetch time a security block keeps the existing `SECURITY_BLOCKED` (403) error code with `details.denial`; no new `ErrorCode` is added (R-10).

Old names in `docs/CONTRACTS.md` (before this document) are superseded by the table above. `CONTRACTS.md` marks the old list as planned.

## 6. Events and payloads (planned, WS14.S3)

- No new event types. The list is frozen (`docs/CONTRACTS.md`, the event list).
- `run_started.payload` gains `scope`, `scope_mode` and `robots`.
- `page_blocked.payload` is `{url, category: "blocklist", code, message, remedy, source: "fetch" | "discovery", user_agent?, path?}`.
- Terminal payloads (`run_completed`, `run_cancelled`) gain `pages_blocked`, `pages_blocked_robots` (the part of it the robots gate refused, WS14.S4), `links_denied`, `hosts_added` and `denials: {mode, counts, samples}`. Counts are per code. Samples are capped at 3 per code and 20 per page.
- Field names are stable: they enter the run-trace vocabulary through the default-deny allowlist of WS10 (R-11).
- Logs never carry a URL, query, title or raw error text (log-field lint). Counts and classes only.

## 7. Flags and rollback

| Variable | Values | Default when built | Notes |
|---|---|---|---|
| `QUARRY_EDGE__CRAWL_SCOPE_MODE` | `off`, `observe`, `enforce` | `observe` (WS14.S3) | `off` stamps `scope.legacy=true` and sends no context. `enforce` is set by WS14.S8 after the soak. |
| `QUARRY_EDGE__CRAWL_ROBOTS` | `observe`, `enforce` | `observe` (WS14.S4) | Enforce needs X-10 and ADR-5. |
| `QUARRY_EDGE__ROBOTS_UNREACHABLE` | `disallow`, `allow` | `disallow` | Recommended default. |
| `QUARRY_EDGE__CRAWL_REQUEST_VALIDATION` | `lenient`, `strict` | `lenient` (WS14.G, I) | `strict` is the 400 behaviour for `max_pages: 0`. |
| `QUARRY_EDGE__CRAWL_BATCH_MAX_URLS` | integer | `500` at the edge; the gateway keeps 200 | WS14.S5. |
| `QUARRY_EDGE__BATCH_PARALLEL` | `1`, `true`, `yes`, `on` | off | Lets a batch body's `max_parallel` take effect (W3-E3.7.2w). Read on each batch request from the process environment, so a change needs an edge restart; off, the key is ignored. The orchestrator also needs `QUARRY_BATCH_PARALLEL` on. |
| `QUARRY_EDGE__BATCH_MAX_PARALLEL` | integer, 1 to 16 | `4` | The edge ceiling on one batch's parallel pages. Checked at boot (an unparsable value stops the edge) and read once per process, so a change needs an edge restart. No organization term sets or bounds it. |

Rollback: set `QUARRY_EDGE__CRAWL_SCOPE_MODE=off` and restart the edge, or redeploy the previous orchestrator (the legacy branch stays in place behind `GetVersion`). Version skew fails closed: an orchestrator ahead of the edge ends the run with `run_failed` and code `edge_scope_unsupported`, and fetches no child page.

Versioning: the orchestrator change sits behind `workflow.GetVersion(ctx, "crawl-scope-v1", ...)`. A history recorded before it keeps the legacy behaviour on replay. Each `GetVersion` id is unique and stays in introduction order (plan H19).

## 8. ZDR

Crawl and batch refuse ZDR. The refusal (`refuse_durable_job_under_zdr`, `quarry-edge/src/routes.rs`) stays. The edge's internal run-page call already accepts `zdr`; the orchestrator does not carry it. Lifting the refusal is a redesign across five retention surfaces and is not in WS14.

## 9. The Model boundary

- The Model path reaches crawl only through the user's token via the gateway. Service principals cannot call `/v1/crawl`, `/v1/batch` or `/v1/map` (`quarry-edge/src/auth.rs`).
- The Model may request the registry-governed subset of scope fields (WS14.S6).
- `ignore_robots_txt` is refused for every service principal at the edge (second line), and is policy-gated (403 `policy_denied`) for users. The Model never sees the ignore-robots entitlement (WS14.S7).
- The override key stays refused on the Model path until WS14.S7 is built and approved.

## 10. Open items (not decided here)

| Item | Owner | Gate for |
|---|---|---|
| Hard ceiling 5000 pages, or 10,000 | Owner | WS6.A and WS14.S6 limits |
| Robots unreachable: disallow or allow | User (ADR-5 / OQ-5) | WS14.S4 enforce, WS14.S8 flip |
| Ignore-robots grantor (platform operators or org admins) | User (OQ-11) | WS14.S7 |
| Default scope change: soak length and the observe report | Ingestion owner | WS14.S8 |
| Robots user-agent identity (`QuarryBot` vs the driver strings) | Ingestion owner with ADR-5 | WS14.S4 |
| CRAWL_SCOPE_ROBOTS.md review | Ingestion owner | Merge of WS14.S0 (definition of done) |

## 11. Tests named by this contract

- Go (`services/quarry-orchestrator/internal/workflows`): `TestCrawlJobWF_CharacterizesExternalLinkFollowing_Legacy`, `TestCrawlJobDurable_EnqueuesEveryLinkWithOneActivityEach` (WS14.S0). Later: `TestCrawlJobWF_SendsCrawlContextOnEveryRunPage`, `TestCrawlJobWF_RefusesChildrenWhenEdgeDoesNotApplyScope` (S3).
- Rust: `crawl_filter` unit tests and `crawl_denial` unique-code tests (S2).
- Go job CI: the `go-test` job in `.github/workflows/quarry-v2-ci.yml` runs `go test ./...` for `services/quarry-control`, `services/quarry-orchestrator` and `pkg/quarrycontracts` and blocks the merge (WS14.S0b). Until this job is green on the branch, Go tests in these modules gate nothing except through the report-only `go-coverage` job.

## 12. Request contract and shared caps (WS14.S6, built dark, unit W2-U65)

The `/v1/crawl` body is read as a JSON value and checked field by field (`quarry-edge/src/crawl_request.rs`).

Flag: `QUARRY_EDGE__CRAWL_REQUEST_VALIDATION`: `lenient` (default; unknown keys are warnings in `effective.request_warnings`) or `strict` (unknown keys and `max_pages: 0` are 400). An unknown value is logged and treated as `lenient`.

Accepted options and their spellings (canonical first):

| Option | Spellings | Type and rule |
|---|---|---|
| `url` | `url` | required, non-empty string |
| `max_pages` | `max_pages`, `maxPages`, `limit` | whole number; 0 is the default (lenient) or 400 `invalid_option` (strict) |
| `max_depth` | `max_depth`, `maxDepth`, `max_discovery_depth` | whole number; 0 or absent is the default; hard ceiling 10 |
| `ingest`, `zdr` | unchanged | boolean |
| `allow_subdomains` | `allow_subdomains`, `allowSubdomains` | boolean, default false |
| `allow_external_links` | `allow_external_links`, `allowExternalLinks` | boolean, default false; UI only (the gateway drops it on the Model path) |
| `crawl_entire_domain` | `crawl_entire_domain`, `crawlEntireDomain` | boolean, default true |
| `include_paths` | `include_paths`, `includePaths` | array of strings (D3 grammar) |
| `exclude_paths` | `exclude_paths`, `excludePaths` | array of strings (D3 grammar) |
| `deny_extensions` | `deny_extensions`, `denyExtensions` | array of alphanumeric extensions, at most 16 characters each |
| `sitemap` | `sitemap` | `skip` (default); `include` and `only` with `QUARRY_EDGE__CRAWL_SITEMAP` on (INNH-6a, needs the scope switch on); otherwise 400 `unsupported_option` (F-t2-041) |
| `ignore_query_parameters` | both spellings | any value 400 `unsupported_option` (F-t2-041) |
| `scrape_options` | both spellings | with `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS` on (INNH-6b): the per-page extraction subset of `/v1/scrape`; otherwise any value 400 `unsupported_option` until WS5 slice 4 |
| `ignore_robots_txt` | `ignore_robots_txt`, `ignoreRobotsTxt`, `ignoreRobots` | `true` is 403 `policy_denied` (WS14.S7 owns the entitlement); `false` accepted |

Refused in both modes (400 `reserved_field`): `headers`, `proxy`, `proxy_url`, `skip_tls_verification`, `stealth`, `mobile`, `delay`, `max_concurrency` (and their camel forms). `webhook` is refused the same way while `QUARRY_EDGE__JOB_WEBHOOK` is off; on, it is the per-job webhook (CONTRACTS, "Batch request options"). Two spellings of one option: 400 `invalid_option`.

Errors are the canonical envelope: 400 `BAD_REQUEST` with `details.fields[] {path, code, message}`, or 403 `FORBIDDEN` with `details.reason = policy_denied`. The `path` is the spelling the caller sent. A message never echoes a caller's value.

Ack: `HandoffAck.effective` carries `scope`, `scope_mode`, `robots`, `max_pages`, `max_depth`, `legacy`, `warnings` (scope notes such as `patterns_reported_not_enforced`, `scope_not_applied_legacy`, `same_directory_only`) and `request_warnings` (`{code, field}` for ignored keys).

### Shared caps (one table, two trees)

Gateway `domains/knowledge/quarry.rs` (`CRAWL_MAX_*`) and edge `quarry-core/src/crawl_scope.rs` enforce the same numbers:

| Cap | Value | Constant (edge) | Constant (gateway) |
|---|---|---|---|
| Patterns across include and exclude | 100 | `MAX_PATTERNS` | `CRAWL_MAX_PATTERNS` |
| Characters per pattern | 512 | `MAX_PATTERN_CHARS` | `CRAWL_MAX_PATTERN_CHARS` |
| Characters across all patterns | 8192 | `MAX_PATTERN_TOTAL_CHARS` | `CRAWL_MAX_PATTERN_TOTAL_CHARS` |
| Extensions in `deny_extensions` | 100 | `MAX_EXTENSIONS` | not forwarded by the UI |
| Max depth | 10 | edge limits (WS6.A hard ceiling) | `CRAWL_MAX_DEPTH` |
| Max pages | 5000 | edge limits (WS6.A hard ceiling) | clamped in `normalize_crawl_body_for` |

Test pins: edge `crawl_request` boundary tests (100/101, 512/513, 8192/8193); gateway `pattern_caps_match_the_edge_boundaries`.

### Model boundary (WS14.S6)

The Model may propose `knowledge.crawl_site` with `url`, `maxPages`, `maxDepth`, `includePaths`, `excludePaths` and `allowSubdomains` only. The gateway builds that body with `Surface::Model`: `allow_external_links` is dropped, and no key containing `robot` is forwarded.

## 13. Absent scope: what a crawl without a scope does (D2-073 default)

A crawl job can reach the orchestrator with no `scope` object. That happens to a job row accepted before scope existed, and to a job from an edge older than the orchestrator during a rolling deploy. The default (W2-U64, D2-073) depends on the job's `scope_mode`:

| The job carries | `scope_mode` | What the crawl does |
|---|---|---|
| no `scope` object | `observe`, empty or unknown | Runs as a legacy-unscoped crawl. Nothing is refused, and no `crawl` context is sent. `scopeFromParams` returns a legacy scope and `newCrawlScopeRun` returns nil. |
| no `scope` object | `enforce` | Refused. `crawlInputFromJob` (`jobs/dispatcher.go`) returns an error, so the dispatcher does not start the workflow. |
| a `scope`, but the edge did not report `scope_applied` | `observe` | Each such page counts as legacy-unscoped (version skew). Tolerated only on executions that carry the `crawl-scope-applied-tolerant-v1` marker. An older execution keeps the strict behaviour and ends the crawl. |
| a `scope`, but the edge did not report `scope_applied` | `enforce` | The crawl ends with `edge_scope_unsupported`, and no child of that page is enqueued. |

A crawl started while `QUARRY_EDGE__CRAWL_SCOPE_MODE` is `off` is stamped with `scope.legacy` set to true. It runs as a legacy crawl whatever `scope_mode` says.

The edge keeps the same table as `scope_skew` (`crates/quarry-edge/src/crawl_request.rs`), and its unit tests pin the cases. Nothing calls `scope_skew` on the request path yet: the orchestrator makes the decision in production.

Known gap, not changed by this section: a scheduled crawl (`startScheduledChild` in `services/quarry-orchestrator/internal/workflows/scheduled.go`) builds its child input with `CrawlInputFromParams`, which does not run the dispatcher's refusal. A scheduled crawl that names `scope_mode: enforce` and carries no scope therefore runs legacy-unscoped today. It is open as depth row W23-b-U64-1 (next round: move the check into `CrawlInputFromParams` in `workflows/inputs.go`).
