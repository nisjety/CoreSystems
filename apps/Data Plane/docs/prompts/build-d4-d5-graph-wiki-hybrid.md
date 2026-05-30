# Build prompt — Data Plane D4 + D5 (Graph + Wiki + Hybrid retrieval)

> **You are picking up Wave 11.C of the CoreSystem effort.** This brief is self-contained — read it cold, deliver the listed services, no clarifying questions to product. Architecture details live in the companion spec. Use them; don't redesign.

---

## 0 · Read these three documents first (in order)

1. `apps/master-ownership-matrix.md` — the constitutional plane rules. Memorize §0 "Decision Rules".
2. `apps/Data Plane/docs/gap-data.md` — target architecture; the source of truth for what each service owns. Read §5.1–5.4, §6, §8 Phase D4 + D5, §9 API surface.
3. `apps/Data Plane/docs/d4-d5-graph-wiki-hybrid-spec.md` — concrete schemas, endpoint shapes, and build order. **Treat this as the contract.**

If anything in this prompt contradicts those documents, those documents win.

---

## 1 · What you are building

Two new Data Plane services + extensions to three existing ones, in parallel with a velion UI track that wires against your OpenAPI from day one.

| Service | Lives at | Language | New / Extend |
|---|---|---|---|
| `graph-index-rs` | `apps/Data Plane/services/graph-index-rs/` | Rust 1.83+ (axum 0.7 + sqlx 0.8 + petgraph 0.6 + tree-sitter 0.22) | **NEW** |
| `wiki-store-go` | `apps/Data Plane/services/wiki-store-go/` | Go 1.23+ (chi v5 + pgx v5 + sqlc) | **NEW** |
| `retrieval-engine-rs` | `apps/Data Plane/services/retrieval/` | existing Rust | EXTEND — add `/v1/retrieve/hybrid`, `/v1/knowledge/search`, blending logic |
| `index-engine-rs` | `apps/Data Plane/services/index-engine-rs/` | existing Rust | EXTEND — emit entity/edge proposals during chunking |
| `data-orchestrator-go` | `apps/Data Plane/services/data-orchestrator-go/` | existing Go | EXTEND — graph rebuild jobs, wiki maintenance sweeps |

---

## 2 · Hard rules (non-negotiable)

1. **No mocks, ever.** Velion's UI track is reading your OpenAPI on day one. Endpoints that have no data yet return real-shaped empty responses (`200 { items: [] }`), never fixtures or `TODO` placeholders.
2. **Plane purity.** You write Postgres, Qdrant, object storage. You do NOT call Quarry. You do NOT call model-gateway except through `index-engine-rs::llm_light` for entity extraction. You do NOT touch Convex. If you feel tempted to cross a plane, re-read §0 of `master-ownership-matrix.md`.
3. **Org-scoped everything.** Every query has `WHERE org_id = $1`. Every endpoint validates `X-Org-ID` against the JWT. Postgres RLS turned on after D4-7.
4. **JWT mandatory.** auth-core JWKS, RS256. Implement once in shared middleware. No bypass except `make dev-loopback`.
5. **OpenTelemetry mandatory.** Every endpoint emits an OTel span with attributes `org_id`, `agent_id?`, `trace_id`, and (for retrieval) `mode_mix`. Use `traceparent` for service-to-service propagation.
6. **Idempotent writes.** Re-running a write with identical inputs is a no-op, not a duplicate row. Use `ON CONFLICT DO NOTHING` or content-hash dedup.
7. **No new languages.** Rust for everything latency- or CPU-bound, Go for the document/wiki APIs and orchestration. No Python in the request path; Python only for `retrieval-eval-py` offline regression.
8. **Test as you go.** Each phase below has a "Done-when" gate. You don't move to the next phase until that gate is green.

---

## 3 · Deliverables checklist (work top-to-bottom)

> Each item maps 1-1 with §9 "Build order" in `d4-d5-graph-wiki-hybrid-spec.md`. Hit them in order; D4-3/D4-4/D4-5 can fan out to parallel sub-agents if you have them.

### D4 — Graph + Wiki foundation (~7 days)

- [ ] **D4-1 · Contract freeze (0.5d)**
  - Author `apps/Data Plane/proto/d4d5.proto` (services `GraphIndex`, `WikiStore`).
  - Author `apps/Data Plane/openapi/d4d5.yaml` covering every endpoint in §3 of the spec.
  - Run `pnpm openapi-typescript apps/Data Plane/openapi/d4d5.yaml -o apps/Frontend Plane/velion/src/types/data-plane/d4d5.ts` so velion gets types immediately.
  - Commit + tag `data-plane-d4-contract-v1`. **DONE-WHEN**: `tsc --noEmit` clean in velion against the generated types.

- [ ] **D4-2 · Postgres migrations (0.5d)**
  - Write `apps/Data Plane/migrations/0010_graph_tables.sql` and `0011_wiki_tables.sql` exactly matching §2.1 of the spec.
  - Include `DOWN` migrations. Use `sqlx migrate` semantics.
  - **DONE-WHEN**: `make migrate-up && make migrate-down && make migrate-up` is green on a fresh Postgres 16 container.

- [ ] **D4-3 · `wiki-store-go` skeleton (2d)**
  - Boot a chi router with: middleware (otel, jwt, org-scope), pgx pool, structured logger (zerolog), graceful shutdown.
  - Implement every §3.2 endpoint as a real handler against the new tables (no `TODO` returns).
  - sqlc-generate the query layer; do NOT hand-write SQL in handlers.
  - **DONE-WHEN**: integration test creates a page → fetches it → fetches backlinks → contradiction sweep emits zero rows on a clean DB; all 200/201.

- [ ] **D4-4 · `graph-index-rs` skeleton (2d)**
  - axum router with the same middleware shape as wiki-store-go (otel, jwt, org-scope).
  - sqlx async repo over the graph tables.
  - Implement every §3.1 endpoint, plus the gRPC server side of `GraphIndex` (tonic).
  - **DONE-WHEN**: `cargo test -p graph-index-rs` green; `GET /v1/graphs/{org_id}` returns `{nodes: [], edges: []}` 200.

- [ ] **D4-5 · Entity-extraction hook in `index-engine-rs` (2d)**
  - Add `extractor/` module. Code paths first: `tree-sitter-{rust,go,typescript,python,java,javascript}` AST walks → typed declaration + import edges. Markdown paths next: `tree-sitter-markdown` headings + link parser.
  - For free-text chunks: a `LightLLMExtractor` trait. Default impl calls model-gateway `/v1/invoke` with structured output (provide a Zod-style schema in the prompt). Cap to 200 chunks/min throttle; results land with `provenance='inferred'` and `confidence=0.6`.
  - Emit edge proposals via tonic gRPC `GraphIndex.UpsertEdges`. Batch in groups of 100.
  - **DONE-WHEN**: ingest a real markdown doc → query `GET /v1/graphs/{org_id}` → see at least one node and one edge.

- [ ] **D4-6 · Wiki Logseq-format read/write (1.5d)**
  - Block-outline parser: walks `- ` / `\t- ` outline markdown into a typed tree (`Block { id, depth, content, children }`). Block IDs are content-hashed (BLAKE3) so re-saves don't churn IDs.
  - Serializer: typed tree → Logseq-format markdown round-trip identical.
  - `[[page link]]` and `#tag` extraction → upsert into `wiki_backlinks` via a Postgres trigger on `wiki_page_versions` insert.
  - Version chaining: new version = current_version + 1; previous remains addressable.
  - **DONE-WHEN**: `POST /v1/wiki/pages` with body referencing `[[Other Page]]` → `GET /v1/wiki/backlinks/{otherPageId}` shows the link; round-trip parses bit-identical markdown.

- [ ] **D4-7 · Graph rebuild job + community pass (1d)**
  - `data-orchestrator-go` worker: subscribes to NATS `doc.ingested`; debounces 30s; calls `POST /v1/graphs/build` internally.
  - Inside `graph-index-rs`: Leiden community detection over the org's full edge set using `petgraph` + a small Leiden impl (Traag 2019). Persist `community_id` on nodes; persist a `graph_rebuild_jobs` row with stats.
  - Recompute only when ≥ 5% of edges changed since last run.
  - **DONE-WHEN**: `POST /v1/graphs/build` with `{scope:{full:true}}` on a populated DB completes; community IDs visible on nodes.

### D5 — Hybrid retrieval + GraphRAG (~7 days)

- [ ] **D5-1 · `wiki_block_embeddings` Qdrant collection (0.5d)**
  - Create collection lazily on first write; vector size matches existing chunk embedding model.
  - Write-through hook in wiki-store-go: when a version's status flips to `published`, embed each block via `embedding-engine-rs` and upsert with payload `{page_id, version, block_path, org_id}`.
  - **DONE-WHEN**: publish a page → wait 5s → Qdrant `/collections/wiki_block_embeddings/points/count` reflects the block count.

- [ ] **D5-2 · `entity_summary_embeddings` Qdrant collection (0.5d)**
  - Nightly job in `data-orchestrator-go`: for each community changed today, summarize via model-gateway, embed via embedding-engine-rs, upsert.
  - Payload: `{node_id, org_id, kind, community_id}`.
  - **DONE-WHEN**: a community appears in Qdrant within one job cycle.

- [ ] **D5-3 · `/v1/retrieve/graph` (1d)**
  - In `graph-index-rs`: parse `query` → entity-extract via embedding-engine-rs (`extract_entities(query)` — small NER pass) → anchor those entities into existing nodes (case-insensitive label match + canonical-uri).
  - Expand BFS hops ≤ `hops`. Score: `sum(edge.weight * edge.confidence * decay(hop))`. Cap nodes at `k`.
  - Return ranked node IDs + subgraph + community summaries (top 3 by ranked nodes).
  - **DONE-WHEN**: a query like `"What does Alice work on?"` returns Alice + her direct neighbors.

- [ ] **D5-4 · `/v1/retrieve/wiki` (0.5d)**
  - In wiki-store-go: ANN query against `wiki_block_embeddings` + boost by backlink count (`log(1 + count)`).
  - Returns page + matched-block list with scores.
  - **DONE-WHEN**: returns ranked page hits for a real query.

- [ ] **D5-5 · `/v1/retrieve/hybrid` (1.5d)**
  - In `retrieval-engine-rs`: implement the §7 blending formula.
  - Dense via existing Qdrant chunks; bm25 via Postgres `tsvector` + `pg_trgm`; graph via internal gRPC to graph-index-rs; wiki via internal HTTP to wiki-store-go.
  - Rerank: when `rerank=true`, take top 50 union → cross-encoder (`bge-reranker-v2-m3` hosted by embedding-engine-rs) → narrow to k.
  - Persist a `retrieval_trace` row per call (existing table).
  - **DONE-WHEN**: a known query returns blended items with `modeMix` populated; `/v1/retrieval/{trace_id}` returns the trace.

- [ ] **D5-6 · `/v1/retrieve/contradictions` (1d)**
  - Detector: for a `topic`, fetch top-k wiki blocks + graph subgraph for that topic; cluster by claim (model-gateway structured-output call: `extract_claim(block) -> {subject, predicate, object}`); flag pairs with same `(subject, predicate)` but conflicting `object`.
  - Persist into `wiki_maintenance_log` with `kind='contradiction'`.
  - **DONE-WHEN**: seed two contradictory pages → endpoint returns one conflict row with sources.

- [ ] **D5-7 · `/v1/knowledge/search` (0.5d)**
  - Envelope around `/v1/retrieve/hybrid` with default weights and `kinds` filter (`docs | wiki | nodes`). Used by velion's global search bar.
  - **DONE-WHEN**: a single GET returns mixed results across kinds.

- [ ] **D5-8 · Retrieval trace audit + Grafana (0.5d)**
  - `GET /v1/retrieval/{trace_id}` returns the full stored trace (query, weights, mode_mix, per-source scores, time per stage).
  - Grafana dashboard panels: p50/p95/p99 hybrid latency, mode-mix histogram, rerank impact (Recall@k delta).
  - **DONE-WHEN**: dashboard renders against staging traffic.

- [ ] **D5-9 · Eval harness `retrieval-eval-py` (1d)**
  - 100-question golden set (seed file `apps/Data Plane/eval/d4d5_golden.jsonl`).
  - Metrics: nDCG@10, Recall@10, MRR@10. Also: rerank uplift.
  - Run on `make eval`. Output a markdown report.
  - **DONE-WHEN**: first run completes; baseline numbers committed.

- [ ] **D5-10 · Release gates (0.5d)**
  - All gap-data.md §11 gates pass: p95 hybrid < 800ms on the golden set; zero plane-mixing in CI audit (grep CI rule: no `qdrant_client` import outside Data Plane; no Postgres knowledge-table writes from any other plane).
  - Tag release `data-plane-d4d5-v1`.

---

## 4 · Style / engineering rules

- **Small files.** 200–400 lines typical, 800 max. Split by concern (`routes/`, `repo/`, `domain/`, `errors.rs`/`errors.go`).
- **Immutability.** Construct new structs; never mutate inputs. Borrow rules in Rust enforce this; in Go use `WithX` builders.
- **Errors at boundaries.** Every public handler returns RFC-7807 problem+json; convert internal `Result`/`error` exactly once at the HTTP edge.
- **No `unwrap()` / `panic!()` in request paths.** Tests are fine.
- **Validate at the boundary.** Use `validator` (Rust) / `go-playground/validator` (Go) on every body and query parameter. Reject on parse, don't sanitize.
- **No swallowed errors.** Log with structured context (`org_id`, `trace_id`).
- **No hardcoded secrets.** Env-var the JWKS URL, Qdrant URL, DB URL, NATS URL, model-gateway URL. Defaults work in `make dev-loopback`.

---

## 5 · Local dev story

The build target is `make d4d5-up` (add to root `Makefile`). It must spin:

- Postgres 16 (existing compose file)
- Qdrant (existing compose file)
- NATS (existing)
- `graph-index-rs` on `:8201`
- `wiki-store-go` on `:8202`
- Extended `retrieval-engine-rs` on existing port
- Extended `data-orchestrator-go` on existing port

A `make d4d5-seed` target loads:
- 3 markdown docs into documents-api-go
- 1 codebase tarball
- Triggers graph build + wiki sync
- Asserts non-empty results from each `/v1/retrieve/*`

The velion side will hit these directly — no proxy, no mock. Use ngrok or direct localhost.

---

## 6 · Definition of done (the whole wave)

You're done when ALL of the following are true:

1. Every endpoint in §3 of the spec responds with the documented shape.
2. `make eval` shows non-zero nDCG/Recall/MRR against the golden set.
3. `cargo audit` and `go vet ./...` clean on both new services.
4. CI plane-purity rule passes (no cross-plane imports).
5. `apps/Frontend Plane/velion/src/types/data-plane/d4d5.ts` compiles against the live OpenAPI without diff.
6. The Grafana dashboard shows real traffic from `make d4d5-seed`.
7. Spec doc + this prompt + a status appendix are linked from the gap-data.md "## 13 · Wave-completion log" table.

---

## 7 · When in doubt

- Architecture question → re-read `d4-d5-graph-wiki-hybrid-spec.md`.
- Ownership question → re-read `master-ownership-matrix.md` §0.
- Behaviour question → re-read `gap-data.md` §6 (Agentic Context Engine).
- Wiki format question → re-read `Quarry-v2/docs/LLM-Wiki.md`.

If the answer isn't in those four documents, write up the gap as `apps/Data Plane/docs/d4d5-open-questions.md` and ping the spec owner before guessing.

Go build it.
