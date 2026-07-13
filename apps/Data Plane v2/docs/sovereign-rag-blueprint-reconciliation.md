# Sovereign EU RAG Blueprint (v3.0) — Reconciliation with CoreSystem Reality

> Maps the v3.0 blueprint onto what already exists in CoreSystem (Data Plane v2 +
> Model Plane), what we built & verified this session, what conflicts with a
> decision, what crosses a plane boundary, and what is genuinely net-new. Read
> this before picking a "deep dive" — three of the blueprint's choices fork against
> work that already exists.

> **Verified 2026-07-10** — re-checked against live containers + current source.
> Two facts below have moved since this doc was written (single commit,
> 2026-06-23) and are corrected in place:
> 1. **PR-F (MinIO CAS + page-image producer) has since shipped and is live.**
>    `dpv2-minio` bucket `dataplane-cas` holds real page-image PNGs (org- and
>    doc-scoped, timestamps through 2026-07-05); Qdrant `dataplane_page_images`
>    holds 12 indexed points (`status: green`); the live `dpv2-retrieval-engine`
>    container has `W_VISUAL=0.2`, not `0`. The "ships dark" framing and the
>    "Recommended sequence" step 1 (PR-F) below are **stale — that step is done**.
> 2. **The Dragonfly "no RediSearch → cannot do vector search" premise is now
>    wrong.** The deployed `dpv2-dragonfly` is `df-v1.37.0`, which ships a
>    `search` module supporting `FT.CREATE ... VECTOR HNSW` and `FT.SEARCH ...
>    KNN` — live-tested here with real cosine-similarity ranking, not just
>    command-acceptance. Decision-table row 1's *recommendation* (split
>    Dragonfly=KV / Qdrant=semantic-vector) may still be reasonable for other
>    reasons (Qdrant already owns the vector SoR; avoid a second vector layer to
>    keep in sync), but re-justify it on those grounds, not on Dragonfly being
>    incapable — it isn't, as of this version.
>
> Everything else checked (RRF `fusion.rs` k=60 folding in dense+sparse+wiki+
> visual via three sequential pairwise calls in `orchestrator.rs`; semantic-cache
> collection name `semantic_response_cache` matching `config.rs`'s
> `default_semantic_cache_collection()`; rerank model still `rerank-english-v3.0`
> per `.env`/live container) held up as described.
>
> One more shift worth knowing before re-reading Decision 1: ColQwen2 already has
> a foothold in the code as an **opt-in late-interaction reranker** over Embed
> v4's top-K visual candidates (`VISUAL_RERANK_ENABLED`, `src/search/colqwen.rs`,
> calls out to a separate GPU inference server) — currently `false` in the live
> env. That is *not* the "replace Embed v4 as the embedder" fork Decision 1
> describes; it's a smaller, already-built, disabled-by-default add-on. Decision
> 1 itself (embedder choice) is still open.

## Starting point (built + verified this session)

The visual RAG core is implemented and passing real tests (`cargo test`,
exit-code-checked): authz-safe semantic cache (PR-A), **Cohere Embed v4** image
provider (PR-B), page-image consumer → Qdrant `dataplane_page_images` (PR-D),
visual query arm + `w_visual` RRF fusion (PR-C), structural chunker keeping
tables/code atomic (PR-E). ~~It ships **dark** (`W_VISUAL=0`, no producer yet).~~
**Superseded 2026-07-10: PR-F (CAS + producer) has since shipped; the live
deployment runs `W_VISUAL=0.2` with real indexed page-images — see the verified
note above.** Detail: `visual-rag-integration-plan.md`.

## Component-by-component reconciliation

| Blueprint component | Reality in CoreSystem | Verdict |
|---|---|---|
| **Dragonfly "semantic cache"** (entry firewall + *semantic vector match*) | ~~Dragonfly has **no RediSearch → cannot do vector search**. Exact-match KV only.~~ **Corrected 2026-07-10: the deployed `df-v1.37.0` DOES support real `FT.CREATE ... VECTOR HNSW` + `FT.SEARCH ... KNN` (live-tested, real cosine ranking) — this premise is outdated.** The *semantic* cache is still implemented as **Qdrant `semantic_response_cache`** (built; authz-scoped in PR-A; currently `SEMANTIC_CACHE_ENABLED=false` in the live env). | ⚠️ Split still stands, but re-justify on "Qdrant is already the vector SoR, avoid a second synced ANN store" — not on Dragonfly being incapable. |
| **Not Diamond meta-router** | Model Plane already has an intent/router layer (cost-core complexity scoring, model-router, inference-core `FallbackChain` Budget/Balance/Genius). | ⚠️ **Plane boundary + sovereignty.** This is **Model Plane**, not Data Plane, and partly exists. "Not Diamond" is an external **US SaaS** called per query — contradicts the sovereignty thesis and adds latency. Use a **local** classifier (the blueprint itself says "localized random forest"). |
| **Cohere Command R+ reasoner + LangGraph/Agno loop** | Model Plane **execution-core** (Rust agent loop, governed multi-tool, HITL, runs-history) + inference-core. | ⚠️ **Plane boundary.** Reasoning/agent loop is **Model Plane** and already exists in Rust. Don't bolt on a second (Python LangGraph/Agno) runtime — add Command R+ as an inference-core **provider**. |
| **Local Llama 3.3 via vLLM** | Not present. | ✅ **Net-new** — but a **Model Plane** inference-core provider, not a Data Plane component. |
| **Qdrant SoR — ColQwen2 multivectors** | Qdrant SoR is built. We implemented **Cohere Embed v4 single-vector** (1536), *deliberately* to avoid self-hosted GPU + the multivector/MaxSim rewrite. | 🔱 **THE fork.** Embed v4 (built, managed, Azure-EU) vs ColQwen2 (sovereign, GPU, multivector — reverses PR-B/C/D and re-adds the multivector work we cut). See "Decision 1". |
| **Postgres + pgvector + pgvectorscale** | Postgres **graph** is built (`graph_entities/relationships/communities`, full-text) + hard permissions. Vectors live in **Qdrant**, not pgvector. | ✅ Graph aligns. ❌ **Drop pgvector** — redundant with the Qdrant SoR; two ANN stores = sync burden for no gain. |
| **Quickwit sparse (cold text on MinIO)** | Quickwit `dataplane-corpus` on MinIO + Postgres `ts_rank` fallback. | ✅ **Built.** |
| **Meilisearch keyword (typo-tolerant IDs)** | Not present. | ✅ **Net-new, additive** — a genuine 4th arm (exact codes/IDs/typos) Quickwit/BM25 doesn't serve well. |
| **MinIO CAS (immutable binaries + PixelRAG images)** | ~~Not present for documents (MinIO exists only as Quickwit's segment backend).~~ **Corrected 2026-07-10: PR-F shipped.** `dataplane-cas` bucket holds real org/doc-scoped page-image PNGs; feeds Qdrant `dataplane_page_images` (12 points, green). | ✅ **Done, not `🔜`.** PR-F is no longer the gating dependency — see "Recommended sequence" note. |
| **Redis "Live Context Engine" (multi-turn session)** | Multi-turn session/run state = Model Plane **session-core** (durable). The Redis fleet already migrated to Dragonfly. | ⚠️ **Plane boundary + redundant.** Sessions are **Model Plane**; don't stand up a second Redis in Data Plane. |
| **RRF fusion** | `fusion.rs` RRF (k=60), built; fuses dense+sparse+wiki+visual. Graph is a separate endpoint. | ✅ Built. Arm-set differs (see Decision 3). |
| **Cohere Rerank 4** | `rerank-english-v3.0` wired. | ✅ Built; bump model id = trivial config. |
| **GDPR Art. 17 purge cascade** (Postgres → MinIO/Qdrant/Quickwit/cache) | Erasure/DSAR built (Phase 2 + compliance MVP); page-image delete wired (PR-D → `delete_vectors_by_document` purges Qdrant); cache invalidator + semantic-cache `prune()`. | ✅ **Largely built** and the visual arm's purge path is already in. Strong alignment. |
| **EU residency / Azure-EU / ZDR** | ZDR egress guards built into every embed hop (PR-B/C); residency work (Phase 3). | ✅ Aligns. |

## Decisions to make (these fork the build)

### Decision 1 — Visual embedder: Embed v4 (built) vs self-hosted ColQwen2
The sharp point: **Command R+ (reasoning) and Cohere Rerank already egress the
retrieved text *and the generated answer* to Azure EU.** Those see far more
sensitive content than an embedding call. So self-hosting **only** the visual
embedder (ColQwen2 on bare-metal GPUs) buys **limited** sovereignty while adding the
biggest cost line in the blueprint — *unless* the hard requirement is specifically
"**document page-images never leave our hardware**" (plausible: scanned PII, layout).
- **Keep Embed v4** (managed Cohere on Azure-EU DataZone, ZDR, single-vector) →
  consistent with the blueprint's own reasoning tier; zero GPU; already built.
- **Switch to ColQwen2** (self-host) → maximal sovereignty + top ViDoRe accuracy,
  but reverses PR-B/C/D, re-adds multivector/MaxSim Qdrant work, and needs the H100/
  B200 fleet. Only worth it if "images never leave" is a hard line **and** you also
  self-host reasoning (else the boundary is already crossed downstream).
- **Both behind a flag** → Embed v4 default, ColQwen2 as a sovereign opt-in. Most
  expensive to maintain (two visual code paths) but hedges the policy.

### Decision 2 — Respect the plane boundary
The blueprint's *middle* (router → reasoner → agent loop → session) is **Model
Plane**, and most of it exists (intent layer, execution-core, session-core,
inference-core). Data Plane owns the **4-arm retrieval + stores + erasure**. Keep
them separate (CLAUDE.md: "No direct database crossing between planes"). Concretely:
router, Llama/vLLM, Command R+, LangGraph, Redis session → **Model Plane** changes.

### Decision 3 — Arm set + drop redundancies
- Blueprint arms = graph + visual + sparse + **keyword**. Ours = dense + sparse +
  wiki + visual (graph is a separate endpoint). To match: **add Meilisearch** (new)
  and **fold graph into the fused path** (outstanding). Keep **dense** (text) — it's
  your highest-recall arm; the blueprint omits it.
- **Drop pgvector** (Qdrant is the vector SoR). **No second Redis** (Dragonfly +
  session-core cover it).
- "Not Diamond" → **local classifier** (sovereign, <10ms, no external call).

## Recommended sequence

1. ~~**PR-F — MinIO CAS + page-image producer.** Lights up the visual arm already
   built; identical work whether Decision 1 picks Embed v4 or ColQwen2 (only the
   embedder model swaps). **Highest leverage, no blocked forks.**~~ **Done as of
   2026-07-10 (see verified note at top) — `dataplane-cas` populated, `W_VISUAL=0.2`
   live, `dataplane_page_images` indexed. Start execution at step 2.**
2. **Resolve Decision 1** (visual embedder) — gates GPU sizing. (Note: ColQwen2
   already has a disabled-by-default reranker foothold — see verified note — but
   the embedder-choice fork itself is still open.)
3. **Fold graph into the fused RRF path** + (optionally) **add Meilisearch** as the
   keyword arm → matches the blueprint's 4-arm retrieval (Data Plane).
4. **Model Plane track** (separate): local router classifier, Command R+ provider,
   vLLM Llama provider — only if/when reasoning moves in-house.

## On the blueprint's 3 "deep dives"
- **#1 GPU/infra sizing** — *blocked on Decision 1*. Only meaningful if ColQwen2/Llama
  self-host is chosen. Premature otherwise.
- **#2 Postgres schema & Graph-RAG** — *useful now, mostly decoupled*. Graph is the one
  retrieval arm not yet fused; designing its schema + fusion is real outstanding work.
- **#3 Not Diamond routing** — *Model Plane*; reframe as a **local** classifier design
  (don't introduce an external US router into a sovereign system).
