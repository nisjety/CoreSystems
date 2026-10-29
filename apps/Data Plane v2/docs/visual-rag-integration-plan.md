# Visual RAG in Data Plane v2 — Build Plan & Contract

> Status: in implementation (PR-A landed 2026-06-22). Grounded in a code-level map
> of the live DP2 stack + 2026 research on Azure AI Foundry embeddings, ColPali,
> GraphRAG, and chunking. This doc is the single source of truth for the build.
>
> **Verified 2026-07-10** (re-audit, live checks against running containers + `cargo test`):
> PR-F (CAS + page-image renderer/producer) is done and live end-to-end — it landed in
> **Ingestion Plane's `Quarry-v2`** (`quarry-runtime/src/page_renderer.rs` + `page_image.rs`),
> not `documents-api-go` as this doc originally said; `dataplane_page_images` is a real Qdrant
> collection (1536-dim cosine, confirmed live with indexed points), fed via the
> `dataplane.page_images.created`/`.deleted` JetStream subjects exactly as specified below.
> A **ColQwen visual reranker** (`retrieval-engine-rs/src/search/colqwen.rs` +
> `services/colqwen-reranker` Python/GPU service) has also been built since this doc was
> written — it is real code, gated off by default (`VISUAL_RERANK_ENABLED=false`,
> `W_VISUAL=0`), and is not otherwise described here. See the corrected sections below;
> the rest of this doc's architecture, weights, and contracts still match the live code.

## TL;DR (revised after the Azure-Foundry research)

1. **Visual arm = Cohere Embed v4 (`Cohere-embed-4`) on Azure AI Foundry**, not
   self-hosted ColPali. Embed v4 is **multimodal** (text + document-page images +
   fused) and produces a **single 1536-dim vector** per page — so it drops into the
   existing single-vector cosine Qdrant pattern with **no multivector / MAX_SIM /
   GPU work**. ColPali/ColQwen2 score a few points higher on dense-layout ViDoRe but
   cost multi-vector storage + a self-managed GPU fleet — kept as a **future flagged
   accuracy tier**, not the v1.
2. **Keep `text-embedding-3-large` (3072) for the text arm.** Run a **combination**:
   two Qdrant collections (text 3072, visual 1536), fused at query time with RRF. Do
   NOT unify — re-embedding the whole text corpus buys nothing.
3. **RAG/visual/GraphRAG are query-time strategies, not storage stages.** Index once
   (chunk+embed text, render+embed page images, build graph, build wiki); retrieve
   per query (dense + sparse + visual + graph → RRF → rerank). See "Conceptual model".
4. **Chunking:** layout-aware (Azure Document Intelligence Layout → Markdown),
   ~500 tokens / ~75 overlap, tables kept whole; recursive 500/75 fallback. Layer
   **Anthropic Contextual Retrieval** (LLM context prefix + contextual BM25) on
   high-value corpora — the single biggest recall lever (−49% retrieval failures).
5. **The real blocker is infra you don't have:** DP2 retains no raw bytes (only
   extracted text in Postgres). A visual arm needs the rendered page image, so a
   **content-addressable raw store** (reuse the MinIO already running for Quickwit)
   is the first non-Rust lift.

## Conceptual model (why the original two sequences failed)

Both proposed sequences described a *pipeline of transforms on retrieved data*
(`rag → graphrag the raged data → store in redis vector → Qdrant`). That conflates
two layers:

- **Index-time** (once per doc version): chunk+embed text, render+embed page images,
  extract the graph, generate the wiki. Produces durable artifacts.
- **Query-time** (per query): dense + sparse + visual + graph retrieval → RRF → rerank.
  Produces an ephemeral, query-dependent result. There is no "raged data" to store.

GraphRAG is built from the **whole corpus** at index time (entity/relation extraction
+ Leiden communities + summaries); you cannot manufacture it from the chunks one query
returned. TTL belongs to the **cache**, not the vectors (which live until their doc
version is superseded/deleted).

## Real stack (mapped from code)

| Layer | Technology (DP2 today) |
|---|---|
| Raw/original bytes | **none** — only `documents.content` TEXT in Postgres. MinIO exists *only* as Quickwit's S3 segment backend. |
| Text vectors | Qdrant `dataplane_knowledge`, single-vector cosine, **3072** (`text-embedding-3-large` via Model Plane inference-core / Azure OpenAI). |
| Sparse / BM25 | Quickwit (`dataplane-corpus`) + Postgres `ts_rank_cd` fallback (default backend = Postgres). |
| Graph | Postgres (`graph_entities/relationships/text_units/communities`); Azure-OpenAI extraction at ingest. **Not** in the main fused path — separate `/v1/retrieve/graph`. |
| Wiki | Postgres (`wiki_pages`/`wiki_page_versions`) + Qdrant ANN `wiki_block_embeddings` (fused via `w_wiki`). |
| Fusion | RRF (`fusion.rs`, k=60), applied twice → live path = **dense+sparse+wiki**. Then Cohere `rerank-english-v3.0`. Weights `w_dense=0.5, w_bm25=0.2, w_graph=0.2, w_wiki=0.1`. |
| Cache | Dragonfly/Redis **KV only** (no RediSearch) + Qdrant `semantic_response_cache` (semantic tier, gated off). |

The **wiki ANN arm is the template** for the visual arm: a dedicated Qdrant
collection, populated by a durable NATS consumer, folded into RRF by its own weight.

## Locked decisions & build contract

**Models / dims**
- Text: `text-embedding-3-large`, **3072**, collection `dataplane_knowledge` (unchanged).
- Visual: **`Cohere-embed-4`** via Azure AI Foundry images/embeddings, **1536** (Matryoshka;
  can truncate to 1024 later without re-calling), collection **`dataplane_page_images`**
  (single-vector cosine).
- Rerank: `rerank-english-v3.0` (unchanged), now over the union incl. visual candidates.

**Cohere Embed v4 REST (mirror the existing direct-Azure provider)**
```
POST {COHERE_EMBED_V4_ENDPOINT}/models/images/embeddings?api-version=2024-05-01-preview
api-key: {COHERE_EMBED_V4_API_KEY}
{ "model": "Cohere-embed-4",
  "input": [ { "image": "data:image/png;base64,…", "text": "<optional page title>" } ],
  "input_type": "document",          // "query" at search time
  "output_dimension": 1536 }
→ { "data": [ { "index": 0, "embedding": [ … ] } ], "usage": {…} }
```
Thread `zdr=false` through this hop exactly as the existing `ai_embeddings` path does
(new content-persisting boundary). Deploy serverless via `az ml marketplace-subscription`
+ `az ml serverless-endpoint` (Foundry portal serverless deploy unsupported for Cohere).

**New env (defaults)**
- Retrieval: `W_VISUAL=0.0` (shadow until eval), `QDRANT_VISUAL_COLLECTION=dataplane_page_images`,
  `VISUAL_EMBEDDING_DIM=1536`, `SEMANTIC_CACHE_REQUIRE_SCOPE=true` *(landed in PR-A)*.
- Embed v4 (embedding-engine + retrieval query path): `COHERE_EMBED_V4_ENDPOINT`,
  `COHERE_EMBED_V4_API_KEY`, `COHERE_EMBED_V4_DEPLOYMENT=Cohere-embed-4`,
  `COHERE_EMBED_V4_DIM=1536`, `COHERE_EMBED_V4_API_VERSION=2024-05-01-preview`.
- Chunking (index-engine): `CHUNK_SIZE` (default 512), `CHUNK_OVERLAP` (default 64) —
  **already implemented**; set ~500 / ~75 per the research if desired. DI-layout +
  Contextual-Retrieval flags (`CHUNK_STRATEGY`, `CONTEXTUAL_RETRIEVAL_ENABLED`,
  `DOCINTEL_ENDPOINT/KEY`) land when those strategies are built (deferred — phased).

**NATS subjects (new) — the contract is the CONSUMER CODE (`image_consumer.rs`), not this doc**
- `dataplane.page_images.created` — JSON on **JetStream** (stream `DATAPLANE_PAGE_IMAGES`).
  REQUIRED (no default; missing ⇒ poison-drop): `document_id:String`, `org_id:String`,
  `page_no:i64`, **`image_url:String`** (a fetchable URL the consumer HTTP-GETs — NOT a CAS
  key). OPTIONAL (`#[serde(default)]`): `content_hash:String`, `title:String|null`, `zdr:bool`.
  ⚠️ An earlier draft of this doc said `image_cas_key` — WRONG: the consumer has no such
  field, so serde would leave `image_url` empty → required-field-missing → **silent
  poison-drop** (the consumer acks poison, no DLQ). Producers MUST emit `image_url`.
- `dataplane.page_images.deleted` — `{document_id:String}` only. Purges Qdrant vectors; does
  NOT yet purge the CAS (Phase-2 erasure gap — see phased plan).

**Qdrant collections after this work:** `dataplane_knowledge` (3072) · `wiki_block_embeddings`
· **`dataplane_page_images` (1536, new)** · `entity_summary_embeddings` · `semantic_response_cache`.

## Recommended architecture

### INDEX-TIME (once per document *version*; re-run only when content hash changes)
1. **Ingest → canonical raw copy** in a content-addressable store (hash = key); reuse the
   Quickwit MinIO. System-of-record + dedup key + re-embed source. *(net-new infra; first lift)*
2. **Render page images** (rasterize) → CAS.
3. **Text path** (mostly unchanged, chunking upgraded — see below): extract/OCR → layout-aware
   chunk → `text-embedding-3-large` (3072) → Qdrant `dataplane_knowledge`; sparse → Quickwit.
4. **Visual path (NEW):** for each page image call **Cohere Embed v4** (`input_type:"document"`,
   optionally fuse page title/caption text) → **single 1536-dim vector** → Qdrant
   `dataplane_page_images`. No image chunking — Embed v4 fuses layout+text per page.
5. **Graph** (unchanged): corpus-level entity/relation/community extraction → Postgres.
6. **Wiki** (unchanged): generate, loop back through 1/3/4 as first-class docs.

### QUERY-TIME (per query)
1. **Semantic-cache check** (Qdrant `semantic_response_cache`, authz-scoped — PR-A): hit → stop.
2. **Route:** factoid vs holistic, text-native vs visual; only spin up expensive arms (visual,
   graph) when warranted — selective routing keeps Embed v4 cost honest.
3. **Parallel retrieve:** dense · sparse · **visual** (embed the query via Embed v4
   `input_type:"query"`, search `dataplane_page_images`) · **graph** (read prebuilt summaries).
4. **RRF fuse** (rank-based, scale-agnostic — the only sound way to merge cosine arms).
5. **Rerank** the union (Cohere; page-image candidates carry their CAS image ref).
6. **Assemble → generate**, citing the canonical CAS copy.
7. **Write-through** query→result into the semantic cache (authz-scoped, TTL).

## Store roles

- **MinIO/CAS** — canonical raw bytes + rendered page images (the "unchanged version").
- **Postgres** — canonical text, chunks, graph, wiki, ownership/visibility gate.
- **Qdrant** — all durable vectors: text dense (3072), wiki ANN, **visual (1536 single-vector)**.
  One SoR, multiple collections; lives until the version is superseded/deleted.
- **Redis/Dragonfly** — exact-match KV + hot payloads only. Never a vector store here.

## Fusion & weights

Add `w_visual` and finally fold **graph** into the main fused path → 5-way RRF
(`dense + sparse + wiki + graph + visual`). Roll out weight-gated: ship `W_VISUAL=0`
(shadow), eval recall on a visually-rich set (tables/charts/scans), then raise. Target
blend once proven: `dense 0.4, bm25 0.15, visual 0.2, wiki 0.1, graph 0.15` — tune per
`agent_retrieval_configs`.

## Chunking strategy (optimized)

| Strategy | Benefit | Index cost | Use |
|---|---|---|---|
| Layout-aware (DI Layout → Markdown) | High on PDF/office/scanned; real section/table boundaries | per-doc DI call | **Default** for structured docs |
| Recursive fixed 500/75 | Baseline; overlap avoids idea-splitting | none | Fallback for unstructured/web prose |
| Anthropic Contextual Retrieval | **−49% retrieval failures** (w/ contextual BM25) | 1 LLM call/chunk, ~$1/1M tok w/ prompt caching | High-value corpora (flag on) |
| Late chunking (Jina) | Context-aware chunks, no per-chunk LLM | needs long-ctx model | Cheaper alt to Contextual Retrieval |

**Default:** DI layout-aware Markdown, **~500 tokens / ~75 (15%) overlap**, tables kept whole,
token-based (not char) splitting; recursive 500/75 when no structure. Layer Contextual
Retrieval on high-value sets (`CONTEXTUAL_RETRIEVAL_ENABLED`). For visually-rich docs: embed
the **full rendered page** with Embed v4 (no image chunking) *alongside* the layout-chunked
text — the page lives in both collections, recall comes from RRF.

## PR sequence & status

- **PR-A ✅ Authz-safe semantic cache** — `scope_key` threaded into the cache point-id +
  Qdrant filter; fail-closed when absent under `SEMANTIC_CACHE_REQUIRE_SCOPE` (default true).
  `cache/semantic.rs`, `config.rs`, `api/mod.rs` + tests. *Gateway must pass the per-user
  visible-set hash / `org-shared` (Model Plane follow-up).*
- **PR-B ✅ Embed v4 multimodal provider** — `embedding-engine-rs/provider/visual.rs`
  (real Foundry `/images/embeddings` client, ZDR egress guard, contract-pinned tests) +
  visual config fields. Compiles + tests pass.
- **PR-D ✅ Page-image consumer** — `embedding-engine-rs/image_consumer.rs` (durable
  JetStream consumer on `dataplane.page_images.created/.deleted`, fetches the rendered page,
  embeds via Embed v4, upserts to `dataplane_page_images`; ZDR pages dropped+acked) + boot
  wiring in `main.rs` + `base64` dep. Compiles + tests pass.
- **PR-C ✅ Visual retrieval arm + fusion** — `retrieval-engine-rs/embed/visual.rs`
  (Embed v4 query embedder, `input_type=query`), `w_visual` in `pipeline/types.rs` +
  `config.rs`, visual RRF pass in `orchestrator.rs`, pipeline wiring in `main.rs`. Compiles +
  41 lib tests pass. *(Graph stays a dedicated endpoint for now — its entity/claim shape
  differs from chunk candidates; folding it into the fused list is tracked separately, not
  faked.)*

  **Verification (real `cargo test`, exit-code-checked — NOT `--lib` on a binary crate,
  which silently no-ops):** retrieval-engine 41/41 lib tests (re-verified 2026-07-10: now
  44/44 — the +3 are ColQwen-reranker tests added after this PR); embedding-engine 15+2
  (re-verified: now 16 lib + 2 integration, +1 from a later test) (Embed v4 provider +
  page-image consumer + ZDR guards); index-engine 7/7 chunker (re-verified: still 7/7);
  semantic-cache authz incl. fail-closed scope tests. PR-F (page-image producer + CAS) needs
  the live stack to verify end-to-end — **done as of 2026-07-10, see PR-F below.**
- **PR-E ✅ Chunking — structural pass** — `index-engine-rs/chunker`: recursive splitter
  now keeps markdown **tables and fenced code atomic** (never scattered across chunks),
  recursively sentence-splits oversized prose, greedy-packs with token overlap. API
  unchanged (zero caller churn); 7/7 chunker tests pass. Tuning via existing `CHUNK_SIZE`/
  `CHUNK_OVERLAP`. DI-layout + Contextual Retrieval remain the next flagged additions
  (deferred per the phased decision until a DI resource + per-chunk LLM budget are approved).
- **PR-F ✅ Infra (Ingestion Plane, not `documents-api-go` as originally planned)** — CAS
  raw+image store, page-image renderer, and the `dataplane.page_images.created`/`.deleted`
  producer landed in **`Quarry-v2`** (`crates/quarry-runtime/src/page_renderer.rs` +
  `page_image.rs`), publishing on the `DATAPLANE_PAGE_IMAGES` JetStream stream to the same
  broker `embedding-engine-rs/image_consumer.rs` binds. Verified 2026-07-10 end-to-end: the
  live `dataplane_page_images` Qdrant collection (1536-dim cosine) holds real indexed points,
  not just an empty schema. No dedicated Postgres migration was needed for image-ref rows.

## Infra gaps to close, in order

1. ~~**Canonical raw/object store** (reuse Quickwit MinIO) + page-image renderer/producer in
   Ingestion Plane / `documents-api-go`. Biggest lift; everything visual depends on it.~~
   **Closed as of 2026-07-10** — landed in Ingestion Plane's `Quarry-v2` (not
   `documents-api-go`), not DP2 at all; see the PR-F note above.
2. **Embed v4 provider** in `embedding-engine-rs/src/provider/mod.rs` (current providers are
   text-only) + the retrieval-side query embedder. ✅ Done (`provider/visual.rs`,
   `embed/visual.rs`).
3. **Document Intelligence layout** integration for chunking (`index-engine-rs`). Still open
   as of 2026-07-10 — `CHUNK_STRATEGY`/`DOCINTEL_ENDPOINT` flags are not implemented; only the
   recursive/structural chunker (PR-E) exists.
4. *(No multivector Qdrant generalization needed — Embed v4 is single-vector. This removes the
   biggest code risk from the original ColPali plan.)*
5. **Not in the original plan, built since:** a ColQwen visual reranker
   (`retrieval-engine-rs/src/search/colqwen.rs` + a new `services/colqwen-reranker` Python/GPU
   service) reorders Embed v4's visual top-K by late-interaction MaxSim score. Off by default
   (`VISUAL_RERANK_ENABLED=false`); not deployed as a compose service today.

## Files to change

- `embedding-engine-rs`: `provider/mod.rs` (Embed v4 image+text), `config.rs`, `main.rs`
  (ensure `dataplane_page_images`), new `image_consumer.rs` (+ `stream/mod.rs`),
  `qdrant_writer/mod.rs`.
- `retrieval-engine-rs`: `embed/visual.rs` (corrected 2026-07-10 — the actual landed path;
  this section originally said `search/visual.rs`), `config.rs` (visual fields — `embed/mod.rs`
  query path for Embed v4), `pipeline/types.rs` (`ModeMixWeights`, `EngineRoute`, metrics),
  `pipeline/orchestrator.rs` (visual route + extra RRF pass + fold graph in — graph fold-in is
  still not done, confirmed 2026-07-10: graph stays a dedicated `/v1/retrieve/graph` endpoint).
  `cache/semantic.rs` + `api/mod.rs` ✅ (PR-A). Also `search/colqwen.rs` (reranker, not in the
  original plan — see PR-F infra-gaps note above).
- `index-engine-rs`: `chunker/mod.rs`, `builder/mod.rs` (DI layout + Contextual Retrieval).
- Ingestion Plane / `documents-api-go`: page-image renderer + CAS write + producer.
- `infra/postgres/migrations/`: image-ref/metadata rows (additive, nullable, expand-contract).

## Anti-patterns to avoid

- ❌ `pip install pixelrag` / the `pixelbrowse` plugin (fabricated paper provenance; un-audited).
- ❌ Self-hosting ColPali for v1 — Embed v4 gives 90% of the value, managed, single-vector.
- ❌ Dual-writing vectors to Redis "for speed" + Qdrant "for durability."
- ❌ Treating RAG/GraphRAG as storage stages.
- ❌ Enabling the semantic cache without a `scope_key` policy (PR-A fails closed by default).
- ❌ Chunking the page image — Embed v4 already fuses layout+text per page.
- ❌ Running the visual arm on every doc — gate it to visually-rich/scanned content.

## Caching — do we add "Redis vector search / semantic caching (Redis RAG)"?

**Redis vector search as a second vector store: NO.** Dragonfly (Redis-5 core) has no
RediSearch — it cannot do vector search; and it's redundant with Qdrant. Keep Redis for
exact-match KV + hot payloads.

**Semantic caching: YES — already exists, Qdrant-backed, gated off.** `cache/semantic.rs`
+ `semantic_response_cache`: cosine ≥ `semantic_cache_min_score`, TTL + `prune()`. To enable:
`SEMANTIC_CACHE_ENABLED=true` + tune `min_score`/`ttl` + prune cron — **after** the authz gate
(PR-A, now landed) and freshness invalidation are in place.

**Two correctness gates:**
1. **Authz/ownership leak — CLOSED by PR-A.** Cache is now partitioned by `scope_key`
   (per-user visible-set hash / `org-shared`) and fails closed when absent. *Remaining:* the
   Model Plane gateway must compute and pass `scope_key` on its `SemanticCache` calls.
2. **Freshness.** Logical TTL can serve stale answers when a doc version changes. Tie
   invalidation to doc-lifecycle events via `cache/invalidator.rs` (verify it covers the
   semantic collection), not TTL alone.
