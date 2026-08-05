# Quarry-v2 — OSS-Parity Implementation Plan

> Source backlog: `docs/OSS_PARITY_BACKLOG.md`. Consumer = **Verevon** frontend (no MCP).
> Style: compressed. Discipline: **TDD** (RED→GREEN→REFACTOR, ≥80% cov). Gate every PR through
> **rust-review** + `cargo clippy -D warnings`. Lenses: rust-patterns · golang-pro · api-design ·
> system-design · agent-harness · code-quality. **No code until user confirms this plan.**

## Invariants (must never regress)
- Org isolation: every store/query takes `&org_id`; client `org_id` overridden by JWT claim.
- ZDR: no durable writes/embeddings when `ZdrMode::on`.
- Default image stays slim → new heavy deps behind cargo features (mirror `postgres-queue`).
- Wire shapes snake_case; API uses the `{error,code,hint}` envelope already in `search_routes.rs`.
- Immutable data; many small files (<400 lines); typed errors (`QuarryResult`).

## Anchor points (real symbols verified)
- `serp.rs`: `trait SearchProvider { async fn search(q, &SearchOptions)->QuarryResult<Vec<SearchResult>> }`, `SearchResult`(:21), `FallbackSearchProvider`.
- `smart_router.rs`: `SmartSearchRouter`(:175) + builder, `impl SearchProvider`(:669) ← **RRF fuse here**.
- `local_index.rs`: `TantivyLocalIndex` (BM25, org_id facet).
- `mp_client.rs`: `ModelPlaneClient.invoke()` — **no embed() yet**.
- `search_routes.rs`: `SearchRequest{query,limit,country,language,safe_search,request_id}`, `SearchResponse`, `async fn search`(:55), error has `code`+`hint`.
- `crawl_frontier.rs`: `FrontierConfig.host_in_scope()`(:87), `enqueue()`(:165 checks scope :194).
- `chunks.rs` (transform): chunker for `format=context`/`chunks_per_source`.
- Go control `internal/resources/cycle23.go`: chi `Mount*` pattern.

---

## Phase 0 — Decisions + scaffolding (BLOCKS Phase 1C)  ·  S
RESOLVED (filesystem-confirmed): **Qdrant is the org vector standard** — Data Plane v2 runs
`dpv2-qdrant` (qdrant/qdrant v1.17, gRPC :6334, collection `dataplane_knowledge`); legacy Data
Plane runs qdrant v1.9. → **Use Qdrant, not pgvector.** Drop the pgvector/hybrid-store idea.
ARCHITECTURE FORK (needs confirm — §Open Decisions A1):
- **A) Delegate (recommended, plane-pure):** Quarry runs NO vector store. Hybrid = local Tantivy
  (lexical) RRF-fused with **Data Plane** vector retrieval over the existing `GrpcDataPlaneClient`
  (DP owns embed-on-ingest + query-embed + Qdrant). Honors Quarry's "evidence engine, not knowledge
  owner" rule; **removes the Model Plane embeddings blocker**; zero new infra.
- **B) Quarry-local Qdrant:** own collection `quarry_corpus` on a `quarry-qdrant` service; needs
  query+doc embeddings (Model Plane `/v1/ai/embed` or DP embed API). Faster/self-contained but
  duplicates DP capability + adds infra.
- D3 gate: `hybrid-search` cargo feature (default off), like `postgres-queue`.
Scaffold (both paths): `VectorIndex` trait (`knn(org,query,k)->Vec<Hit>`; impl `DataPlaneVectorIndex`
[A] or `QdrantVectorIndex` [B]) + `fusion.rs` (RRF, k=60). Tests: trait + RRF math first (RED).

---

## Phase 1 = P0 (Verevon-facing)

### 1A — Tavily search-param parity + RAG context  ·  M  ·  api-design, search-first
Files: `quarry-edge/search_routes.rs`, `quarry-core` SearchOptions, `transform/chunks.rs`, OpenAPI + TS SDK.
Add `SearchRequest` fields: `topic{general,news,finance}`, `time_range`/`days`, `exact_match`, `chunks_per_source`, `include_answer`, `format{results,context}`.
- `exact_match` → Tantivy phrase query; `topic` → reuse `intent_classifier`; `time_range` → filter by fetched_at.
- `format=context` → rank chunks (chunks.rs) + token-budget → single context string for Verevon RAG.
- `include_answer=true` → fuse `/v1/answer` (AnswerPipeline) inline.
TDD: unit per param (parse+apply); golden test for context token-budget; phrase-query test.
Deps: none. Quick wins: exact_match, time_range, include_answer flag.

### 1B — `/v1/map`  ·  S  ·  api-design  (QUICK WIN, do first)
New `quarry-edge/map_routes.rs`: `GET/POST /v1/map {url, search?, limit?, include_subdomains?}`.
Recombine existing: `transform::sitemap` + `robots` + link extract + `CrawlRanker::LexicalRanker` (rank by `search`).
Returns `{links:[{url,title?,score?}], count}`. Org-scoped, ZDR-safe (no writes).
TDD: sitemap-only fixture, ranked-by-search fixture, subdomain scope test.

### 1C — Hybrid search (Tantivy ⊕ vector via RRF)  ·  L  ·  system-design, search-first  (STRATEGIC)
Default = **Path A (delegate to Data Plane Qdrant)**:
1. `vector_index.rs`: `VectorIndex` trait `knn(org_id,query,k)->Vec<Hit>`; `DataPlaneVectorIndex`
   wraps the existing `GrpcDataPlaneClient` retrieval (query text in → ranked hits out; DP embeds + Qdrant). Behind `hybrid-search` feature.
2. `fusion.rs`: RRF merge `Vec<SearchResult>`(Tantivy lexical) ⊕ `Vec<Hit>`(DP vector) → fused ranking.
3. `smart_router.rs`: when corpus + DP retrieval enabled, run Tantivy ∥ DP-knn, RRF-fuse before SERP widen. Wire into `/v1/search` + AnswerPipeline retrieval.
4. Ingest already covered: PageRunner already calls IngestClient/GrpcDataPlaneClient → DP embeds on ingest. No new embed path in Quarry; ZDR already blocks ingest.
TDD: RRF unit (RED, P0); DP-retrieval mock (gRPC/wiremock); fused-beats-either; org-isolation knn; ZDR unaffected.
Path B (only if chosen): add `QdrantVectorIndex` (qdrant-client, `quarry_corpus`) + `embed()` on `mp_client.rs` + embed-on-PageRunner. Same trait, swap impl.
Deps: A1 fork. Risk (A): DP retrieval latency/coupling → cache by query+fingerprint; circuit-break to lexical-only on DP outage.

**Phase 1 exit gate:** `/v1/search` returns hybrid-ranked org-scoped results + context mode; Verevon can drive search/answer; default build unaffected (feature off); clippy clean; cov ≥80%.

---

## Phase 2 = P1

### 2A — `/v1/extract`  ·  L  ·  agent-harness, api-design — ✅ SHIPPED
`quarry-edge/extract_routes.rs`: `POST /v1/extract {urls:[..], schema?, prompt?, max_urls?}`.
Bounded fan-out (`expand_targets`: validate http(s) + dedup + cap, default 10 / max 25), per-source
isolation (one failure → per-item `error`, batch survives), typed `ExtractItem{url,status,data?,
markdown?,error?}` envelope. Each URL: driver fetch → readability markdown → `AiFormatRunner.json`
(schema) or markdown-only (no schema / no Model Plane). Org-metered, ZDR-safe. 2 helper tests + bin
green. Wildcard `domain/*` = `/v1/map` → `/v1/extract` two-step (documented; map already ships).

### 2B — Crawl glob include/exclude  ·  S  ·  rust-patterns  (QUICK WIN) — ✅ SHIPPED
`crawl_frontier.rs` already had `include_patterns`/`exclude_patterns` enforced in `enqueue()` but only
as **substring** match. Upgraded to **glob** (`*`/`?`, full-URL) via dep-free `pattern_matches` +
`glob_match` (linear two-pointer); non-glob patterns keep substring back-compat. Wired into both
exclude + include checks in `enqueue()`. 4 new glob tests + 19 existing frontier tests green.
Fields already serialize via `FrontierConfigSnapshot` (edge/Go plumbing is field pass-through).

### 2C — AutoscaledPool  ·  M  ·  rust-patterns, system-design — ✅ SHIPPED
`quarry-runtime/src/autoscale.rs`: `AutoscaledPool` — global concurrency cap over a resizable
`tokio::Semaphore`, sized from `available_parallelism` (`from_parallelism`: cores → cores×4),
AIMD (`record_ok` +1 to max, `record_overload` halve to min) via pure `next_target`. RAII
`acquire()` permits. Dep-free (no sysinfo; core-count + backpressure signals). Layers above per-host
`HostScheduler`. 6 tests. **WIRED** into `PageRunner` fetch path (`global_autoscale()` permit per fetch + AIMD by outcome).

### 2D — Fingerprint/session auto-rotation on block  ·  M  ·  rust-patterns — ✅ SHIPPED
`quarry-runtime/src/fingerprint_rotation.rs`: `is_block_status` (401/403/407/429/503),
`FingerprintRotator` (round-robin over impersonation profiles, bounded `max_rotations`, `on_block()`
→ next profile or None when exhausted), pure `next_index`. Dep-free, 4 tests. **WIRED** into
`FallbackDriver`: a block STATUS (403/429/503) now falls through to the next (different-fingerprint)
driver in the chain; exhausted rotation surfaces the block response. +2 driver rotation tests.

**CYCLE 2 (P1) COMPLETE** — 2A `/v1/extract`, 2B crawl globs, 2C AutoscaledPool, 2D fingerprint
rotation, 2E highlight/facets. All green, 0 warnings.

### 2E — Search highlighting + facets  ·  M  ·  search-first — ✅ SHIPPED
`search_routes.rs`: request `highlight`/`facets` flags; response gains `facets: [{value,count}]`
(host aggregations, desc) + `<mark>` term highlighting in titles/snippets (case-insensitive,
ASCII-safe byte handling, longest-term-first). Provider-agnostic (works on any SearchResult set).
Pure helpers `compute_host_facets`/`highlight_terms` + `wrap_ci`. 3 new tests. (date/content-type
facets deferred — need enriched results carrying those fields.)

---

## Phase 3 = P2

- **3A Index analyzers/synonyms** (S, search-first) — ✅ SHIPPED: `transform/src/synonyms.rs` —
  `SynonymMap` (+ `with_defaults` tech set, bidirectional) + `expand_query` (term → `term OR syn…`,
  case-preserving). 4 tests. (Tantivy snapshot/reindex/aliases = ops/filesystem follow-up.)
- **3B Agent memory + redaction** (M, agent-harness) — ✅ SHIPPED: `quarry-runtime/src/agent_memory.rs`
  — `AgentScratchpad` (bounded FIFO, redact-on-store, `render` for prompt) + `redact_sensitive`
  (emails / long secret tokens / card numbers; dep-free token classifier). 6 tests. (DOM-for-LLM
  serialization upgrade = follow-up.)
- **3C DOCX parsing** (S) — ✅ SHIPPED: `transform/src/docx.rs` — `extract_text(bytes)` unzips
  `word/document.xml` (added `zip` dep, deflate-only) + pure `strip_wordml` (runs→text, `</w:p>`/
  `<w:br>`→newline, entity unescape, blank-line collapse). 5 tests incl. in-memory zip round-trip.
- **3D Structured rate-limit envelope** (S, api-design) — ✅ SHIPPED: `quarry-edge/src/api_error.rs`
  `ApiError{error,code,hint?,window?,retry_after_seconds?,next_actions[]}` + `humanize_secs` +
  `rate_limited()` builder; wired into `/v1/search` 429 path. 3 tests. (Other routes adopt the
  envelope in follow-up.)
- **3E Benchmark comparison** (L, benchmark lens) — ✅ SHIPPED (scoring primitive): `quarry-core::
  benchmark::compare_score(candidate, baseline, MetricTarget, warn_pct, fail_pct) → ComparisonVerdict`
  (Pass/Warn/Fail), direction-aware (HigherBetter/LowerBetter/InRange), zero-baseline-safe, with
  `WARN_PCT`/`FAIL_PCT` defaults. 4 tests. Powers release + producer bake-off verdicts on top of the
  cycle-28/31 suites + `lab/evals` harness. (Live runs vs Firecrawl/Tavily/Trafilatura/Readability
  still need creds in the runner env — ops follow-up, as long-noted.)

**CYCLE 3 (P2) COMPLETE** — 3A synonyms, 3B agent memory+redaction, 3C DOCX, 3D rate-limit envelope,
3E benchmark comparison. All green, 0 warnings.

---

## ✅ PLAN COMPLETE — all cycles executed
P0 (1A/1B/1C) · P1 (2A–2E) · P2 (3A–3E) all shipped, each TDD with passing tests + zero warnings.
Remaining items are explicitly-noted follow-ups: **OpenAPI/TS-SDK regen** (blocked on the
Verevon↔Model-Plane↔Quarry topology decision), **onboarding→Data/Model-plane bridge** (real gap —
preview path doesn't yet ingest/enrich), Tantivy snapshot/aliases, DOM-for-LLM serialization, live
benchmark runs needing creds. (AutoscaledPool + fingerprint-rotation are now wired, not deferred.)

---

## Cross-cutting (every phase)
- **API/Verevon contract:** update `docs/openapi.yaml` + regen Python/TS SDKs (`sdks/generate.sh`) so Verevon has typed clients. golang-pro: any control-plane list views mirror `cycle23.go` Mount pattern.
- **context7-mcp:** pull current docs for `tantivy` (phrase/highlight/facet), `pgvector`/`qdrant`, `sysinfo` before coding each.
- **rust-review** gate + `cargo test --workspace` + clippy `-D warnings` per PR; Go `go test ./... -race`.
- Keep `gap-quarry.md` / backlog synced as items land.

## Risks
- HIGH: Model Plane embeddings endpoint may not exist → Phase 1C blocked on cross-plane work (D2).
- MED: embedding cost/latency on hot scrape path → async + fingerprint-keyed cache.
- MED: pgvector recall at scale → trait lets us swap qdrant later (multi-backend).
- LOW: feature-flag matrix growth → CI matrix `--features hybrid-search,postgres-queue,grpc`.

## Sequencing
P0: **1B → 1A → 1C** (quick wins build confidence; 1C after D1–D3).  
P1: 2B (quick) → 2E → 2A → 2C → 2D.  P2: opportunistic; 3E before any external parity claim.

## Decisions — ALL RESOLVED (locked 2026-05-30)
- Vector tech = **Qdrant** (org standard — Data Plane v2 already runs it). No pgvector.
- Hybrid-search arch = **Path A — delegate vector retrieval to the Data Plane** via the existing
  `GrpcDataPlaneClient`. Quarry runs NO vector store; no Model Plane embeddings dependency.
  `QdrantVectorIndex` (Path B, Quarry-local) deferred behind the same `VectorIndex` trait.
- Cycle 1 scope = **Full P0**: 1A (Tavily params + RAG context) + 1B (`/v1/map`) + 1C (hybrid).
- Build order (TDD, RED→GREEN→REFACTOR): **1B → 1A → 1C**. rust-review + clippy gate per step.

## Cycle-1 task list
- [x] 1B `/v1/map` — **SHIPPED** `quarry-edge/src/map_routes.rs` (sitemap+robots+links+LexicalRanker,
  SSRF guard via heur::check, org-scoped usage metering, ZDR-safe). 6 unit tests, 0 warnings.
  Wired `POST /v1/map` in routes.rs + lib.rs.
- [x] 1A search params — **SHIPPED**: `SearchOptions` gained `topic/time_range/exact_match`;
  `/v1/search` request gained `topic/time_range/days/exact_match/chunks_per_source/include_answer/
  format=context`; response gained optional `answer/citations/context`. Pure helpers
  `apply_exact_match`/`derive_time_range`/`build_context` (10 unit tests). include_answer fuses
  AnswerPipeline (best-effort); format=context emits token-bounded RAG string. 303 runtime + 75 edge tests green.
- [x] 1C hybrid — **SHIPPED** (Path A, zero new deps): `quarry-runtime/src/vector_index.rs`
  (`VectorIndex` trait + `DataPlaneVectorIndex` HTTP client to DP `retrieval_v2` `/v1/retrieve` +
  `NoopVectorIndex`), `fusion.rs` (RRF, k=60), `hybrid.rs` (`HybridSearchProvider` — runs lexical ∥
  DP-vector concurrently, RRF-fuses, circuit-breaks to lexical-only on DP failure). Wired in
  `main.rs`: wraps the SmartSearchRouter when `data_plane_url` is set → transparent to /v1/search +
  AnswerPipeline. 12 unit tests (RRF math, DP mapping, wiremock retrieve, degrade-on-error).
- [~] Cross-cutting — docs synced; **OpenAPI + TS SDK regen for Verevon still TODO** (run after P0/P1
  surface settles); no `hybrid-search` feature needed (Path A is dep-free, gated by `data_plane_url`).

**CYCLE 1 (Full P0) COMPLETE** — 624 workspace lib tests pass, 0 failures, 0 warnings (default +
--all-features). Edge binary builds clean. → proceeding to P1.

**STATUS: plan confirmed. Ready to start cycle 1 at 1B on user go-ahead.**
