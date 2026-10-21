# Quarry V2 — Progress

Living doc. Source of truth for current build state.

**Last updated:** 2026-05-08 (Cycle 14: structured-extract + max_cost + crawl frontier + robots/sitemap + S3 profiles + Firecrawl + audio + eval +4 + field traces)

---

## Overall status

| Phase | Title                                  | Status         | Notes                                    |
|-------|----------------------------------------|----------------|------------------------------------------|
| 0     | Freeze contracts                       | ✅ Done         | Rust + Go + docs aligned                 |
| 1     | Go control plane to parity             | ✅ Done         | CRUD + events + webhook dispatcher + blocklist + presets |
| 2     | Rust runtime under current API         | 🟡 In progress  | Static + chromiumoxide (incl. action primitives) + browserless REST + readability + urlsig heuristics; JA3 deferred |
| 3     | Fast-path cutover                      | 🟡 In progress  | SSE + Redis cache + canary shipped; donor scrape removal pending |
| 4     | Browser lease model                    | 🟡 In progress  | LeasePool TTL+affinity+cap+metrics; profiles/snapshots/restore endpoints + capture/restore deferred |
| 5     | Crawl workers to Rust                  | 🟡 Partial      | Crawl BFS lives in Go orchestrator (with dedupe + checkpoints + tests); Rust frontier deferred |
| 6     | Normalized output + change tracking    | ✅ Done         | Envelope + fingerprint + paragraph semantic diff (`SemanticDiff::is_real_delta`) + text_fingerprint in change events |
| 7     | Presets + determinism                  | ✅ Done         | Preset catalog (`fast`/`polite`/`stealth`/`deterministic`) + `/v1/presets` + strict-mode roundtrip determinism test |
| 8     | Benchmark + retire                     | 🟡 In progress  | Eval harness foundation shipped (2 fixtures + scoreboard); canary controls done; donor removal pending |
| 9     | Product parity hardening               | ⬜ New          | Firecrawl/V1 surface parity after benchmark truth |

Legend: ✅ done · 🟡 in progress · ⬜ not started · 🔴 blocked

---

## Deployables — current state

| Deployable              | Lang | Build  | Tests | Feature status                              |
|-------------------------|------|--------|-------|---------------------------------------------|
| `quarry-edge-rs`        | Rust | ✅     | ✅     | `/health`, `/ready`, `/v1/scrape` (real, +ingest/org_id), `/v1/crawl` + `/v1/batch` (handoff), `/v1/internal/run_page` (+ingest/org_id), `/v1/profiles` CRUD (save/load/delete/list), `/v1/audio` (Unsupported 501), Firecrawl-compat response adapter; OpenAPI 3.1 spec |
| `quarry-runtime` (lib)  | Rust | ✅     | ✅     | static driver, pipeline (+ Data Plane ingest wiring + SourceTrace), in-mem artifact store, retry policy, event sink, AgentLoop (max_steps + max_runtime + max_cost_usd + allowed_domains enforcement), Planner trait + MockPlanner, ModelPlaneClient + ModelPlanePlanner (real LLM-driven planner over Model Plane gateway HTTP), StructuredExtractClient (forwards to Model Plane `/v1/structured/extract` with cost ceiling enforcement), EventBus abstraction + InProcessEventBus, GrantValidator trait + HttpGrantValidator + NoopGrantValidator, CrawlFrontier (BFS + dedup + scope/depth/include/exclude with typed denial reasons), S3ProfileStore |
| `quarry-evals`          | Rust | ✅     | —     | eval runner: 10 HTML fixtures (article_basic, article_js_heavy, blocked_page, minimal_content, table_heavy, multi_section, cache_warm, js_blocked, change_precision, profile_restore), expectations manifest, quality scoreboard |
| `quarry-control-go`     | Go   | ✅     | ✅     | CRUD + events on mem+Postgres; pgx pool + embed migrations + cursor pagination + `/v1/jobs/{id}/history`; webhook dispatcher (HMAC-SHA256, exp backoff, DLQ) |
| `quarry-orchestrator-go`| Go   | ✅     | ✅     | Temporal worker, ScrapeJobWF + BatchJobWF + CrawlJobWF (BFS w/ dedupe + 50-page checkpoints), 5 testsuite workflow tests |

---

## Workstream detail

### Research update — Quarry V1 + Firecrawl + ecosystem

**Local Quarry V1 finding:** V1 remains broader as a product donor. It has richer schema/prompt extraction, Rod sessions, sitemap/robots-aware crawl behavior, search/map/extract/interact endpoints, and more complete Firecrawl-like API semantics. Its main weakness is architectural: Go owns too much of the hot path and many scrape behaviors are intertwined with API/middleware concerns.

**Local Firecrawl finding:** Firecrawl is ahead on product breadth. Its useful donor pieces are:

- v2 request/format/action schema in `apps/api/src/controllers/v2/types.ts`;
- engine waterfall in `apps/api/src/scraper/scrapeURL/engines/index.ts`;
- transform pipeline in `apps/api/src/scraper/scrapeURL/transformers/index.ts`;
- crawl filtering and operator-readable denial reasons in `apps/api/src/scraper/WebScraper/crawler.ts`;
- Playwright service hardening in `apps/playwright-service-ts/api.ts`;
- Go HTML-to-Markdown service as a benchmark target.

Firecrawl self-hosting explicitly lacks the full managed Fire Engine advantage. Quarry V2's opportunity is to make advanced drivers and durable recovery first-party/self-owned rather than hidden behind a hosted service.

**External ecosystem finding:**

- `rquest` upstream, published as `wreq` on crates.io, is the default Rust TLS impersonation transport because it targets TLS/JA3/JA4 and HTTP/2 browser emulation with BoringSSL. `wreq-util` is deferred until explicit license review.
- `impit` is a useful secondary experiment, but it currently requires patched dependencies and unstable flags.
- `curl_cffi` belongs in Python lab as a benchmark/control, not production hot path.
- Browserless persistent sessions map directly to Quarry profiles/snapshots: store connect/reconnect/stop URLs as secrets, capture browser state, clean up explicitly.
- Crawlee session pools and Scrapy AutoThrottle provide the right behavior model for session/proxy health and adaptive per-host delays.
- Mozilla Readability and Trafilatura are extraction quality baselines.

### Phase 0 — Contracts ✅

- [x] `docs/CONTRACTS.md` written
- [x] Rust `quarry-core` crate (ids, envelope, event, policy, output, artifact, cache, lease, error)
- [x] Go `pkg/quarrycontracts` mirror (ids, envelope, event, policy, output)
- [x] ID prefix + roundtrip tests (both languages)
- [x] Envelope serde tests

### Phase 1 — Go control plane

**Done:**
- [x] `DB` interface + in-memory backend
- [x] Resources mounted: jobs, stores, snapshots, artifacts, profiles, schedules
- [x] Event log append + list (`/v1/runs/{id}/events` GET + POST)
- [x] Request ID middleware, logger, panic recover
- [x] Unit tests for store (CRUD, conflict, event filter)
- [x] httptest integration for resources
- [x] Envelope writer + error envelope helper

**Todo:**
- [x] Postgres backend via pgx ✅ 2026-04-22
- [x] Migrations (embedded SQL + `schema_migrations` tracker) ✅
- [x] Cursor pagination (opaque base64 cursor over `(created_at, id)`) ✅
- [x] `/v1/jobs/{id}/history` merged view ✅
- [x] Schedule cron parsing + enable/disable ✅ 2026-04-22
- [x] Webhook dispatcher (queue + HMAC + retry + DLQ) ✅ 2026-04-25
- [x] Blocklist persistence endpoint ✅ 2026-05-02

### Phase 2 — Rust runtime

**Done:**
- [x] Crates wired: core, security, transform, browser (stubs), runtime, edge
- [x] Static HTTP driver (reqwest + rustls)
- [x] Driver selection function (hints + JS-required hostlist)
- [x] PageRunner: preflight → fetch → transform → diff → artifacts → return
- [x] Default security engine: scheme + SSRF heur + blocklist + allowlist + discovered-URL scope
- [x] Transform pipeline: markdown (html2md), links, metadata, fingerprint (blake3), text-fingerprint, diff, chunk
- [x] Artifact store trait + in-memory impl
- [x] Event sink (mpsc) for page-level events
- [x] Retry backoff calculator
- [x] Axum router with timeout + trace layers

**Todo:**
- [ ] Replace TLS-profile driver with real impersonating transport — use rquest upstream (`wreq`) first; keep `wreq-util` behind license review, `impit` as a fallback experiment, and `curl_cffi` in lab baseline only
- [ ] Add typed `DriverPlan` and Firecrawl-style engine fallback metadata
- [ ] Wire readability-first markdown helper into production `PageRunner` path
- [ ] Add `images`, `attributes`, and Firecrawl-compatible requested format parsing
- [x] `chromiumoxide` browser driver ✅ 2026-04-27
- [x] Browserless remote driver ✅ 2026-04-29
- [x] Lease pool (acquire/reconnect/release with affinity) ✅ 2026-04-28
- [x] Action runtime (wait/click/scroll/screenshot/pdf/evaluate) ✅ 2026-04-29 — `BrowserDriver` trait extended with default `Unsupported` impls; chromiumoxide overrides for `wait_for`, `click`, `type_text`, `scroll`, `press`, `evaluate`; `ActionRuntime` wired to the new methods
- [x] S3 artifact backend ✅ shipped earlier; quarry-edge selects via `cfg.artifact_backend == "s3"`
- [x] DNS resolution guard at fetch time ✅ `quarry-runtime/src/dns_guard.rs` + wired in `pipeline.rs:88`
- [x] Readability-first markdown converter ✅ 2026-04-29 — `quarry_transform::readability` (article/main/role=main candidate scoring with link-density penalty + script/style/noscript/iframe/svg stripping); `html_to_readable_markdown` helper
- [x] Full heuristic port from donor `internal/security/heur/` ✅ 2026-04-29 — `quarry_security::urlsig` ports the URL signature analyzer (scheme, suspicious TLD, IP literal, homograph, typosquat, path/query patterns, length/character composition, phishing keywords) with confidence-weighted suspicion score
- [x] Runtime → control event publisher (HTTP POST loop) ✅ 2026-04-23

### Phase 3 — Fast-path cutover

**Done:**
- [x] SSE streaming for `/v1/scrape?stream=true` ✅ 2026-04-24
- [x] Redis-backed edge cache with CachePolicy enforcement ✅ 2026-04-26

**Todo:**
- [x] Canary split on edge feature flag ✅ 2026-05-02
- [ ] Remove donor scrape handler

### Phase 4 — Browser lease model

**Todo:**
- [x] `ProfileStore` trait + `InMemoryProfileStore` + `SessionSnapshot` extended (viewport, locale, timezone) ✅ 2026-05-07
- [x] `/v1/profiles` REST endpoints — POST/GET (single+list)/DELETE wired to `ProfileStore` ✅ 2026-05-07
- [x] Capture/restore round-trip integration test — 5 tests covering save/load preservation across all snapshot fields, list, delete, 404, custom-id round-trip ✅ 2026-05-07
- [ ] S3 session snapshot storage
- [ ] Persistent Browserless/CDP session driver: `connect`/`browserQL`/`stop`/reconnect URL stored as secret metadata
- [x] Rust `LeasePool` fleshed out (TTL + eviction + affinity) ✅ 2026-04-28
- [x] Sticky proxy routing ✅ 2026-04-28
- [x] Lease pool metrics snapshot ✅ 2026-05-02

### Phase 5 — Crawl workers to Rust

**Todo:**
- [ ] Rust-native frontier queue
- [ ] Checkpoint signal protocol (Rust ↔ Temporal workflow)
- [ ] Pause/resume/cancel signal routing
- [ ] Crash recovery E2E test
- [ ] Port V1/Firecrawl crawl behavior: sitemap, robots, include/exclude paths, depth/discovery limits, external/subdomain/backward controls
- [ ] Add typed crawl denial reasons with operator-readable messages
- [ ] Add adaptive per-host throttle and session/proxy health (`good`, `bad`, `retired`)

### Phase 6 — Output + change tracking

**Done:**
- [x] `NormalizedOutput` envelope (schema + serde)
- [x] blake3 fingerprint
- [x] Text-normalized fingerprint (ws/case)
- [x] Change status enum (New/Changed/Unchanged)
- [x] Paragraph chunker

**Todo:**
- [x] Semantic-ish diff (paragraph-level add/remove/change) ✅ 2026-04-29 — `quarry_transform::diff::diff_paragraphs` returns `SemanticDiff { added, removed, moved, unchanged }` with `is_real_delta()` helper; pure reorderings and whitespace-only changes are not real deltas
- [x] `meta.json` sidecar artifact ✅ 2026-05-03
- [x] `change.detected` event payload now carries `text_fingerprint` (whitespace/case-normalized) so consumers can suppress boilerplate-only churn at the subscription layer ✅ 2026-04-29

### Phase 7 — Presets + determinism

**Todo:**
- [x] Preset catalog + `/v1/presets` endpoint ✅ 2026-04-29 — `services/quarry-control/internal/resources/presets.go` ships 4 presets (`fast`, `polite`, `stealth`, `deterministic`); `POST /v1/jobs` accepts `preset: "<name>"` to materialize the policy; unknown preset names → 400
- [x] Determinism mode enforcement in runtime ✅ 2026-05-03
- [x] Strict-mode roundtrip determinism test ✅ 2026-04-29 — `tests/transform.rs::strict_determinism_roundtrip` runs the full HTML→readable→markdown→links→metadata→fingerprint pipeline twice + verifies byte equality, plus a third invocation guard against state-baked-across-calls bugs

### Phase 8 — Benchmark + retire

**Done:**
- [x] Eval harness foundation in `lab/evals/` ✅ 2026-05-07 — 6 HTML fixtures (article_basic, article_js_heavy, blocked_page, minimal_content, table_heavy, multi_section), expectations.json manifest, Rust eval runner binary with quality checks (title, lang, content presence, boilerplate absence, link count/content, fingerprint stability), JSON scoreboard generation; 6/6 passing
- [x] OpenAPI 3.1 spec ✅ 2026-05-07 — `docs/openapi.yaml` covering all 7 endpoints with full request/response component schemas matching Rust types

**Todo:**
- [ ] `docs/SCOREBOARD.md` auto-gen
- [ ] Canary rollout controls
- [ ] Donor execution code deletion
- [ ] Compare against Quarry V1 local and Firecrawl local/self-hosted; optionally Firecrawl cloud if API key available
- [ ] Extraction quality bakeoff: Quarry readability/html2md vs Firecrawl Go converter vs Mozilla Readability vs Trafilatura

### Phase 9 — Product parity hardening

**Todo:**
- [ ] Firecrawl-compatible response adapter and SDK ergonomics.
- [ ] Crawl status/errors/webhook parity.
- [ ] Browser interact/session API parity where it fits Quarry lease model.
- [ ] Feature-gated `branding`, `audio`, `summary`, `query`, and `json` transforms over captured artifacts.
- [ ] Publish parity matrix: V1, Firecrawl local, Firecrawl cloud, Quarry V2.

---

## Tests snapshot

| Suite                                | Count | Status |
|--------------------------------------|-------|--------|
| `quarry-core::ids` unit              | 2     | ✅     |
| `quarry-core` integration (contracts) | 30    | ✅ 2026-10-19 run (R8b; 11 when added) |
| `quarry-security` integration        | 11    | ✅     |
| `quarry-transform` integration       | 10    | ✅     |
| `pkg/quarrycontracts` Go             | ~6    | ✅     |
| `control/internal/store` Go          | 4     | ✅     |
| `control/internal/resources` Go      | 5     | ✅     |
| `control/internal/store/pg` Go       | 2 (skip w/o DSN) | ✅ |
| `quarry-runtime` publisher wiremock  | 1     | ✅     |
| `quarry-edge` sse_stream             | 1     | ✅     |
| `control/internal/dispatcher` Go     | 7     | ✅     |
| `quarry-edge` cache_policy           | 9     | Needs a live Redis (`REDIS_URL`, default 127.0.0.1:6379). Without one each case prints `skip: REDIS_URL unreachable` and passes, so a bare `cargo test` asserts nothing here. With `REQUIRE_CONTRACT=1` an unreachable Redis fails each case. CI runs it in the `cargo-check` gate step "Page-cache and ZDR cache gates (Redis)" against a `redis:7-alpine` service on port 6380, with `REQUIRE_CONTRACT=1` (R8a). 2026-10-19: 9 passed with Redis; 9 passed with no env (all skip); 9 failed with `REQUIRE_CONTRACT=1` and no Redis |
| `quarry-browser` (pool + browserless + session/profiles, slot book, deadlines, live-gated driver tests, `live_gate`) | 172 (4 ignored) | ✅ 2026-10-21 run (V9, HEAD `95f94e36`, the same command, image and flags as below, one run): lib 172 passed, 0 failed, 4 ignored (the four are the three public-origin cookie tests and the memory measurement, run in no lane; the 34 more than at `c7429f2e` are the tests of WS3.K, trusted input: 9 unit and 25 live-file tests, see RUNBOOKS "Trusted input for ref actions") and `tests/ssrf.rs` 4 passed, 1 ignored. Earlier, 2026-10-20 run (U8, HEAD `c7429f2e`, `--features chromiumoxide`, `CHROMIUMOXIDE_TEST=1`, Chromium 155.0.8059.79 in `quarry-rust-env:1`, `--test-threads=1`): lib 138 passed, 4 ignored (135 at `137ba2ac`, 129 at R8b; the 9 `live_gate` tests are in it) and `tests/ssrf.rs` 4 passed, 1 ignored (all 5 were ignored until `3496dd07`); with the flag unset the same command is 138 passed and 4 passed (the live tests skip); with `CHROME=/nonexistent` 119 passed and 19 failed (rc 101); without the feature the lib is 85 passed and `tests/ssrf.rs` 0. The workspace run shows the lib at the same count (138 then, 172 at `95f94e36`), because `quarry-edge` and `quarry-runtime` switch the feature on. The `quarry-browser-live` CI job has had no `continue-on-error` since `bf2e7882`; whether it is a required status check on `main` is a repository setting that was not verified (it has not run on GitHub Actions) |
| `quarry-transform::readability` unit | 4     | ✅ added 2026-04-29 |
| `quarry-transform::diff` unit (semantic) | 6 | ✅ added 2026-04-29 |
| `quarry-security::urlsig` unit       | 10    | ✅ added 2026-04-29 |
| `quarry-transform` integration (incl. strict_determinism_roundtrip) | 11 | ✅ +1 from 2026-04-29 |
| `quarry-control/resources` Go (incl. 6 preset tests) | 11 | ✅ +6 from 2026-04-29 |
| `quarry-orchestrator/workflows` Go (testsuite) | 5 | ✅ added 2026-04-29 |
| `quarry-runtime::agent_loop` domain check + planner e2e | 4 | ✅ added 2026-05-07 |
| `quarry-runtime::planner` MockPlanner       | 2     | ✅ added 2026-05-07 |
| `quarry-runtime::event_bus` InProcessEventBus | 2   | ✅ added 2026-05-07 |
| `quarry-runtime::mp_client` ModelPlanePlanner | 9   | ✅ added 2026-05-07 |
| `quarry-runtime::grant_validator` GrantValidator | 9 | ✅ added 2026-05-07 |
| `quarry-edge` profile_roundtrip integration | 11    | ✅ 2026-10-19 run: 11 passed (5 when added 2026-05-07) |
| `quarry-runtime::crawl_frontier`     | 11    | ✅ added 2026-05-08 |
| `quarry-runtime::structured_extract` | 6     | ✅ added 2026-05-08 |
| `quarry-runtime::s3_profile_store` keys | 2  | ✅ added 2026-05-08 |
| `quarry-runtime::agent_loop` cost guard | 2 | ✅ added 2026-05-08 |
| `quarry-core::crawl_denial`          | 3     | ✅ added 2026-05-08 |
| `quarry-transform::robots`           | 8     | ✅ added 2026-05-08 |
| `quarry-transform::sitemap`          | 5     | ✅ added 2026-05-08 |
| `quarry-transform::source_trace`     | 5     | ✅ added 2026-05-08 |
| `quarry-edge::firecrawl_adapter`     | 5     | ✅ added 2026-05-08 |
| `quarry-edge::audio_routes`          | 1     | ✅ added 2026-05-08 |
| `quarry-edge` zdr_claim_floor | 22 | ✅ 2026-10-19: 22 passed with no env, and 22 passed with Redis and `REQUIRE_CONTRACT=1`. Its Redis case skips without Redis, and with `REQUIRE_CONTRACT=1` and no Redis 21 passed and 1 failed (`zdr_claim_floor.rs:1036`) |
| `quarry-edge` unit tests, `src/lib.rs` | 381 | ✅ 2026-10-21 run (V9, HEAD `95f94e36`; 379 at `c7429f2e`, 364 at R8b) |
| `quarry-edge` unit tests, `src/main.rs` (same modules compiled again) | 375 | ✅ 2026-10-21 run (V9; 373 at `c7429f2e`, 358 at R8b) |
| `quarry-edge` agent_run_control | 30 | ✅ 2026-10-21 run (V9; 28 at `c7429f2e`: the opener-cap test of `1932afdf` and the vanished-tab test of `73b6003b` came since; 17 at R8b: `dec93d1e` added 11 tests, its message says 9) |
| `quarry-edge` grant_budget_claim (WS6.E, through the real router), grant_budget_boot (starts the real `quarry-edge-rs` binary) | 8, 2 | ✅ 2026-10-20 run (U8) |
| `quarry-edge` zdr_refusal_modes (WS4.H stage A, observe and enforce; since `e32a7cf8` also the three refusals that were already enforced, each in both modes) | 8 | ✅ 2026-10-21 run (V9); it was 5 at `c7429f2e` (U8) |
| `quarry-edge` run_sweeper_integration | 23 | ✅ 2026-10-19 run (R8b; 22 in R8a) |
| `quarry-edge` scrape_records_durable | 4 | ✅ 2026-10-19 run |
| `quarry-edge` zdr_gap_rows | 9 | ✅ 2026-10-19 run |
| `quarry-edge` zdr_feature_matrix | 12 | ✅ 2026-10-19 run |
| `quarry-edge` ready_browser | 6 | ✅ 2026-10-19 run |
| `quarry-edge` log_field_discipline (log-field lint) | 22 | ✅ 2026-10-19 run (R8b; 20 in R8a) |
| `quarry-edge` other integration binaries: artifacts_fs_list 5, change_webhook_proptest 4, crawl_limits 5, drain 2, job_admission 2, metrics_recorder 7, properties 6, start_run_grants 5, zdr_behavior 4, zdr_log_capture 12 | 52 | ✅ 2026-10-19 run (R8b; 51 in R8a, start_run_grants was 4) |
| `quarry-transform` clean_html_selectors | 16 | ✅ 2026-10-19 run |
| `quarry-transform` clean_html_cost | 0 (1 ignored) | Measurement, not a check: `cargo test -p quarry-transform --test clean_html_cost -- --ignored --nocapture` prints `MEASURE` lines. Not run in R8a |
| verevon gateway `log_field_discipline` (`apps/Frontend Plane/verevonv3/apps/gateway`, includes `the_gateway_stays_within_the_log_field_baseline`) | 13 | ✅ 2026-10-19 run, native `cargo test --test log_field_discipline` |
| `quarry-core` lib, properties, change_history_proptest | 170, 10, 4 | ✅ 2026-10-21 run (V9, HEAD `95f94e36`). The lib was 119 at R8b: WS3.K added the `error` reason tests and WS12.S0 the redaction unit tests (`quarry_core::redaction`, library only). The same run: `contracts` 30 and `tests/redaction_golden.rs` 15 passed, 1 ignored (`redaction_perf_budget`, the 1 MiB budget, which a CI step runs in release; not run in the workspace run) |
| `quarry-runtime` lib | 739 | 2026-10-21 (V9, HEAD `95f94e36`): 739 passed, 0 failed in the workspace run (the crate in all: 757, with `doc_parse_sandbox` 10, `dom_summary_proptest` 4, `integration_runtime` 3 and `publisher_integration` 1). 2026-10-20 (U8, HEAD `c7429f2e`): 739 passed, 0 failed in the workspace run. History, 2026-10-19 (R8b): 733 passed and 1 failed in the workspace run, then 734 passed in each of two reruns of the lib. The failing test is `pipeline::scrape_challenge_tests::the_scrape_path_logs_the_reason_and_the_status_and_never_the_host` (assertion at `pipeline.rs:2869`); it passes alone, so it is flaky under the parallel suite. Re-measured by the review of R8b (quarry-rust-env:1, 20 cores): full lib at default threads failed 2 of 12 runs, at `--test-threads=4` 1 of 10, at `--test-threads=2` 0 of 3, at `--test-threads=1` 0 of 3; the module filter `scrape_challenge_tests` at default threads failed 35 of 43 runs and at `--test-threads=1` 0 of 15; the single-test filter 0 of 25. The failure mode is a dropped tracing event, not a miscount: in the captured failure `pipeline.rs:2855` reported left 0, right 1 with an empty event list although the refusal happened. CI runs `cargo test --workspace`, so it could fail there. **Fixed 2026-10-20 in `d711a1bb` (round 4, B-T2, run by G-small-fixes):** the cause (the callsites at `pipeline.rs:843` and `:859` are first registered concurrently by tests with no subscriber, and a stale never-interest survives a concurrent `set_default`, tracing-core 0.1.36) was confirmed by a standalone two-crate repro of the same tracing pattern (9 threads, 60 runs each: 49 failed without the fix, 0 with it). tracing-core takes a shortcut while only one dispatcher is registered and asks only the calling thread's default dispatcher, so a subscriber-less thread caches "never"; `keep_per_thread_subscribers_reliable()` in the `scrape_challenge_tests` module keeps two `NoSubscriber` dispatchers alive for the whole process, which turns the shortcut off. After the fix (quarry-rust-env:1, default threads): the module filter 0 of 20 runs failed, the full lib 0 of 20, and the last full run was 739 passed, 0 failed (the count has grown since the 734 above) |
| `quarry-runtime` Postgres store tests (`--features postgres-queue --lib postgres_`; the lib is 762 tests with that feature, 739 without) | 23 | ✅ 2026-10-21 (V9, HEAD `95f94e36`, a throwaway `postgres:16-alpine`, `REQUIRE_CONTRACT=1`, the default thread count, one run): 23 passed, 0 failed in 17.7 s, and 23 `qt_` schemas left behind, as designed. 2026-10-20 (U8) against a throwaway `postgres:16-alpine` with `DATABASE_URL`, `REQUIRE_CONTRACT=1` and `--test-threads=1`: 23 passed, 0 failed (since `c9d61e31` each test has its own schema and the set runs in parallel: 23 passed, 0 failed in 5 of 5 runs at the default thread count, and 5 of 5 runs of the tree before the change failed, see RUNBOOKS "Test hygiene, round 6"); with no `DATABASE_URL` and `REQUIRE_CONTRACT=1` the 5 `postgres_step_receipts` tests fail (rc 101). CI runs this as the step "Postgres store gates" since `f27a3327` (not run on GitHub Actions). Without a database and without `REQUIRE_CONTRACT` a default run skips them silently |
| `quarry-runtime` doc_parse_sandbox (Linux, parser child) | 10 | ✅ 2026-10-19 run (R8b) |
| `quarry-transform` lib, clean_html, markdown_blowup, transform | 156, 16, 10, 11 | ✅ 2026-10-19 run (R8b) |
| Node: openapi route-coverage, scrape-parity and challenge-parity; browser-grant-contract; zdr-contract-schema | 23; 19; 65 | ✅ 2026-10-21 (V9, HEAD `95f94e36`): `scripts/tests/*.test.mjs` of Quarry-v2 23 passed and `scripts/tests/browser-grant-contract.test.mjs` 19 passed again (`zdr-contract-schema` not re-run). 2026-10-20 (U8), native `node --test --test-reporter=tap` on the worktree at HEAD `c7429f2e` (no uncommitted change under `scripts/`): `scripts/tests/*.test.mjs` of Quarry-v2 23 passed, `scripts/tests/browser-grant-contract.test.mjs` 19 passed, `scripts/tests/zdr-contract-schema.test.mjs` 65 passed. browser-grant-contract was 15 before `1c5f000a`, 16 at `137ba2ac` (one of the 16, the check that the base Model Plane compose does not enable `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT`, could not fail there: its regex `/^s+.../m` needed a literal `s`), 17 at `b40242f7` (regex corrected, with a self-test that it matches an indented key; the mutation run is in that commit's message and was not repeated by U8) and 19 since `8b350923` (the budget fields 40-44) |
| `scripts/tests/bootstrap-broker-db.test.sh` (needs Docker: a throwaway `postgres:16-alpine`) | 34 | ✅ 2026-10-21 (V9, HEAD `95f94e36`, under the docker lock): 34 ok, `ALL PASSED`. `a1cb37c4` added the check that the probe role `other_role` exists and tightened the isolation check to the exact refusal `permission denied for database`; with the `CREATE ROLE` line removed from a copy of the test the new version fails (2 FAILED, measured by V2 in that commit), where the 33-check version still passed. 2026-10-20 (U8) at HEAD `c7429f2e` under the docker lock: 33 ok, 0 FAIL, `ALL PASSED` (19 at `137ba2ac`; `08d07ad2` added the argv shims for the three mint paths, the byte-identical env file, the revoke on Quarry-style databases, `--keep-public-connect` and the known-limitation check that the superuser can still open `browser_broker`). (Fixed by `a1cb37c4`; what follows is the state at `c7429f2e`.) One weakness remained there: the check "another ordinary role cannot connect to browser_broker" only tests that the probe login fails (`[[ $rc -ne 0 ]]`) and the `CREATE ROLE other_role` before it is not checked. Measured by U8 on a copy of the test with that one line removed: 33 ok and `ALL PASSED` still. The checks on the `browser_broker` role itself have a positive control (it connects to its own database), and the rotation check follows a check that the new password logs in (read from the file, not mutated) |
| verevon gateway binary `verevon-gateway-rs` (`apps/Frontend Plane/verevonv3/apps/gateway`) | 758 (1 ignored) | ✅ 2026-10-21 (V9), native `cargo test --bin verevon-gateway-rs` in `apps/gateway` on the working tree at HEAD `95f94e36` (which also holds WS5.G's uncommitted hunks of `knowledge/quarry.rs` and `search.rs`): 758 passed, 0 failed, 1 ignored. This is not the count of the committed tree alone, because the working tree also holds WS5.G's uncommitted hunks; the committed-only count was not measured at `95f94e36` (at `c7429f2e` it was 751, and this round committed two gateway tests, `aa54623a` and `10f91e58`). 2026-10-20 (U8), native `cargo test --locked --bin verevon-gateway-rs` on an export of HEAD `c7429f2e` (`git archive` of the gateway directory plus the three files its `include_str!` calls read and the Quarry field-list golden that `the_sensitive_field_list_equals_quarrys` reads): 751 passed, 0 failed, 1 ignored (749 at `137ba2ac`: `f3f6a367` and `52cfbf6d` added one test each); `domains::browser` 105, `domains::actions` 30, `knowledge::quarry` 22; `--test log_field_discipline` 13. The working tree's uncommitted WS5.G hunks in the gateway are not in these counts |
| `scripts/tests/zdr-contract-sync.test.sh` | 52 | ✅ 2026-10-21 (V9, HEAD `95f94e36`): 52 passed, 0 failed, and `--self-test` ok (WS12.S0 added the checks for the golden `secret-shapes.v1.json` and its pin file, 50 to 52; 7 consumers are pending, among them `mp-scrub`). 2026-10-19 (R8b) after `13c6f5ac`; it was 48 passed and 1 failed before that commit (stale digest of the ZDR matrix golden). Re-run 2026-10-20 (U8) at HEAD `c7429f2e`: 50 passed, 0 failed |
| `browser-broker` (Go, `apps/Model Plane/go/services/browser-broker`) and its end-to-end harness | go test: 4 packages ok; e2e: 7 scenarios, 285 assertions | ✅ 2026-10-21 (V9, HEAD `95f94e36`, native): `GOWORK=off go test -count=1 ./...` ok in `cmd`, `authz`, `grant`, `server`, and `go vet -tags e2e ./e2e/...` clean. The e2e run (`e2e/run.sh`, Docker) is V1's: `E2E_RESULT rc=0 scenarios=7/7`, assertions a20 b14 c72 d38 f38 g83 e20 at `37712250`, from its commit message and note, not re-run by V9; not in CI. The tagged Postgres integration suite of the grant store was not re-run |
| Verevon v3 SPA (vitest, `tsc -b`, `eslint`) | 322 of 324 files, 3700 tests (`221ed5e8`) | not re-run by V9: from the commit messages of V3 (`wf-V3-spa-errors.md`): the two files that load no test are `SupportPage.test.tsx` and `quarry-client.test.ts` (an unresolved workspace dependency, failing the same way before); the last run on the touched directories was 242 of 243 files, 2845 tests (`399d3f10`); `tsc -b` 33 errors (`RemoteSupportPage.tsx` 12, `quarry-client.ts` 21) and `eslint .` 191 warnings, 0 errors, all the baseline |
| `quarry-orchestrator` (Go) | all packages ok | not re-run by V9: `GOWORK=off go vet ./...` and `go test -count=1 ./...` from the commit messages of V4 (`bf5c7efd`, `b57c0be8`, `57607501`), which add the activity and workflow tests for a refused change record |
| **Total** | 2414 passed, 0 failed, 8 ignored | `cargo test --workspace --no-fail-fast` on 2026-10-21 (V9) at HEAD `95f94e36`, no Redis, in the quarry-rust-env:1 image, rc 0: 54 test binaries and 7 doc-test targets (61 `test result` lines), no failure. 2026-10-20 (U8) at HEAD `c7429f2e`: 2305 passed, 0 failed, 7 ignored, 53 test binaries and 7 doc-test targets, no failure (R8b on 2026-10-19: 2230 passed, 1 failed, 11 ignored; the failure was the flaky `quarry-runtime` lib test fixed by `d711a1bb`). The 8 ignored are 4 plain `#[ignore]` lib tests of `quarry-browser` (three cookie tests that need a public origin because the navigation guard refuses loopback, and the memory measurement `measure_memory_per_open_session`, which needs `CHROMIUMOXIDE_MEASURE=1`; they are not live-gated, no CI lane runs them, the `quarry-browser-live` job only lists them with `--ignored --list`, and a run with `CHROMIUMOXIDE_TEST=1` still reports them ignored, so cookie restore and cookie isolation are not covered by the live lane), its `test_valid_public_url`, `clean_html_cost`, `redaction_perf_budget` of `quarry-core` and one `quarry-runtime` doc-test (U8's 7 lacked the redaction one). `quarry-edge` alone is 975 (lib 381, bin 375, 24 integration binaries with 219 tests; 966 at `c7429f2e`, 910 at R8b). The Go rows, the gateway, the Postgres store tests and the Node rows are not part of this sum, and rows without a date were not re-run |

Coverage floor target 80% not yet enforced in CI.

Rows dated 2026-10-19 come from two runs. R8a re-ran quarry-edge and quarry-transform (quarry-rust-env:1, under the docker lock) on HEAD 23e1ad7f plus comment-only edits, and the gateway lint natively. R8b re-ran the whole Quarry workspace on the code of `e78b562c` (rows marked R8b, which also carry the R8a figure where it moved) and the Node scripts natively. Rows without a date were not re-run. Rows dated 2026-10-20 are U8 runs at HEAD `c7429f2e`. Rows dated 2026-10-21 are V9 runs at HEAD `95f94e36`, before V9's own documentation commits: the Quarry workspace, live-lane and Postgres runs mounted `apps/Ingestion Plane/Quarry-v2` of the worktree (git status listed no uncommitted change under it; V9 edited documents only), the gateway ran natively on the working tree and the broker and Node rows natively. The figures of round 6 that V9 did not re-run (the SPA's vitest, `tsc` and `eslint`, the orchestrator's Go tests, the e2e harness) are marked "not re-run" above or are in the build ledger of `docs/plans/quarry-browser-build-plan-2026-10-14.md`, taken from commit messages and hand-over notes. The Quarry workspace, live-lane and Postgres runs mounted `apps/Ingestion Plane/Quarry-v2` of the worktree, which had no uncommitted change (`git status` was clean for it and for `scripts/`; the doc edits of this commit came after the runs); the gateway run used a `git archive` of HEAD. The earlier 2026-10-20 rows at `137ba2ac` were replaced by these. Round-4 counts of the broker (Go), model-gateway and the SPA (vitest, `tsc`) are in the build ledger of `docs/plans/quarry-browser-build-plan-2026-10-14.md`, taken from commit messages and hand-over notes, not from this table.

---

## Known gaps / risk register

| Risk                                                       | Impact | Mitigation                                    |
|------------------------------------------------------------|--------|-----------------------------------------------|
| ~~In-memory control store loses state on restart~~ (fixed)  | ~~High~~ | Postgres backend w/ migrations shipped        |
| ~~No Rust browser driver yet~~ (fixed)                     | ~~High~~ | chromiumoxide driver shipped 2026-04-27       |
| Orchestrator doesn't consume control job queue yet         | Med    | Phase 5 adds dispatcher or cron-pull          |
| ~~SSE not implemented ⇒ clients can't stream scrape events~~ (fixed) | ~~Med~~ | Phase 3 SSE shipped 2026-04-24 |
| No TLS fingerprinting ⇒ Cloudflare et al. may block        | Med    | Phase 2.2 JA3 spoof — adopt rquest upstream via `wreq` |
| Current TLS-profile driver is header-level only, not real browser transport | High | Replace with `quarry-tls` adapter over `wreq` and benchmark against fingerprint endpoints |
| `wreq` is an RC and requires Rust 1.85 + CMake/BoringSSL build tooling | Med | Exact-pin `=6.0.0-rc.28`; document MSRV/tooling; before release run cargo-audit/advisory review for `wreq` + BoringSSL stack, verify crate checksums, and track stable release/license audit |
| TLS redirects need hop-by-hop SSRF validation before enablement | High | `quarry-tls` disables redirects for first cut; DriverPlan redirect support must validate each target before fetch |
| TLS profile presets can drift as browsers update | Med | Validate Chrome/Firefox/Safari profiles monthly against tls.peet.ws/browserleaks and before production GA; automate refresh after `wreq-util` license decision |
| Firecrawl has broader format/action product surface today  | High   | Add format parity workstream and response adapter |
| Readability helper exists but production pipeline may still call plain markdown converter | Med | Wire helper into `PageRunner` and add regression tests |
| Browserless driver is one-shot REST, not persistent session | Med | Add CDP/BQL persistent session path and snapshot/restore |
| ~~No `cargo audit`~~ / ~~`govulncheck`~~ in CI yet            | ~~Low~~ | cargo-audit shipped 2026-05-03; govulncheck shipped 2026-04-29 |
| ~~donor heuristics incompletely ported~~ (fixed)            | ~~Low~~ | `quarry_security::urlsig` port shipped 2026-04-29 |
| ~~Browser session capture/restore round-trip not yet exercised~~ (fixed) | ~~Med~~ | `/v1/profiles` + 5-test integration suite landed 2026-05-07; S3 backend remains follow-up |
| Rust crawl frontier still lives in Go orchestrator         | Low    | Phase 5 deferred: port to Rust w/ pause/resume/cancel signals |

---

## Recent changes (reverse chronological)

- **2026-05-08** — Cycle 14 (P0+P1+P2 gap closure batch — 12 deliverables, +56 tests):
  1. **StructuredExtractClient** (P0/QRY-06) — `crates/quarry-runtime/src/structured_extract.rs` HTTP client to Model Plane `/v1/structured/extract`. ZDR + artifact-ref validation Quarry-side; **cost ceiling enforced post-hoc** (rejects responses with `usage.cost_usd > max_cost_usd`). 6 wiremock tests including budget rejection, ZDR-without-inline-markdown rejection, default-ceiling fallback.
  2. **AgentLoop max_cost_usd enforcement** (P1/QRY-05) — `LoopTermination::MaxCostExceeded { spent_usd, limit_usd }` variant; `record_cost(usd)` accumulates via AtomicU64 micro-USD; `over_budget()` checked at every step boundary. 2 tests (accumulate + terminate, ignore NaN/negative).
  3. **CrawlDenialReason enum** (P1) — `crates/quarry-core/src/crawl_denial.rs` 11 typed reasons (OutOfScope, RobotsDisallowed, DepthExceeded, MaxPagesReached, Duplicate, ExternalHostDisabled, SecurityRejected, IncludePatternMiss, ExcludePatternHit, ContentTypeRejected, NotCrawlable) with stable codes + operator-readable messages; serde-tagged. 3 tests.
  4. **robots.txt parser** (P1) — `crates/quarry-transform/src/robots.rs` RFC 9309 minimal parser: `User-agent`, `Allow`, `Disallow`, `Sitemap`, `Crawl-delay`. Longest-prefix-wins with Allow-beats-Disallow tiebreak; wildcard `*` + end-anchor `$`; UA-specific groups override `*`. 8 tests.
  5. **sitemap.xml parser** (P1) — `crates/quarry-transform/src/sitemap.rs` for both `<urlset>` and `<sitemapindex>`; lastmod/changefreq/priority extraction; tolerant of malformed input. 5 tests.
  6. **Field-level SourceTrace builder** (P1/QRY-08) — `crates/quarry-transform/src/source_trace.rs` populates `FieldTrace[]` for title, description, lang, canonical_url, og_title/description/image, author, published_time, body (with selector for the chosen content root). 5 tests including absent-body case.
  7. **Rust crawl frontier scaffold** (P1/Phase 5) — `crates/quarry-runtime/src/crawl_frontier.rs` BFS `VecDeque` + `HashSet` dedup; URL normalization (fragment-stripped); enforces depth, max_pages, external-host, include/exclude patterns; rejected URLs recorded in `denials()` for audit. Subdomain-of-seed-host allowed. 11 tests.
  8. **S3ProfileStore** (P1) — `crates/quarry-runtime/src/s3_profile_store.rs` `aws-sdk-s3` impl of `ProfileStore`; layout `profiles/{id}.json` + flat `.profile-index/{id}` so `list()` is one prefix scan. NoSuchKey gracefully maps to `Ok(None)`. 2 unit tests for key construction (full integration deferred — needs live S3 or `aws-sdk-mock`).
  9. **`/v1/audio` Unsupported endpoint** (P2/QRY-10) — `crates/quarry-edge/src/audio_routes.rs` returns 501 with explicit reason ("audio routing belongs to Model Plane"). Makes the unsupported state explicit for clients.
  10. **Firecrawl-compat response adapter** (P2/Phase 9) — `crates/quarry-edge/src/firecrawl_adapter.rs` `adapt(NormalizedOutput) → FirecrawlResponse` with `success/data/{markdown,html,rawHtml,links,metadata{sourceURL,statusCode}}` + `_quarryFingerprint` extension. `adapt_with_inline()` for callers with materialized artifact bytes. 5 tests including field-name parity check.
  11. **4 new eval fixtures** (P0/QRY-19) — `cache_warm.html`, `js_blocked.html`, `change_precision.html`, `profile_restore.html`; expectations.json updated; **10/10 fixtures passing**.
  12. **Doc cleanup** — already-completed P0 items (donor scrape removal, readability wired into PageRunner) marked done in gap-quarry.md and PROGRESS.md; Phase 4 capture/restore round-trip, Phase 5 frontier scaffold, Phase 8 fixture coverage updated.

  Workspace tests: 283 Rust passing (was 227); +56 net new across 10 files. Eval harness: 10/10 passing (was 6/6). Robots `path_matches` bug-fixed: previously matched literal patterns containing `*` against `$`-anchored stripped form; now correctly composes wildcard + end-anchor.

- **2026-05-07** — Cycle 13 (real ModelPlaneClient + /v1/profiles + capture/restore + grant validator):
  1. **Real Model Plane integration** — `crates/quarry-runtime/src/mp_client.rs` ships `ModelPlaneClient` (HTTP client to gateway `/v1/invoke`, optional Bearer auth, 30s timeout) and `ModelPlanePlanner` implementing the `Planner` trait. The planner serializes a BrowserObservation into a structured prompt, calls the gateway, parses JSON action lists (with code-fence stripping for LLM responses), and returns `PlannerDecision::Continue|Done`. 9 tests including wiremock end-to-end for `Continue`, `Done`, and 5xx error propagation.
  2. **`/v1/profiles` REST endpoints** — `crates/quarry-edge/src/profile_routes.rs` wires `ProfileStore` (already in `quarry-browser`) into 4 routes: `POST /v1/profiles` (save, optional client-supplied ID), `GET /v1/profiles` (list), `GET /v1/profiles/:id` (load — 404 on miss), `DELETE /v1/profiles/:id` (204 on success). `AppState.profiles: Arc<dyn ProfileStore>` plumbed through `main.rs`, `sse_stream.rs`, and `cache_policy.rs` test harnesses. Default backend is `InMemoryProfileStore`; S3 swap is pluggable.
  3. **Capture/restore round-trip integration test** — `crates/quarry-edge/tests/profile_roundtrip.rs` ships 5 end-to-end tests against a live axum app: full-fidelity field round-trip (cookies × 2, local_storage, session_storage, user_agent, viewport with non-default scale, locale, timezone), list-includes-saved, delete-removes-entry, load-unknown-404, and custom-ID round-trip. Closes the long-standing risk-register gap.
  4. **BrowserBroker grant validator** — `crates/quarry-runtime/src/grant_validator.rs` ships the `GrantValidator` trait, `GrantValidation` struct (with `is_usable()` checking active flag + expiry), `NoopGrantValidator` (dev/test always-allow), and `HttpGrantValidator` (calls `POST {base}/v1/broker/grants/{id}/validate` with optional Bearer auth; 404 maps to inactive grant; 5xx propagates as `DriverFailed`). Wires Quarry to Model Plane's `BrowserBrokerService.ValidateGrant` via an HTTP shim until tonic gRPC client lands. 9 tests (4 `is_usable` cases + Noop + 4 wiremock cases).

  Workspace tests: 227 Rust passing (was 197); +9 mp_client, +9 grant_validator, +5 profile_roundtrip integration, +7 net-new in adjacent crates from required wiring updates. Eval harness: 6/6 still passing.

- **2026-05-07** — Cycle 12 (persistent profiles + eval expansion + Model Plane integration):
  1. **Persistent browser profiles** — `ProfileStore` async trait (save/load/delete/list) + `InMemoryProfileStore` implementation in `quarry-browser/src/session.rs`; `SessionSnapshot` extended with `viewport: Option<Viewport>`, `locale: Option<String>`, `timezone: Option<String>`; `Viewport` struct (width, height, device_scale_factor, is_mobile); 5 new tests (roundtrip, missing, delete, list, serde).
  2. **Eval harness expansion** — 4 new HTML fixtures: `blocked_page` (access denied / challenge page), `minimal_content` (single paragraph notice), `table_heavy` (performance report with data tables), `multi_section` (changelog with 4 distinct sections); expectations.json updated; 6/6 passing (was 2/2).
  3. **Planner trait + MockPlanner** — `quarry-runtime/src/planner.rs` with `Planner` async trait (`next_actions → PlannerDecision::Continue|Done`), `MockPlanner` (VecDeque-based step sequence); 2 tests.
  4. **EventBus abstraction** — `quarry-runtime/src/event_bus.rs` with `EventBus` async trait (publish/subscribe/unsubscribe), `EventReceiver` trait, `InProcessEventBus` wrapping existing `EventSink`; 2 tests (publish+receive, unsubscribe stops receiver).
  5. **MockPlanner→AgentLoop e2e test** — integration test in `agent_loop.rs` wiring MockPlanner (Navigate + Screenshot steps) through AgentLoop, verifying 2 observations produced and Completed termination.

  Workspace tests: 197 Rust passing (was 187); Go unchanged. Eval harness: 6/6 passing.

- **2026-05-07** — Cycle 11 continued: (1) Fixed readability boilerplate leak — added `nav`, `header`, `footer`, `aside` to strip tags and 200-point semantic bonus for `<article>`/`<main>`/`[role=main]` candidates; eval harness now 2/2 passing. (2) Python + TypeScript SDK generation — `sdks/generate.sh` runs openapi-generator-cli 7.22; Python SDK has 5 API classes + 21 models + 26 pydantic smoke tests passing; TypeScript SDK type-checks clean. (3) Fixed OpenAPI spec `const` boolean fields that broke SDK generation.

- **2026-05-07** — Cycle 11 ship batch (Waves 5-7: pipeline integration + AgentLoop + eval harness + OpenAPI):
  1. **Data Plane ingest wiring** — `PageRunner` gains optional `ingest: Option<Arc<IngestClient>>` and `org_id: Option<String>` fields; edge request schemas (`ScrapeRequest`, `InternalRunPage`) gain `ingest` and `org_id` fields; all 3 route handlers (`scrape`, `scrape_stream`, `internal_run_page`) conditionally pass IngestClient to PageRunner; post-scrape block builds `DataPlaneIngestRequest` with paragraph chunks and populated `SourceTrace` (source_url, fetched_at, fingerprint), then fires non-blocking `tokio::spawn` ingest so failures don't fail scrapes; ZDR guard prevents ingest when active; emits `StoreRecordWritten` event on success.
  2. **AgentLoop** — new `quarry-runtime/src/agent_loop.rs` combining `ObservationRunner` with constraint enforcement for agentic browsing. `AgentLoop::execute()` enforces `max_steps`, `max_runtime_s`, and `allowed_domains` (with subdomain-aware matching) before each action step; emits `agent.started`, `action.failed`, `agent.completed`/`agent.failed` events; `LoopTermination` enum captures completion reason (Completed, MaxStepsReached, MaxRuntimeExceeded, DomainViolation, Aborted). 3 unit tests for domain checking.
  3. **Eval harness foundation** — `lab/evals` Rust crate: 2 HTML fixture files (article_basic with nav/footer boilerplate, article_js_heavy with SPA structure); `expectations.json` manifest with quality checks per fixture (title, lang, must_contain, must_not_contain boilerplate, min_links, link_must_contain, fingerprint_stable); `quarry-eval` binary reads fixtures, runs transform pipeline, scores results, writes `scoreboard.json`; 1/2 passing — immediately surfaced real readability quality gap where nav/footer boilerplate leaks into extracted content.
  4. **OpenAPI 3.1 spec** — hand-crafted `docs/openapi.yaml` covering all 7 endpoints (/health, /ready, /v1/scrape, /v1/scrape/stream, /v1/crawl, /v1/batch, /v1/internal/run_page) with full component schemas (ScrapeRequest, CrawlRequest, BatchRequest, InternalRunPage, CachePolicy, DriverSignals, NormalizedOutput, all envelope types) matching Rust type definitions.

  Workspace tests: 187 Rust passing (was 184); Go unchanged.

- **2026-05-04** — Cycle 10 TLS foundation: contracts now lock rquest upstream via the crates.io `wreq` package (BoringSSL-backed TLS/JA3/JA4/H2 emulation) and defer `wreq-util` until license review. Added `crates/quarry-tls` as the narrow adapter over exact-pinned `wreq = 6.0.0-rc.28`, bumped Rust MSRV to 1.85 to match the dependency, and documented the CMake/BoringSSL build-tool requirement. `quarry-runtime::TlsProfileDriver` now delegates to `WreqTlsClient` instead of the old `reqwest`/`rustls` header-level shim. Redirects are explicitly disabled in the TLS adapter until hop-by-hop SSRF validation lands. Focused tests: `cargo test -p quarry-tls -p quarry-runtime`; focused lint: `cargo clippy -p quarry-tls -- -D warnings` and `cargo clippy -p quarry-runtime --no-deps -- -D warnings`.

- **2026-04-29** — Cycle 9 ship batch (Phase 2/6/7 + tests):
  1. **govulncheck CI** added to `.github/workflows/security.yml` as a matrix job over `services/quarry-control`, `services/quarry-orchestrator`, and `pkg/quarrycontracts`; closes the remaining half of the cargo-audit/govulncheck risk-register row.
  2. **chromiumoxide action primitives** — `BrowserDriver` trait extended with default `Unsupported` impls for `wait_for`, `click`, `type_text`, `scroll`, `press`, `evaluate`; chromiumoxide overrides them via Page/Element APIs (poll-based selector wait, scroll via `window.scrollTo` + `scroll_into_view`, `press_key` via body-element handle). `ActionRuntime` now drives the new methods; `Wait` is a pure `tokio::time::sleep`.
  3. **Readability-first markdown converter** — `quarry_transform::readability::extract` scores `<article>`, `<main>`, `[role=main]`, `#content`, `.entry-content` etc. by paragraph length × `(1 - link_density)` and strips `script`/`style`/`noscript`/`iframe`/`svg`. `html_to_readable_markdown` is the one-shot helper.
  4. **Donor URL heuristics ported** — `quarry_security::urlsig::analyze` mirrors `internal/security/heur/urlsig.go`: scheme, suspicious TLD, IP-as-host, homograph (Cyrillic/Georgian/Greek), typosquat (substitution + 1-edit-distance) against 17-domain popularity list, path/query suspicious patterns, length and digit-ratio analysis, URL-shortener detection, phishing keyword scan with trusted-domain bypass; confidence-weighted `suspicion_score` capped at 1.0.
  5. **Semantic-ish paragraph diff** — `quarry_transform::diff::diff_paragraphs` returns `SemanticDiff { ops, added, removed, moved, unchanged }` with `is_real_delta()` gating against pure reorderings or whitespace-only changes. Hashing collapses whitespace before fingerprinting so reflowed paragraphs match.
  6. **Change events carry text_fingerprint** — `pipeline.rs` now emits `text_fingerprint` (whitespace + case-normalized) alongside the byte fingerprint so consumers can suppress boilerplate-only churn at the subscription layer.
  7. **Preset catalog** — `services/quarry-control/internal/resources/presets.go` ships four presets (`fast`, `polite`, `stealth`, `deterministic`) exposed via `GET /v1/presets` and `GET /v1/presets/{name}`; `POST /v1/jobs` accepts `preset: "<name>"` to materialize the policy. Unknown preset names → 400. 6 new resource tests.
  8. **Strict-mode determinism test** — `tests/transform.rs::strict_determinism_roundtrip` runs the full HTML→readable→markdown→links→metadata→fingerprint pipeline twice + verifies byte equality, plus a third invocation guard against state baked across calls.
  9. **Orchestrator workflow tests** — 5 new tests under `internal/workflows/workflows_test.go` covering `ScrapeJobWF` happy path + failure propagation, `BatchJobWF` per-page failure tolerance, `CrawlJobWF` `MaxPages` boundary (exact visit count) and frontier dedupe.

  Workspace tests: 116 Rust passing (was 96), Go orchestrator workflows 5/5, Go control resources includes 6 preset tests.

- **2026-05-04** — Stub remediation pass: `ActionRuntime` no longer silently no-ops unsupported actions when a `BrowserSession` is attached. Added `ErrorCode::Unsupported` (HTTP 501) + `QuarryError::unsupported_action(&str)` helper in `quarry-core`. `Wait`, `WaitFor`, `Click`, `Type`, `Scroll`, `Press`, and `Evaluate` now return `Unsupported` errors when invoked against an attached session, while remaining trace-only in dry-run mode (no driver) so existing tests stay green. Cleaned up "stub" wording in `quarry-runtime/action_runtime.rs` and `quarry-transform/markdown.rs` doc comments.
- **2026-05-03** — Cycle 8 ship batch: (1) cargo-audit CI workflow in `.github/workflows/security.yml` running on push/PR/weekly cron (commit `0948dd5`); (2) `meta.json` sidecar emission in `quarry-transform` via `write_meta_sidecar` capturing run_id, source_url, fingerprint, timestamps, byte sizes alongside normalized output (commit `ccac297`); (3) determinism verification mode `verify_deterministic` returning `DeterminismError::Mismatch { first, second }` on divergent fingerprints with 2 unit tests (commit `617c546`); workspace tests 96 green.
- **2026-05-02** — Cycle 7 ship batch: (1) Canary splitter on edge feature flag — deterministic hash-bucket router on `org_id` with `QUARRY_EDGE_CANARY_PCT` env (commit `b9da284`); (2) Blocklist persistence endpoint — `POST/DELETE /v1/blocklist/{host}` with Postgres+memory backends and migration `005_blocklist.sql` (commit `f132276`); (3) Lease pool metrics snapshot — `LeaseMetrics` (AtomicU64 acquires/releases/timeouts, CAS-loop peak in-flight) exposed via `LeasePool::metrics_snapshot() -> LeaseMetricsSnapshot` (commit `3d31047`); 96 → 96+ tests green across all three.
- **2026-05-01** — Tag generator harness: deterministic `FixtureTagger` keyed on `sha256(canonical(input))` + injectable seed in `services/quarry-control/internal/tagging`; golden-fixture suite (basic/empty/multi) plus whitespace-insensitivity, seed-sensitivity, and context-cancellation cases; `-update` flag rewrites fixtures; commit `356e2d9` (5 files, 378 insertions); `go test ./...` PASS.
- **2026-04-30** — Schedule run history endpoint: `GET /v1/schedules/{id}/runs` with cursor pagination (base64 `created_at|id`), memory + Postgres backends, migration `004_jobs_schedule_id.sql` adds `schedule_id` column to `jobs`, `JobsStore.ListBySchedule` honours cursor + emits `next_cursor` on full pages; 5 new `TestScheduleRuns` cases green.
- **2026-04-29** — Browserless REST `BrowserDriver`: HTTP client against `/content`, `/screenshot`, `/pdf` endpoints with optional `?token=` auth and 60s timeout; nav-state `Mutex` enforces goto-before-content; 7 wiremock tests green (`cargo test -p quarry-browser` 12/12).
- **2026-04-28** — Phase 4 Browser LeasePool: TTL eviction, ProxyAffinity sticky reuse, tokio::Semaphore concurrency cap, Deref-based PooledSession; 12 tests green.
- **2026-04-27** — Phase 2 carryover chromiumoxide BrowserDriver: feature-gated `chromiumoxide` module in `quarry-browser` (`chromiumoxide = ["dep:chromiumoxide", "dep:futures"]`); lazy `Browser::launch` via `BrowserConfig::builder().build()` with event-drain `JoinHandle` stored in `Arc<Mutex<Option<...>>>` and aborted on `Drop`; `BrowserDriver` impl covers goto/content/screenshot (Png + `capture_beyond_viewport`)/pdf (`PrintToPdfParams::default`); env-gated integration test `CHROMIUMOXIDE_TEST=1 launches_and_fetches_content` plus 7 unit tests; `cargo test --features chromiumoxide -p quarry-browser` 8/8 green.
- **2026-04-26** — Phase 3.1 Redis edge cache: `quarry-edge` now honours `CachePolicy { mode, max_age_s }` on `/v1/scrape`. Added `PageCache::put_with_ttl` and `DEFAULT_TTL_SECS = 3600`. Routes consult `should_read`/`should_write`/`effective_ttl` helpers to skip reads for `WriteOnly`/`Bypass`, skip writes for `ReadOnly`/`Bypass`, and use `max_age_s` (when > 0) as the TTL otherwise falling back to default. Fixed 422 deser bug by adding `#[serde(default)]` to `CachePolicy::max_age_s`. Integration suite `crates/quarry-edge/tests/cache_policy.rs` covers bypass-always-hits-origin, read-write-serves-second-call, write-only-skips-read-but-populates, and max-age-zero-uses-default-ttl (4/4 green against authenticated Redis via `REDIS_URL`).
- **2026-04-25** — Phase 1.4 webhook dispatcher: `internal/dispatcher` polls due deliveries via `WebhookDeliveryStore.ClaimDue`, signs body with `X-Quarry-Signature: t=<ts>,v1=<hmac-sha256("<ts>.<body>")>`, retries on 5xx/408/429/transport errors with exponential backoff `[1s,5s,30s,5m,1h]`, sends to DLQ on other 4xx or after 5 attempts, marks inactive webhooks as `failed`. Wired into `cmd/control/main.go`. White-box test suite 7/7 green (Success, RetryOn5xx, DLQOn4xx, RetryOn429, InactiveWebhook, TransportError, DLQAfterMaxAttempts).
- **2026-04-24** — Phase 3 SSE streaming: `/v1/scrape?stream=true` returns `text/event-stream` of run envelopes from runtime mpsc; SSRF gating wired (`ErrorCode::SecurityBlocked` → HTTP 403); workspace clippy clean (`-D warnings`) and `cargo test --workspace` green (73/0/0).
- **2026-04-23** — Runtime→control event publisher: Rust `EventPublisher` batches up to 64 events from mpsc and POSTs `{base}/v1/runs/{run_id}/events` with bearer auth; Go receiver endpoint added to both `quarry-control` and `quarry-orchestrator` with bearer auth + `quarrycontracts` shared types; wiremock integration test `publisher_posts_batch_with_bearer_auth` in `quarry-runtime`. Rust `cargo test --workspace` green (72/0/0, 0 warnings); Go tests green per-module (no root go.mod by design).
- **2026-04-22** — Phase 1.1 Postgres backend for control plane: `pgxpool`, embedded migrations (`001_init.sql`), full `DB` impl with keyset cursor pagination, `EventLog.ForJob`, `/v1/jobs/{id}/history` route. Select via `QUARRY_CONTROL_DSN` env (empty = in-memory dev mode). Events table enforces unique `(run_id, seq)` per contract §1.3.
- **2026-04-22** — Edge internal `/v1/internal/run_page` endpoint; real HTTP handoff edge→control; control event-append endpoint; Rust + Go test suites (48 tests green).
- **2026-04-22** — Phase 0 contracts frozen; four deployables scaffolded; docker-compose + Dockerfiles; cargo + go workspaces compiling clean.

---

- **2026-04-22** — Schedule cron validation + enable/disable: `internal/cron.Validate`, pointer-receiver Schedule.Validate auto-fills ID, `POST /v1/schedules/{id}/enable|disable` via `SchedulesStore.UpdateEnabled`, pg + memory impls, `enabled` column in migration, 4 new tests green.

---

## Next 3 things

1. **Kernel browser driver** (P1/QRY-02) — fourth `BrowserDriver` impl alongside Chromiumoxide, Browserless, Browserbase. CDP-based; Kernel cloud session API.
2. **Tonic gRPC `GrantValidator`** — replace HTTP shim with real `BrowserBrokerService.ValidateGrant` client. Needs proto vendoring + tonic-build script + integration test.
3. **NATS `EventBus` impl** — second `EventBus` impl behind the trait. Pre-req for distributed agentic browsing (long-running jobs across nodes).

Pick one or launch in parallel.
