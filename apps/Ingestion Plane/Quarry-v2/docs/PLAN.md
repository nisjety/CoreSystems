# Quarry V2 — Detailed Plan

Granular work plan. Pairs with `ROADMAP.md` (phase shape) and `GOAL.md` (north star).
Each workstream: scope, deliverables, tests, exit criteria.

---

## Donor strategy

### Quarry V1 donor map

Use `/Volumes/Lagring/Triodelab/CoreSystem/apps/Ingestion Plane/Quarry` as the product and behavior donor.

| V1 area | V2 target | Notes |
|---------|-----------|-------|
| `internal/crawl/runner.go` | Rust frontier + Go Temporal envelope | Port robots, sitemap, depth, dedupe, errors, documents; keep frontier mutation in Rust |
| `internal/session/manager.go` | Browser profile/snapshot/lease APIs | Port TTL, org/user ownership, profile UX; replace Rod page ownership with lease/profile snapshot |
| `internal/scraper/scraper.go` | Format pipeline + schema/prompt behavior | Port semantics, not structure |
| `internal/driver/*` | DriverPlan + challenge/block classification | Learn from Rod/stealth/waterfall/challenge detection; do not clone driver tangle |
| `internal/api/*` | REST parity checklist | Keep useful endpoints; keep V2 contracts strict |
| `internal/security/*` | `quarry-security` | Continue porting heuristics and SSRF/DNS guard |

### Firecrawl donor map

Use `/Volumes/Lagring/Triodelab/firecrawl` as the product surface and engine-strategy donor.

| Firecrawl area | V2 target | Notes |
|----------------|-----------|-------|
| `apps/api/src/controllers/v2/types.ts` | Request/format/action schema | Adopt compatible names and limits where practical |
| `apps/api/src/scraper/scrapeURL/engines/index.ts` | Rust `DriverPlan` | Engine feature scoring + fallback list + unsupported feature reporting |
| `apps/api/src/scraper/scrapeURL/transformers/index.ts` | Rust/control format pipeline | Raw → clean HTML → markdown → links/images/metadata → extraction/diff |
| `apps/api/src/scraper/WebScraper/crawler.ts` | Crawl denial reasons | Typed skip reasons with operator-readable messages |
| `apps/playwright-service-ts/api.ts` | Browser service hardening | SSRF route interception, DNS guard, ad/media blocking, per-page semaphore |
| `apps/go-html-to-md-service` | Markdown benchmark | Compare against Rust readability/html2md and Firecrawl Go converter |
| `SELF_HOST.md` | Self-host strategy | Fire Engine gap is V2's opportunity: make advanced drivers first-party or pluggable |

### External references

- `rquest` upstream / `wreq` crate (`https://docs.rs/wreq/latest/wreq/`): default Rust TLS impersonation transport; supports TLS/JA3/JA4 and HTTP/2 emulation with BoringSSL. Use direct `wreq::Emulation` presets in the hot path; evaluate `wreq-util` only after explicit license review.
- `impit` (`https://github.com/apify/impit`): fallback experiment only; useful but heavier because it needs patched dependencies and unstable flags.
- `curl_cffi` (`https://curl-cffi.readthedocs.io/`): Python lab baseline for TLS/H2/H3 impersonation.
- Browserless persistent sessions (`https://docs.browserless.io/browserql/session-management/persisting-state`): session persistence, reconnect URLs, explicit stop URL, CDP/BQL connect fields.
- Crawlee session management (`https://crawlee.dev/python/docs/0.6/guides/session-management`): session pool, proxy rotation, retire/mark_bad/mark_good health model.
- Scrapy AutoThrottle (`https://doc.scrapy.org/en/latest/topics/autothrottle.html`): per-host latency-driven delay; errors may increase delay but must not decrease it.
- Mozilla Readability (`https://github.com/mozilla/readability`) and Trafilatura (`https://trafilatura.readthedocs.io/`): extraction quality baselines.

## Phase 0 — Freeze contracts ✅

**Scope:** IDs, envelopes, events, policy, output, cache, lease schemas.

**Deliverables:**
- `docs/CONTRACTS.md` — authoritative spec.
- `crates/quarry-core` — Rust types + ID newtypes + serde.
- `pkg/quarrycontracts` — Go mirror.

**Tests:**
- ID roundtrip + prefix rejection (Rust + Go).
- Envelope serde shape (ok + err paths).
- Error code ↔ HTTP status matrix.
- Default policy sanity.

**Exit:** All contract tests green in both languages. ✅ done.

---

## Phase 1 — Go control plane to parity

**Scope:** Replace donor Go API resource endpoints in V2.

### 1.1 Postgres store backend
- `internal/store/pg.go` — `DB` interface implemented via `pgxpool`.
- Schema migrations in `internal/store/migrations/*.sql` (goose or `pgx/migrate`).
- Tables: `jobs`, `stores`, `snapshots`, `artifacts`, `profiles`, `schedules`, `events`, `checkpoints`.
- Indexes: `events(run_id, seq)`, `jobs(kind, status, created_at DESC)`.
- Keep `NewMemory()` for tests.

**Tests:** Testcontainers-postgres integration suite, same contract as memory tests.

### 1.2 Resource parity
- List + cursor pagination (opaque base64-encoded cursor) on every resource.
- `GET /v1/jobs/{id}/history` — checkpoints + events interleaved.
- `GET /v1/runs/{id}/events?after_seq=N&limit=...` (exists ✅).
- `POST /v1/schedules` + cron parsing + enable/disable toggle.
- `POST /v1/profiles` + S3-backed snapshot URI.

### 1.3 Event history durability
- `events` table append-only.
- Unique constraint on `(run_id, seq)` to prevent dupes.
- Idempotency via `idempotency_key`.

### 1.4 Webhook dispatcher
- Table `webhook_deliveries(id, url, secret_id, status, attempts, next_attempt)`.
- Worker poller in control (`internal/webhooks`).
- HMAC-SHA256 signature header, exp backoff, 24h DLQ.

**Exit:** Donor-equivalent CRUD + history working on Postgres; webhook delivery at-least-once.

---

## Phase 2 — Rust runtime under current API

**Scope:** Build full runtime stack behind feature flags. Edge keeps using in-process Rust runtime; orchestrator activities call `/v1/internal/run_page`.

### 2.1 Security engine (`quarry-security`)
- Port donor `internal/security/heur/` rules completely (TLD, cert mismatch, sus headers, challenge detect).
- Add DNS resolution guard (resolve host → `heur::resolve_guard`) before fetch.
- Persistent blocklist backend via control plane (`/v1/security/blocklist`).
- **Tests:** ≥25 heuristic cases, property-test for URL normalization.

### 2.2 Fetch engine (`quarry-runtime::fetch`)
- Static driver exists ✅.
- Replace the current fake TLS-profile shape with a real impersonating driver.
- Add `crates/quarry-tls` as the narrow adapter over the rquest upstream `wreq` crate for TLS/JA3/JA4/H2 emulation, cookies, proxies, and compression.
- Keep `impit` behind a future experiment only if `wreq` cannot satisfy target sites; document patched dependency cost before it enters the hot path.
- Keep Python lab `curl_cffi` as a comparison baseline only.
- Cookie jar per lease (not per request).
- Compression: gzip + brotli + zstd.
- Add transport fingerprint artifact: observed JA3/JA4/H2/H3 fields when tested against fingerprint endpoints.
- **Tests:** local wiremock + optional internet-gated fingerprint tests; block-prone corpus A/B vs `reqwest`.

### 2.3 Browser runtime (`quarry-browser`)
- `chromiumoxide`-based `BrowserDriver` impl (local Chrome).
- Remote Browserless client impl (same trait).
- Session affinity → lease key; sticky proxy routing.
- Action runtime: wait/click/scroll/screenshot/pdf/evaluate.
- Add persistent CDP/BQL session driver:
  - store `connect`, `browserQL`, `stop`, reconnect URL as secret metadata;
  - explicit cleanup on release;
  - expired-session recreate + retry path;
  - profile hydration before navigation.
- Add Playwright-compatible storage-state semantics to snapshots: cookies, localStorage, sessionStorage, IndexedDB, UA, viewport, locale, timezone.
- Keep one-shot Browserless REST `/content` as fallback only.

### 2.4 Transform pipeline (`quarry-transform`)
- Wire readability-first markdown into the production pipeline, not only tests/helpers.
- Benchmark against:
  - Firecrawl Go HTML-to-Markdown service;
  - Mozilla Readability;
  - Trafilatura;
  - current `html2md`.
- Add Firecrawl-compatible transform chain:
  1. raw HTML/body bytes;
  2. cleaned HTML (`only_main_content`, `include_tags`, `exclude_tags`, base64 image removal);
  3. markdown;
  4. metadata;
  5. links/images;
  6. requested extractors.
- Structured extraction hook: `Extract` trait; default impl = schema-less; pluggable for LLM-backed extract via control plane job.
- Fingerprint: blake3 content (✅) + semantic text fingerprint (whitespace/case normalized ✅).
- Add `attributes` extractor (CSS selector + attribute).
- Add `images` extractor.
- Add `branding` artifact shape behind browser/CDP feature gate.
- Add `summary`, `query`, `json` as control/lab jobs over captured artifacts, not runtime dependencies.

### 2.5 Artifact store (`quarry-runtime::artifact_store`)
- S3 backend via `aws-sdk-s3` (rustls, no OpenSSL).
- Local FS backend for dev.
- Signed URL issuance for cross-plane consumption.

### 2.6 Diff engine (`quarry-transform::diff`)
- Fingerprint compare (✅).
- Add semantic-ish diff: paragraph-level match, return added/removed/changed blocks.
- Output as `ChangeDetail` sidecar artifact (`meta.json`).
- Add Firecrawl-compatible `changeTracking` modes:
  - `json`: structured paragraph ops;
  - `git-diff`: markdown line/paragraph diff.

**Exit:** Rust runtime passes full scrape integration suite against real sites (20+ fixtures).

### 2.7 DriverPlan and engine fallback
- `quarry-runtime/src/driver_plan.rs` with feature scoring.
- Inputs: requested formats, actions, URL type, cache policy, policy preset, profile requirement, prior block signals, timeout.
- Outputs: chosen driver, fallback order, unsupported features, reason codes.
- Emit driver-plan metadata to `meta.json`.
- **Tests:** table tests mirroring Firecrawl engine matrix: actions require browser; JSON can use static + transform; screenshot requires browser; PDF/document route specialty; profile disables cache-only path.

---

## Phase 3 — Fast-path cutover

**Scope:** Move `/v1/scrape` immediate path + SSE + cache-hit to edge.

### 3.1 Edge cache layer
- `crates/quarry-edge/src/cache.rs` — Redis-backed.
- Key: `blake3(url + vary_on_headers + render.js)`.
- Respect `CachePolicy.mode` (bypass/read_only/read_write/write_only).
- Stale-while-revalidate via background revalidate task.

### 3.2 SSE streaming
- Edge SSE for `/v1/scrape?stream=true` — emit events from runtime `EventSink`.
- Reuse `quarry-core::event::Event` as event-stream payload.

### 3.3 Remove donor `/v1/scrape` from Go API
- Donor Quarry freezes on scrape; new traffic hits edge.
- Dual-run canary: 5% → 50% → 100% via feature flag.
- Add Firecrawl-compatible response adapter for format names and action results while retaining Quarry envelopes internally.

**Exit:** p50 warm scrape ≤ 120ms; SSE event parity with donor; donor scrape path removed.

---

## Phase 4 — Browser lease model

**Scope:** First-class lease + profile + sticky affinity.

### 4.1 Durable profiles (Go control)
- `/v1/profiles` CRUD.
- `/v1/profiles/{id}/snapshots` — capture current session state.
- `/v1/profiles/{id}/restore` — produce pre-warmed lease.
- Storage: S3 blob per snapshot, metadata in Postgres.

### 4.2 Runtime leases (Rust)
- `LeasePool` (✅ stub) fleshed out: TTL, eviction, affinity map.
- `BrowserSession` lifecycle: acquire → reconnect if cached → else fresh.
- Sticky proxy routing: `ProxyAffinity.sticky_key` pins outbound IP.

### 4.3 Capture/restore
- Capture: cookies + localStorage + sessionStorage + UA + (optional) cache.
- Restore: hydrate new Chrome context, run `/v1/profiles/{id}/restore_probe` URL to validate.
- Capture IndexedDB when driver supports it.
- Persist Browserless session URLs only in secret metadata.
- Add explicit session stop/cleanup operation to avoid relying on TTL expiry.

**Exit:** Profile restore success ≥ 99%; sticky-proxy reuse measurable in scoreboard.

---

## Phase 5 — Crawl workers to Rust

**Scope:** Move per-page execution out of Temporal activities into Rust-native queue. Temporal stays outer envelope.

### 5.1 Per-run Rust queue
- `crates/quarry-runtime/src/crawl.rs` — Rust-native frontier queue.
- Feeds from Temporal workflow via `/v1/internal/enqueue`.
- Emits `page.*` events back to control event log.
- Implement components from `ARCHITECTURE.md`: `RequestQueue`, `SeenSet`, `HostSlot`, `SessionPool`, `DenialReason`, `Checkpoint`.
- Port V1/Firecrawl crawl options:
  - `includePaths`, `excludePaths`;
  - `allowExternalLinks`, `allowSubdomains`, `allowBackwardCrawling`, `crawlEntireDomain`;
  - `ignoreSitemap`, `sitemapOnly`, sitemap index cap;
  - `maxDepth`, `maxDiscoveryDepth`, `limit`;
  - `ignoreRobotsTxt`, robots crawl-delay.
- Generate typed denial reasons for skipped links.

### 5.2 Checkpoint protocol
- Every N pages or every T seconds → workflow signal back.
- Workflow persists checkpoint to control (`cp_*`).
- Resume: workflow re-hydrates frontier from latest checkpoint.

### 5.3 Cancel/pause/resume signals
- Workflow → runtime via `/v1/internal/runs/{id}/signal`.
- Runtime drains gracefully.

### 5.4 Adaptive throttling
- Per-host latency EWMA.
- Target concurrency per host.
- Non-2xx/block responses may increase delay but must not decrease delay.
- Session/proxy health: `mark_good`, `mark_bad`, `retire`.
- Resource-aware concurrency: pause new browser work when runtime CPU/RAM/browser lease pressure crosses policy thresholds.

**Exit:** Donor crawl test suite passes against new split; crash-recovery ≤ 10s.

---

## Phase 6 — Normalized output + change tracking in Rust

**Scope:** Output envelope owned end-to-end by Rust.

### 6.1 Normalized output envelope
- `quarry-core::output::NormalizedOutput` (✅ schema).
- Runtime assembles; edge returns; orchestrator persists reference in control.

### 6.2 Change metadata artifact
- `meta.json` per page includes `change.status`, `prev_fingerprint`, `diff_summary`.
- `change.detected` event fires only on real content delta (not byte-level noise).

### 6.3 Chunk boundaries
- `quarry-transform::chunk` (✅ paragraph-based).
- Published as `chunks.json` artifact. No embeddings — that's Data Plane.

### 6.4 Firecrawl format parity
- `images.json`
- `attributes.json`
- `summary.txt`
- `extract.json`
- `branding.json`
- `audio.json` behind specialty gate
- `change.json`
- action result artifacts: screenshots, scrape checkpoints, JS returns, PDFs

### 6.5 API response adapters
- Firecrawl-compatible field naming at edge where useful (`rawHtml`, `sourceURL`, format object responses).
- Quarry canonical internal names remain snake_case and artifact-based.
- SDK compatibility tests against V1 client expectations.

**Exit:** Change-detection precision ≥ 95% on gold set.

---

## Phase 7 — Presets + determinism

**Scope:** Policy registry in Go; enforcement in Rust.

### 7.1 Preset catalog
- `control/internal/presets/` — named RunPolicy bundles (e.g., `aggressive_js`, `gentle_docs`, `strict_compliance`).
- `GET /v1/presets` — list; `POST /v1/presets` — create (admin only).

### 7.2 Determinism knob
- `Determinism::Strict` — fixed UA, fixed viewport, no random jitter, robots strict.
- `Determinism::BestEffort` — default.
- `Determinism::Off` — full jitter allowed.
- Runtime asserts chosen mode in artifact meta.

**Exit:** Same URL + strict preset ⇒ identical fingerprint across 3 runs.

---

## Phase 8 — Benchmark + retire

**Scope:** Prove it, then kill donor execution path.

### 8.1 Scoreboards
- Harness in `lab/evals/` (Python).
- Metrics from GOAL.md: latency, JS success, block rate, restore, schedule, precision, recovery.
- Weekly report to a `docs/SCOREBOARD.md` auto-generated page.
- Baselines:
  - Quarry V1 local;
  - Firecrawl self-host local;
  - Firecrawl cloud if API key is available;
  - Quarry V2 static/impersonated/browser paths;
  - Trafilatura/Mozilla Readability/Firecrawl Go converter for extraction quality.
- Corpus buckets:
  - static HTML;
  - JS-heavy;
  - bot-sensitive/TLS-sensitive;
  - e-commerce/product;
  - docs/blog/news;
  - PDF/document;
  - login/profile restore;
  - crawl with sitemap/robots/path filters;
  - change tracking gold set.

### 8.2 Canary rollout
- 5% → 25% → 50% → 100% traffic over 2 weeks.
- Rollback threshold: any scoreboard metric drops >10% vs. baseline.

### 8.3 Donor path removal
- Delete `internal/scraper`, `internal/driver`, `internal/transform` in donor.
- Keep donor temporal/resource code as read-only reference until Phase 1 Postgres migration complete.

**Exit:** v1.0 tag. Donor execution path removed.

---

## Cross-cutting workstreams

### Observability
- OpenTelemetry traces across edge → runtime → control → orchestrator.
- Prometheus metrics (already wired in edge via `metrics-exporter-prometheus`).
- Trace ID propagation via `X-Request-Id` + W3C Traceparent.

### Security
- `cargo audit` + `cargo deny` in CI.
- `gosec` + `govulncheck` in CI.
- SSRF fuzz suite (runs nightly on security engine).
- HMAC on all cross-plane internal calls (Phase 4).

### Testing
- 80% coverage floor per crate/module.
- Testcontainers for Postgres + Temporal integration.
- E2E smoke via `deploy/scripts/smoke.sh` in CI.
- Property tests (proptest) for URL normalization, fingerprint stability, cache key derivation.

### Migration from donor
- No big-bang cutover.
- Traffic split via edge feature flag keyed on org.
- Parallel write to both stores for 1 week before full cutover.

### Documentation
- Every phase writes:
  - updated `CONTRACTS.md` diff note
  - module-level READMEs
  - migration notes in `docs/MIGRATION_FROM_DONOR.md`
  - changelog entry
- Maintain `docs/DONOR_MATRIX.md` or an equivalent section in `SCOREBOARD.md` showing V1/Firecrawl parity and intentional non-goals.

---

## Dependency map (phase order constraints)

```
Phase 0 ──▶ Phase 1 ──▶ Phase 3
                │
                └──▶ Phase 2 ──▶ Phase 4 ──▶ Phase 5 ──▶ Phase 6 ──▶ Phase 7 ──▶ Phase 8
```

Phase 2 and Phase 3 can parallelize once Phase 1's control parity lands. Phase 4 requires Phase 2 browser runtime. Phase 5 requires Phase 4 leases.
