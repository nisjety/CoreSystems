# Data Plane v2 — Retrieval Quality & Durability Plan

> **Created:** 2026-08-05
> **Method:** every claim below was verified against the **live running stack**
> (all 9 services + Postgres/Qdrant/Neo4j/Dragonfly/NATS/MinIO/Quickwit healthy)
> and against current source. Live container env, service logs, Postgres row
> counts, Qdrant collection configs, Neo4j labels, and Quickwit doc counts were
> read directly. Nothing in this document is inferred from other docs.
>
> **Standing rule for this plan:** read the **live container env**, never the
> compose default. Several live values differ from `docker-compose.yml`
> (`SPARSE_SEARCH_BACKEND`, `W_VISUAL`), and one investigation reached the wrong
> conclusion by trusting the compose file.

---

## 0. Executive summary

Data Plane v2 is architecturally ahead of most production RAG systems: 5-arm
concurrent RRF fusion, GraphRAG with Neo4j multi-hop, visual/multimodal
retrieval, community detection, smart query-adaptive mode-mix, ZDR propagation,
signed event envelopes, and four independent `FOR UPDATE SKIP LOCKED` outbox
implementations.

Its problems are **not missing paradigms**. They are:

1. **Two retrieval arms silently serving degraded results** (Quickwit 77% blind;
   English stemming on Norwegian).
2. **Six subsystems built and left inert** (contradiction writer, community
   summaries, response cache, `context_pins`, `agent_retrieval_configs`,
   golden-set evals).
3. **No measurement instrument** — `eval_golden_judgments` is 0 rows, so
   retrieval quality is scored by the proxy `recall = min(cand/10, 1)`.

Consequence for sequencing: **fix the silent defects, then build the instrument,
then tune.** Every relevance change below is unverifiable until the golden set
exists.

---

## 1. Verified current state (2026-08-05)

### 1.1 Services

| Service | Port | Language | Role |
|---|---|---|---|
| `documents-api-go` | 8010 | Go | Document CRUD, bulk ingest, outbox, ZDR gate |
| `wiki-store-go` | 8011 | Go | Wiki pages/versions, wiki event outbox |
| `data-orchestrator-go` | 8012 | Go | Reindex/rebuild/refresh jobs |
| `data-quality-go` | 8013 | Go | Lint, trust scoring, eval runs |
| `retrieval-engine-rs` | 8004 / 50052 | Rust | Hybrid retrieval, fusion, rerank, context packing |
| `index-engine-rs` | 9201 | Rust | Chunking, knowledge units, deletion outbox |
| `embedding-engine-rs` | 9202 | Rust | Embeddings → Qdrant, wiki + page-image consumers |
| `graph-index-rs` | 9203 / 50053 | Rust | Entity/relationship/claim extraction, Neo4j mirror |
| `quickwit-adapter-rs` | 9204 | Rust | Lexical/BM25 index sync |

### 1.2 Storage — live contents

**Postgres** (39 tables, 19 MB):

| Table | Rows | Note |
|---|---|---|
| `documents` | 189 | |
| `knowledge_units` | 223 | 221 `done`, **2 `failed`** |
| `graph_entities` | 547 | vs 276 text units → duplication |
| `graph_relationships` | 214 | |
| `graph_claims` | 128 | `contradicted_by_claim_ids` never written |
| `graph_communities` | 13 | `summary` column always `NULL` |
| `graph_text_units` | 276 | |
| `retrieval_runs` / `retrieval_candidates` | 116 / 1154 | provenance trail works |
| `documents_outbox` | 249 | |
| `chunk_lineage` | 100 | |
| `context_pins` | **0** | CAG unused |
| `agent_retrieval_configs` | **0** | per-agent tuning unused |
| `eval_golden_judgments` | **0** | no measurement |
| `data_orchestrator_jobs` | 0 | |
| `wiki_pages` | 1 | |

**Qdrant** — 4 collections, **all with `quantization_config: None`**, default
HNSW `m=16 / ef_construct=100`, `on_disk_payload: true`:

| Collection | Points | Dim |
|---|---|---|
| `dataplane_knowledge` | 221 | 3072 |
| `dataplane_page_images` | 11 | 1536 |
| `wiki_block_embeddings` | 1 | 3072 |
| `entity_summary_embeddings` | **0** | 3072 |
| `semantic_response_cache` | **does not exist** | — |

**Neo4j** — `:Entity` (547 nodes) and `:REL` only. Claims are not mirrored.
Explicitly a rebuildable, non-authoritative read-model; Postgres is canonical.

**Quickwit** — `dataplane-corpus` (**50 docs**), plus auto-created
`otel-logs-v0_7` and `otel-traces-v0_7`.

**Dragonfly** — used by `retrieval-engine-rs` **only** (sole redis dependency,
`Cargo.toml:40`).

### 1.3 Live env deltas from compose defaults

| Variable | Compose default | **Live** |
|---|---|---|
| `SPARSE_SEARCH_BACKEND` | `postgres` | **`quickwit`** |
| `W_VISUAL` | `0.05` | **`0.2`** |
| `COLQWEN_ENDPOINT_URL` | *(empty)* | `host.docker.internal:8090` — **unreachable** |
| `RERANK_ENDPOINT` | *(empty)* | real Azure Cohere endpoint |
| `SEMANTIC_CACHE_DATAPLANE_ENABLED` (model-gateway) | — | **`false`** |

---

## 2. Verified defects

### D1 — Quickwit lexical arm is 77% blind (P0)

`dataplane-corpus` holds **50 docs** vs 221 in Qdrant / 223 `knowledge_units`,
while `SPARSE_SEARCH_BACKEND=quickwit` is live-primary.

**Root cause:** `quickwit-adapter-rs/src/stream.rs:94-97` uses plain core-NATS
`nats.subscribe()` — no JetStream, no ack, no redelivery. Handler errors log and
drop (`stream.rs:111-113`). `REBUILD_ON_START=false`, so it never self-heals;
the only repair path is the approval-gated `quickwit_admin_jobs` queue (0 rows).

Observed live in logs:
`"sparse search backend failed; falling back", primary: quickwit, fallback: postgres`.

### D2 — English stemming on a Norwegian-first corpus (P0)

`'english'` is hardcoded at **16 sites**, including the STORED generated column:

- `infra/postgres/migrations/20260519190000_bm25_tsvector.sql:9` and
  `init.sql:659` — `content_tsv GENERATED ALWAYS AS (to_tsvector('english', text)) STORED`
- `search/sparse.rs:183,188`; `search/graph.rs` (6 sites); `search/wiki.rs:23`;
  `search/contradictions.rs:21`
- GIN indexes: `init.sql:53` (`documents`), `:136` (`knowledge_units`),
  `:210` (`graph_entities`), `:250` (`graph_claims`)

Postgres has `norwegian` available; `default_text_search_config = pg_catalog.english`.
A sampled live Qdrant point reads *"Dette er en tekstnettdel i en av to kolonner…"* —
Norwegian content, English stemming.

Note: Quickwit's own config uses `tokenizer: default` (no stemming), so it is
language-neutral and currently **better** for Norwegian than the Postgres fallback.

### D3 — Page-image consumer has no DLQ (P0)

`embedding-engine-rs/src/image_consumer.rs:175-176` — `ack_wait 120s`,
`max_deliver 5`, then the message is **silently dropped**. Text consumers have
DLQs (`index-engine` `dataplane.dlq.index-engine` at `stream/mod.rs:15,179,192,196`;
embedding knowledge-units at `stream/mod.rs:297,311,317`). The visual path does not.

### D4 — ColQwen reranker is a live no-op burning timeouts (P0)

`VISUAL_RERANK_ENABLED=true` with `COLQWEN_ENDPOINT_URL` pointing at an
unreachable host (verified HTTP 000). Logs show repeated
`"visual reranker failed; keeping Embed-v4 order"`. No circuit breaker — the
graph arm has one (`search/graph_remote.rs:19-65`), the visual path does not.

### D5 — Two permanently-failed embeddings (P0)

`knowledge_units.embedding_status = 'failed'` × 2. Nothing retries them, and
`search/sparse.rs:188` excludes `'failed'` — that content is unreachable in
both arms, permanently.

### D6 — Contradiction feature is a hollow shell (P1)

Full read path exists and is consumed: `store.rs:640-698` (`get_contradictions`),
`GET /v1/graph/contradictions` (`api.rs:87,420-431`), gRPC `GetContradictions`
(`grpc.rs:239-266`), `retrieval-engine/search/contradictions.rs`, and Model Plane
`session-core::fetch_graph_segments` (`grpc.rs:2972-3020`).

**Nothing writes it.** `persist_extraction` (`store.rs:150-172`) hardcodes
`claim_status='active'` and omits `contradicted_by_claim_ids`. Returns empty forever.

Related dead wiring: `data-orchestrator/executor.go:261-266` forwards a
`"contradiction"` lint kind that `data-quality/lint.go:55-64` never emits.

### D7 — No entity resolution (P1)

`store.rs:81` mints `Uuid::new_v4()` per extraction, so re-ingesting identical
text creates new entities every time → 547 entities over 276 text units. Makes
the Neo4j `MERGE` non-idempotent in effect.

### D8 — Both response-cache tiers are dead (P1)

- DPv2 `SEMANTIC_CACHE_ENABLED=true`, but its only consumer (`model-gateway`)
  has `SEMANTIC_CACHE_DATAPLANE_ENABLED=false`, and `semantic_response_cache`
  does not exist in live Qdrant → never written.
- The Dragonfly response tier (`cache/mod.rs:54-67`) has **zero call sites**
  (only an `#[ignore]`d test), while `cache/invalidator.rs:23-56` faithfully
  invalidates it.

### D9 — Community summaries never generated (P1)

`graph_communities.summary` column exists; `entity_summary_embeddings` collection
is provisioned at `embedding-engine/main.rs:53`; detection runs and produced 13
communities. But `community.rs:85` hardcodes `summary: None` and the collection
has 0 points. GraphRAG "global search" is ~80% scaffolded and inert.

### D10 — Per-agent retrieval tuning unused (P1)

`agent_retrieval_configs (org_id, agent_id, weights, rerank)` exists, is read by
the orchestrator (precedence: request > agent config > smart > static), and has
**0 rows**. Every surface — chat, knowledge search, inbox, agent runs — runs one
static global profile with `rrf_k` hardcoded to `60.0` at four call sites
(`orchestrator.rs:245,260,265,270`).

### D11 — No golden set (P0.5, gating)

`eval_golden_judgments` is 0 rows. Quality is measured by
`recall = min(cand/10, 1)`, `ndcg = recall * 0.9`. Nothing can be tuned safely.

### D12 — `data_orchestrator_jobs` has no durability (P2)

`orchestrator.go:173-180` fires a bare `go e.Run(...)` from the HTTP handler. No
poller, no lease. A restart orphans jobs in `'running'` forever. This is the one
place the plane's own `SKIP LOCKED` idiom is missing — reference implementation
with exponential backoff already exists at `wiki-store-go/internal/events/outbox.go:108-114,128-137,178`.

### D13 — No quantization (P2)

All 4 Qdrant collections created with only
`vectors_config(VectorParamsBuilder::new(dim, Distance::Cosine))`
(`embedding-engine/qdrant_writer/mod.rs:9-21`, `retrieval-engine/cache/semantic.rs:125-137`).
Zero `quantization` matches tree-wide. Low urgency at 221 points, but changing it
later forces collection recreation.

### D14 — `query_expansion` is dead plumbing (P2)

Accepted on HTTP and gRPC (`pipeline/types.rs:69`, `grpc/retrieval_svc.rs:166,560`);
the only other reference is `api/mod.rs:1110: query_expansion: None`. Nothing consumes it.

### D15 — No similarity cutoff on retrieval (P1)

A score threshold exists only for the semantic *cache*
(`config.rs:135` `semantic_cache_min_score`). RRF returns top-k regardless of
absolute relevance, so irrelevant context is fed to the LLM when nothing matches.

### D16 — Slow statements (investigate, P2)

`COMMIT` 7.06s, single-row `INSERT` 3.97s, advisory lock 4.0s. **Not** resource
pressure: 31 MiB / 512 MiB, 100% cache hit, 19 MB DB, 8 connections. Most likely
host fsync latency on the external `/Volumes/Lagring` volume. Confirm it is a
dev-environment artifact before anyone reads it as a production capacity signal.

---

## 3. Decisions taken — third-party memory/RAG products

All four were evaluated against the same bar: **self-hostability, EU data
residency, ZDR propagation (Rule 4), plane ownership (Rule 7), and runtime fit
(Rust/Go hot path).**

| Product | Verdict | Decisive reason |
|---|---|---|
| **Mem0** | ❌ Reject | OSS is `mem0ai` **2.0.16** (no "v3"). Graph memory **removed from OSS** — repo tree has zero graph code, no neo4j/kuzu/memgraph dependency. Their docs: *"Graph Memory is built in… no Neo4j… runs natively inside the platform"* → managed-cloud-only = EU-residency blocker. Python; cannot be a library in a Rust container. |
| **Cognee** | 🔬 Lab only | Apache-2.0, Python, **self-hostable including Neo4j** — clears the sovereignty bar, unlike the others. But Python on the hot path, defaults to LanceDB+Kuzu (Qdrant+Neo4j is its least-tested path), and would become a **second writer** to the knowledge graph. Acceptable use: offline extractor grading beside `retrieval-eval-py`. |
| **LlamaIndex** | ❌ Reject (borrow ideas) | MIT, Python. No Rust binding. Proposal would rewrite `fuse_arms` — the most mature, exact-match-unit-tested component — to gain patterns that are ~600 lines of native Rust. |
| **Maximem Synap** | ❌ Reject (borrow ideas) | Marketing page claims self-hosting; the **repo README contradicts it**: *"the Synap memory engine… is **not** open source… there is nothing to self-host, and an API key is required."* 496 files, zero qdrant/neo4j/postgres references — client SDKs only. Hardest residency conflict of the four. |
| **Maximem Vity** | ❌ N/A | Consumer B2C personal memory vault for ChatGPT/Claude/Gemini users. Not an infrastructure component. |
| **`stashapp/stash`** | ❌ Reject | **AGPL-3.0** — network copyleft would oblige releasing the entire combined work's source; hard blocker for proprietary SaaS, independent of anything else. Also a personal media-library organizer with no documents/chunks/embeddings/retrieval functionality. Not to be confused with the entry below. |
| **`alash3al/stash`** | 🟢 **Best candidate — borrow now, adopt later** | Apache-2.0, **Go**, self-hosted single binary, 77 files, MCP server. Clears every bar the others failed. See §3.2. |
| **LiteLLM** | ❌ Reject | Duplicates Model Plane `inference-core` FallbackChain + router-policy. Wrong plane. License `NOASSERTION` (mixed OSS/commercial). Would arrive transitively via Cognee as an LLM path bypassing `inference-core` — breaking ZDR propagation. |

### 3.1 Cross-cutting rationale

Every vendor above sells **per-user conversational memory**. Data Plane v2 owns
**org-scoped durable corpus knowledge**. Per `master-ownership-matrix.md:110`,
`session-core` owns "memory index, compaction, context assembly" and
`letta-bridge` is the designated memory bridge. Conversational memory belongs in
**Model Plane**, not here.

Benchmark caveat: LongMemEval and LoCoMo measure conversational fact recall over
long dialogues. They do not measure org-scoped document retrieval with citations.
Architectural reasoning transfers; scores do not.

### 3.2 `alash3al/stash` — the one to mine

Apache-2.0 · Go · self-hosted single binary · Postgres + pgvector · MCP server ·
77 files. It is the only evaluated memory product that is simultaneously
permissively licensed, self-hostable end-to-end, and written in a language this
monorepo already uses.

`internal/brain/` implements, working, several things this plane scaffolded and
left inert:

| Stash module | Maps to |
|---|---|
| `contradiction.go` + `00014_create_contradictions.sql` | **D6** — the contradiction writer we designed and never built (P1-4) |
| `decay.go` + `00015_add_decay_checkpoint.sql` | recency decay (P2-3) |
| `consolidate.go` (+ `_failure`/`_goal`/`_hypothesis`) | community summarization / compaction (P1-5) |
| `00006_create_fact_sources.sql` | per-fact provenance — the citation model |
| `00012_create_embedding_cache.sql` | equivalent to our Dragonfly embed cache (ours is already better-placed) |
| `namespace.go` | multi-tenancy |

**Three corrections to the circulating proposal:**

1. It does **not** write to Qdrant/Neo4j/Quickwit. Zero references to any of
   them; it is Postgres + pgvector (`00001_enable_vector.sql`).
2. The Go-vs-Rust-vs-WASM question is moot — it is already Go. No CGO, no FFI,
   no `wazero`.
3. `internal/embedder/openai.go` and `internal/reasoner/openai.go` call an
   OpenAI-compatible API directly, which would breach Rule 2. **Verified:
   model-gateway exposes no OpenAI-compatible surface** (routes are
   `/v1/browser/runs`, `/healthz`, `/v1/invoke/*`), so adoption requires either
   a compat shim there or swapping those two files for `inference-core`
   clients — tractable, since both are clean single-file seams.

**Decision:** mine `contradiction.go` and `decay.go` for P1-4 and P2-3 now;
consider fork-and-vendor for the Model Plane memory engine (P3). Fork rather
than depend: it is ~3 months old, single-maintainer, and has a hosted
commercial tier (usestash.io) — the same shape that moved Mem0's graph memory
behind a paywall.

### 3.3 Approved third-party additions

| Component | License | Where | Why |
|---|---|---|---|
| **Ragas** | Apache-2.0 | `retrieval-eval-py` (sanctioned Python lab) | Builds the golden set. Fills D11. |
| **`maximem-ai/memory_and_context_eval_harness`** | MIT | `retrieval-eval-py` | Second eval source + published methodology. |
| **`maximem-ai/file-vs-vector-study-results`** | CC-BY-4.0 | reference reading | Five-domain keyword (**Tantivy** — what Quickwit is built on) vs vector study. Directly informs `w_sparse`/`w_dense` tuning after D1 is fixed. |
| **Infinity** | MIT | evaluate for D4 | Serves embeddings + reranking + **colpali natively** → could replace both the dead ColQwen server and the Azure Cohere rerank dependency, in-region. |

---

## 4. Ideas borrowed natively (no new runtime dependency)

| Idea | Source | Target |
|---|---|---|
| Per-agent context architecture, configured at setup not inference | Synap (their stated #1 reason for their benchmark lead) | Populate `agent_retrieval_configs` (D10) |
| Entity resolution as always-on, incl. **alias** resolution ("Sarah" / "Sarah Chen" / "SC") | Synap (their stated differentiator) | D7 ladder: UUID v5 → alias clustering → LLM adjudication |
| Ontology-constrained extraction (closed entity-type enum in the JSON schema) | Cognee | `extractor.rs:278-294` — root cause of D7 |
| Community summarization + embed | Cognee / GraphRAG | D9 |
| "Extract structure, not raw text" | Synap | Conversation→knowledge promotion must be gated, never automatic |
| Graph beats vector at scale (embedding space gets noisy; "semantically similar but factually distinct" collide) | Synap | Invest in graph quality over dense-only tuning; pair D13 quantization with rescoring |
| Node postprocessor chain (composable, ordered, individually testable) | LlamaIndex | Refactor target that makes cutoff/decay/reorder cheap |
| Similarity cutoff | LlamaIndex | D15 |
| Lost-in-the-middle reordering | LlamaIndex | `pack_context_with_pins` |
| Parent-child / sentence-window chunking | LlamaIndex | `index-engine-rs` |
| Eval dimensions beyond recall: **consistency**, **false recall**, **context-rot resistance** | Synap | Golden-set design (D11) — weight toward **false recall**, since Verevon sells *cited* answers |
| Staged pipeline: extract → normalize → resolve → write | Cognee | Makes D6 tractable |

---

## 5. Phased plan

### Sequencing rules

1. **Silent defects before enhancements.** Polishing ranking while the lexical
   arm is 77% blind optimizes the wrong layer.
2. **The instrument before the tuning.** Every relevance change after P0 is
   unverifiable without the golden set.
3. **Batch the re-embeds.** Parent-child chunking and quantization both require
   re-embedding. Do them in one pass while the corpus is 221 points.
4. **Respect concurrent WIP.** As of 2026-08-05, 14 DPv2 files are dirty
   (inference-auth / retention-posture work), including
   `retrieval-engine-rs/src/pipeline/orchestrator.rs` and `search/rerank.rs`.
   Avoid those files until that work lands.
5. **Do not mix in Enterprise-next items.** `DATA_PLANE_ROADMAP.md:290-292`
   holds; this plan is MVP-correctness scoped.

---

### P0 — Stop the silent wrong answers

| ID | Task | Files | Status |
|---|---|---|---|
| **P0-1a** | Quickwit live-sync durability: core-NATS → durable JetStream consumer, explicit ack, `Nak` backoff, DLQ `dataplane.dlq.quickwit-adapter`; loud-error fallback per stream | `quickwit-adapter-rs/src/stream.rs` | ✅ **done** |
| **P0-1b** | Retention `WorkQueue` → `Interest` on the three fan-out streams | `index-engine-rs/src/stream/mod.rs`, `graph-index-rs/src/stream.rs`, `embedding-engine-rs/src/wiki_consumer.rs` | ✅ code done, deploy pending |
| **P0-1c** | Recreate the 3 streams (0 messages ⇒ zero-loss), rebuild, backfill 50 → 221 | operator | ▶ in progress |
| **P0-2** | Language-correct FTS — see measurement below | `migrations/20260805120000_language_neutral_fts.sql` (+`.down`), `init.sql`, `search/{sparse,graph,wiki,contradictions}.rs` (18 sites) | ✅ code done, deploy pending |
| **P0-3** | DLQ `dataplane.dlq.embedding-engine-page-images` for the page-image consumer | `embedding-engine-rs/src/image_consumer.rs`, `main.rs` | ✅ **done** |
| **P0-4** | ColQwen: endpoint unset so the arm is a clean no-op instead of a per-query timeout | `.env` (`COLQWEN_ENDPOINT_URL=`) | ✅ **done** |
| **P0-5** | Retry the 2 `failed` embeddings (`model-plane embedding failed`, 2026-07-21 — transient); add an alert for `embedding_status='failed'` | post-rebuild | pending |

#### P0-2 measurement — why `simple`, not `norwegian`

Measured against the 221 live embedded chunks on 2026-08-05:

| Language | Chunks | Share |
|---|---|---|
| English | 132 | 59.7% |
| Norwegian | 85 | 38.5% |
| ambiguous / short | 4 | 1.8% |

The corpus is genuinely mixed, and **the languages mix inside a single chunk** —
e.g. an English heading *"Pathogen Management"* over a Norwegian body
*"Tjenester Erfaringen viser at manglende oppfyllelse…"*. No single stemmer is
correct, and a per-document `language` column cannot help when the mixing is
intra-chunk.

`simple` performs no stemming and no stopword removal, so it is wrong in
neither direction, and it matches Quickwit's `tokenizer: default` — which
matters because Quickwit is the live primary sparse backend and Postgres is its
fallback, so the two arms now tokenise identically instead of disagreeing.

Accepted trade-off: inflected forms no longer unify in either language. Smaller
and more predictable than incorrect cross-language stemming, and revisitable
once the golden set exists (P0.5).

**Not touched:** `migrations/20260519190000_bm25_tsvector.sql` — an
already-applied historical migration. Editing applied migrations breaks
reproducibility; the new migration supersedes it. `init.sql` (fresh-install
baseline) **was** updated, so new deployments start correct.

#### P0-1 blocking constraint discovered during implementation (2026-08-05)

The fix is **not** a straight port of the `index-engine-rs` pattern. Live
JetStream topology (read from `nats:8222/jsz`):

| Stream | Retention | Existing consumer | Quickwit subject | Second consumer possible? |
|---|---|---|---|---|
| `DATAPLANE_KNOWLEDGE` | `interest` | `embedding-engine`, `graph-index-cleanup` | `knowledge.units.created` | ✅ yes |
| `DATAPLANE_SOURCE_OBJECTS` | `limits` | *(none)* | `source_objects.changed/.deleted` | ✅ yes |
| `DATAPLANE_GRAPH` | **`workqueue`** | `graph-index` | `documents.indexed` | ❌ **no** |
| `DATAPLANE_DOCUMENTS` | **`workqueue`** | `index-engine` | `documents.deleted` | ❌ **no** |
| `DATAPLANE_WIKI` | **`workqueue`** | `embedding-engine-wiki` | `wiki.version.published` | ❌ **no** |

`WorkQueue` retention permits exactly one consumer per subject, and **retention
is immutable after stream creation** — changing it requires delete + recreate.
`interest` is the correct policy for a genuine fan-out stream, and
`DATAPLANE_KNOWLEDGE` already proves the pattern in-house.

Therefore P0-1 splits:

- **P0-1a (code, done):** durable consumer per stream, with a loud-error
  fallback to the legacy ephemeral subscriber when a stream refuses the bind.
  No regression; the two eligible streams upgrade immediately.
- **P0-1b (operator + producer code, pending approval):** change retention to
  `interest` on the three WorkQueue streams and recreate them. Producers own
  the definitions — `index-engine-rs/src/stream/mod.rs:10` (`DATAPLANE_DOCUMENTS`),
  `graph-index-rs/src/stream.rs:15` (`DATAPLANE_GRAPH`),
  `embedding-engine-rs/src/wiki_consumer.rs:32` (`DATAPLANE_WIKI`).
  **All five streams currently hold 0 messages**, so recreation is a zero-loss
  operation in this window.
- **P0-1c:** backfill the 50 → 221 gap via the approval-gated
  `quickwit_admin_jobs` reindex path.

Until P0-1b lands, `dataplane.documents.indexed` — the bulk per-document index
trigger — remains on the lossy path, so P0-1a alone does **not** fully close D1.

**P0-1 acceptance criteria**
- Consumer is durable and survives an adapter restart with zero event loss
  (verify: stop adapter → publish → start adapter → event applied).
- Handler failure NAKs and redelivers; after `max_deliver` the event lands in
  `dataplane.dlq.quickwit-adapter`, never silently dropped.
- `dataplane-corpus` doc count reconciles to `knowledge_units` where
  `embedding_status <> 'failed'`.
- `cargo test -p quickwit-adapter` green; `cargo clippy -- -D warnings` clean.

**P0-2 acceptance criteria**
- Index-side and query-side text-search configuration match exactly (a mismatch
  returns **zero rows** — worse than bad stemming). Ship in one commit.
- Migration uses `CREATE INDEX CONCURRENTLY` outside a transaction.
- Norwegian query for a known Norwegian chunk returns it; English regression
  query still returns its chunk.

> **P0-2 correction to an earlier draft:** `content_tsv` lives on
> **`knowledge_units`**, not `graph_claims`; `graph_claims` uses `claim_text`,
> not `text`; and `idx_documents_content_tsv` does not exist (real names:
> `idx_documents_content_fts`, `idx_ku_text_fts`, `idx_gc_text`, `idx_ge_text`).
> Prefer `'simple'` or a per-document `language` column over a wholesale flip to
> `'norwegian'` — the corpus is mixed (2 orgs).

**Rollback for all P0:** each item is independently revertable; P0-1 and P0-3 are
additive consumer changes, P0-4 is config, P0-2 reverts by re-running the inverse
migration with the same one-commit index+query discipline.

---

### P0.5 — Build the measurement instrument (gates everything below)

| ID | Task | Where |
|---|---|---|
| **P0.5-1** | Golden set: ~50 judged queries against the real corpus, Norwegian and English | `eval_golden_judgments` |
| **P0.5-2** | Ragas harness + optional Maximem MIT harness | `retrieval-eval-py` |
| **P0.5-3** | Score **consistency**, **false recall**, **context-rot** — not just recall/nDCG. Weight false recall highest (citations product). | `data-quality-go` metric labelling (`metric_source: golden` already exists) |

---

### P1 — Finish what is already built

| ID | Task | Defect |
|---|---|---|
| **P1-1** | Populate `agent_retrieval_configs` with a profile per surface (chat / knowledge search / inbox / agent runs) | D10 |
| **P1-2** | Entity resolution ladder: UUID v5 over normalized `(org, text, type)` → alias clustering via `entity_summary_embeddings` → LLM adjudication on borderline pairs. Requires coordinated re-extract + `POST /v1/graph/rebuild` (changes all 547 `entity_id`s, referenced by `graph_relationships`/`graph_text_units`/`graph_communities` and Neo4j's unique constraint). | D7 |
| **P1-3** | Ontology-constrained extraction — closed entity-type enum in the extraction JSON schema | D7 root cause |
| **P1-4** | Contradiction **writer**: populate `contradicted_by_claim_ids` + `claim_status`. Separate job from P1-2 — dedup ≠ conflict detection. | D6 |
| **P1-5** | Community summarization: one LLM call per community via `inference-core` → `graph_communities.summary` → embed into `entity_summary_embeddings` | D9 |
| **P1-6** | Wire **one** response-cache tier (add `get_retrieval`/`set_retrieval` call sites in the orchestrator, or flip the gateway flag) | D8 |
| **P1-7** | Similarity cutoff on fused results | D15 |
| **P1-8** | Lost-in-the-middle reordering in `pack_context_with_pins` | — |
| **P1-9** | Emit the `"contradiction"` lint kind or delete the dead forwarding | D6 |

---

### P2 — Structural improvements

| ID | Task | Defect |
|---|---|---|
| **P2-1** | Node-postprocessor trait; migrate rerank / visual_rerank / cutoff / decay / dedup / reorder into ordered testable stages | — |
| **P2-2** | Parent-child (or sentence-window) chunking **+** Qdrant int8 scalar quantization with rescoring — one re-embed pass | D13 |
| **P2-3** | `document_date` column + backfill from connector metadata (SharePoint `modified_at` exists upstream, never persisted) + add to Qdrant payload → then recency decay | — |
| **P2-4** | `data_orchestrator_jobs` lease/claim durability, copying `wiki-store-go/internal/events/outbox.go` | D12 |
| **P2-5** | Expose `rrf_k` via config (hardcoded `60.0` × 4) | — |
| **P2-6** | Dragonfly-back the rate limiter (currently in-process `governor`, wrong across replicas) | — |
| **P2-7** | HyDE / multi-query into the dead `query_expansion` field | D14 |
| **P2-8** | CRAG: return a retrieval-confidence score to Model Plane. **No browser call from DPv2** — that breaks Rules 3 and 7. Note a `low_confidence` signal + `suggested_next_tools` already exists (`orchestrator.rs:1085-1113`); tune its threshold and verify `execution-core` consumes it. | — |
| **P2-9** | Confirm D16 slow statements are a dev-volume fsync artifact | D16 |

---

### P3 — Deferred / requires a decision

- Infinity evaluation to replace ColQwen + Azure Cohere rerank (in-region).
- Cognee as an offline extractor-quality benchmark in `retrieval-eval-py`.
- Conversation→knowledge promotion path (explicit gate, ZDR-classified, via
  `POST /v1/documents`; model on `wiki_proposals`). **Never** an automatic
  async write — `documents-api` would reject it for ZDR orgs anyway
  (`persistentZDRReason()`), and `graph_entities` has no `user_id`, so an
  automatic path would leak one user's facts org-wide.

- **Synap-pattern conversational memory engine — DECIDED 2026-08-05: build in
  Model Plane, later.** The four-stage pattern (ingest → extract → resolve →
  store/compact) is sound and is the same staged design this plan already
  borrows. Placement is what matters:
  - Home: **Model Plane `session-core`** (per `master-ownership-matrix.md:110`,
    it owns memory index / compaction / context assembly). Not Data Plane, and
    not a new engine.
  - Extraction: via `inference-core` with Structured Outputs / JSON schema —
    never a direct provider call (keeps ZDR propagating).
  - Embeddings: via `inference-core`; never independent (Rule 2).
  - Storage: through **Data Plane APIs**, never direct Qdrant/Neo4j clients
    (Rule 1).
  - Triggering: a **NATS durable consumer**, never a fire-and-forget
    `go func(){…}` — that is the D12 anti-pattern and loses the extraction on
    any crash.
  - Gates: ZDR check before persistence, plus explicit user/reviewer promotion
    before a conversational fact becomes org-scoped corpus knowledge.
  Scheduled after the golden set (P0.5) and the P0/P1 corpus fixes, so its
  effect is measurable rather than asserted.

---

## 6. Execution record

### 2026-08-05 — P0 implementation pass

**Code landed (all verified `cargo clippy --all-targets -- -D warnings` clean,
full suites green):**

| Change | Files |
|---|---|
| P0-1a durable JetStream consumers + DLQ + per-stream loud-error fallback | `quickwit-adapter-rs/src/stream.rs` (+233 lines) |
| P0-1b `WorkQueue` → `Interest` on the three fan-out streams | `index-engine-rs/src/stream/mod.rs`, `graph-index-rs/src/stream.rs`, `embedding-engine-rs/src/wiki_consumer.rs` |
| P0-2 language-neutral FTS, index side | `migrations/20260805120000_language_neutral_fts.sql` + `.down.sql`, `infra/postgres/init.sql` (5 sites) |
| P0-2 language-neutral FTS, query side | `retrieval-engine-rs/src/search/{sparse,graph,wiki,contradictions}.rs` (18 occurrences) |
| P0-3 page-image DLQ | `embedding-engine-rs/src/image_consumer.rs`, `src/main.rs` |
| P0-4 ColQwen endpoint unset | `.env` |

**Incidental fixes — pre-existing, unrelated to this work.** A newer clippy
(1.94) flags `field_reassign_with_default`, which was already failing
`-D warnings` on four crates before any change here. Each was rewritten to
build the summary struct from locals, preserving the FK-ordered statement
sequence and changing no behaviour:

- `quickwit-adapter-rs/src/gdpr.rs`
- `index-engine-rs/src/gdpr.rs`
- `graph-index-rs/src/store.rs`
- `retrieval-engine-rs/src/gdpr/purge.rs`

**Test totals:** 198 across the four stream/DLQ crates, 300 in
retrieval-engine-rs. 0 failures.

**Deployment posture at time of writing:** all five JetStream streams verified
at **0 messages**, making the P0-1b stream recreation zero-loss. Images
rebuilt with `docker-compose.yml` + `docker-compose.cross-plane.yml` — the same
pair the running stack was created with (confirmed via the container's
`com.docker.compose.project.config_files` label). Recreating with only the base
file would silently drop the cross-plane overlay.

**Known rollout caveat (P0-2):** index-side and query-side language config must
land together. During the container swap there is a brief window where a
migrated (`simple`) index is queried by a not-yet-replaced (`english`) binary,
which returns zero sparse rows. Acceptable on this stack; production needs a
two-phase or blue/green rollout.

### Deployment verification — 2026-08-05 02:01Z

All 16 Data Plane v2 containers healthy; `migrate` and `minio-init` exited 0.

**Streams migrated.** All four fan-out streams now `interest`;
`DATAPLANE_PAGE_IMAGES` correctly left `workqueue` (single consumer, no second
reader). quickwit-adapter bound **durable consumers on all five streams** with
**zero fallback/refusal log lines**:

```
DATAPLANE_DOCUMENTS       interest   index-engine + quickwit-adapter
DATAPLANE_GRAPH           interest   graph-index + quickwit-adapter
DATAPLANE_KNOWLEDGE       interest   embedding-engine + graph-index-cleanup + quickwit-adapter
DATAPLANE_WIKI            interest   embedding-engine-wiki + quickwit-adapter
DATAPLANE_SOURCE_OBJECTS  limits     quickwit-adapter
DATAPLANE_PAGE_IMAGES     workqueue  embedding-engine-page-images
```

**Zero-loss proven end-to-end.** Stopped quickwit-adapter → published to
`dataplane.documents.indexed` → stream held `messages=1`, consumer
`num_pending=1`, `delivered=0` → restarted → event delivered immediately
(rejected as `invalid signed event envelope`, correct: the probe was unsigned
and my handler treats that as terminal). Under the previous core-NATS
`subscribe()` the event would have been discarded with no trace. All streams
then drained to 0 with every consumer at 0 pending.

**FTS verified functionally** against the live corpus: Norwegian
`erfaringen` → 1 hit, `manglende oppfyllelse` → 1, `tjenester` → 6; English
regression `pathogen management` → 3. Both languages retrievable;
`content_tsv` confirmed as `to_tsvector('simple'::regconfig, text)`.

### ⛔ P0-1c BACKFILL — BLOCKED BY DESIGN, needs a decision

The true gap is **larger than the headline**: `dataplane-corpus` holds 50 docs
of which only **19 are `knowledge_unit`** against 221 indexable — a **91%**
lexical blind spot, not 77%. It also holds **30 `source_object` entries against
10 live**, i.e. stale rows that were never purged.

Every backfill route is deliberately gated:

1. **`REBUILD_ON_START`** — config validation *rejects* it outright:
   `"REBUILD_ON_START is disabled; use the scoped admin API"`
   (`quickwit-adapter-rs/src/config.rs:101-104`).
2. **Scoped admin API** (`POST /admin/rebuild`) — disabled at two layers.
   `api.rs`: *"the authorization policy currently returns only previews, but the
   production handler also refuses to execute a mutation. Durable job state,
   audit, rate limiting and safe Quickwit delete-task completion must exist
   before this branch can ever become executable."* This matches
   `DATA_PLANE_ROADMAP.md` P0 "Complete destructive-operation safety", which
   keeps it disabled on purpose. **Not bypassed.**
3. **`data-orchestrator POST /reindex`** — technically viable and
   non-destructive (its executor republishes `dataplane.documents.indexed`,
   `internal/jobs/executor.go:27`, which the now-durable consumer would index).
   **But** it also re-triggers graph extraction, and with **D7 unfixed
   (`Uuid::new_v4()` per extraction, no dedup)** a 189-document reindex would
   inflate `graph_entities` further and spend LLM calls for that privilege.

**Recommendation: sequence the backfill after P1-2 (entity resolution),** then
use route 3 — one reindex repairs the lexical gap *and* rebuilds the graph
cleanly under deterministic IDs. Alternative if the gap must close sooner: add
a small **additive, org-scoped, Quickwit-only** backfill (no clear) on top of
the existing `rebuild.rs` primitives — new work on a safety-sensitive surface,
so it wants review rather than an unsupervised run.

**P0-5 (2 failed embeddings) rides along.** Both are
`model-plane embedding failed` from 2026-07-21 (transient). They are left as
`failed` rather than reset to `pending`: `pending` would make them lexically
retrievable via `sparse.rs:188`, but with nothing in flight to embed them it
would misrepresent state and hide a known failure. They clear with the same
reindex.

### Final verification — 2026-08-05

| Gate | Result |
|---|---|
| `cargo check --workspace` | clean |
| `cargo test --workspace` | **~508 passed, 0 failed** |
| `cargo clippy -p <5 crates> --all-targets -- -D warnings` | clean (4 pre-existing lints fixed to get here) |
| `make build-go` | documents-api, wiki-store, data-orchestrator, data-quality — all OK |
| Containers | **16/16 healthy**; `migrate` + `minio-init` exited 0 |
| `/readyz` | retrieval-engine, quickwit-adapter, index-engine, embedding-engine, graph-index — all **HTTP 200** |
| ERROR lines since restart | **0 across all five rebuilt services** |
| ColQwen | one boot `WARN` ("visual reranker disabled") replacing a failure on every visual query |

`POST /v1/retrieve` returns **401 by design**: `CONTROL_PLANE_ENFORCEMENT=strict`
with `JWT_REQUIRED_AUDIENCE=data-plane` means the old internal-API-key bypass no
longer authenticates. Not a regression — the sparse arm's own SQL was verified
directly against the live corpus instead (see FTS results above).

### 2026-08-05 — P1-2 entity resolution (deterministic identity)

**Landed** in `graph-index-rs/src/store.rs` (clean file; `extractor.rs` left
alone as concurrent WIP), deployed, 60 tests pass incl. 8 new identity tests,
clippy clean.

- `GRAPH_ID_NAMESPACE` — fixed UUID v5 namespace. **Never change it**: it
  re-keys every entity/relationship/claim and orphans `graph_text_units`,
  `graph_communities` and the Neo4j `entity_id` constraint.
- `normalize_identity()` — trim + collapse internal whitespace + lowercase.
  Identity only; the first-seen spelling stays the display `entity_text`.
- `entity_identity(org, text, type)`, `relationship_identity(org, a, b, type)`
  (direction significant), `claim_identity(org, text)` (org-wide, so the same
  assertion in two chunks becomes one claim accumulating both `source_refs` —
  what contradiction detection needs).
- Field separator is U+001F, which `normalize_identity` strips, so it cannot
  occur inside a field — `("ab","c")` can never collide with `("a","bc")`.
- `ON CONFLICT` changed from `DO NOTHING` to a real **merge**: union
  `source_refs` (and `entity_ids` on claims) via `jsonb_agg(DISTINCT …)`, keep
  `GREATEST(confidence)`. `claim_status` is deliberately **not** reset, so
  re-extraction cannot resurrect a superseded claim as `active`.
- Returned id vectors deduped preserving first-seen order.

**Measured impact (read-only, against the live 547 rows):**

| | |
|---|---|
| existing rows | 547 |
| distinct v5 identities | **394** |
| duplicates that collapse | **153 (28.0%)** |
| identities with >1 existing row | 82 |

**28% is the floor, and the reason matters.** The largest merges are not case
variants — they are *identical* text re-extracted repeatedly, plus the same
entity under LLM-invented competing types:

```
x13  Organization  'Aquatiq'
x9   Company       'Aquatiq'     <-- same entity, different invented type
x8   Date          '10. september 2026'
x5   Service       'Cleaning Systems'
```

`Organization` vs `Company` for one company is direct evidence for **P1-3
(ontology-constrained extraction)**: a closed entity-type enum in the
extraction schema would unify those and push the reduction well past 28%.
P1-3 should follow P1-2 immediately.

**⛔ Legacy cleanup + reindex still blocked.** The 547 existing rows carry v4
ids; new extractions emit v5. Collapsing them requires the re-extraction that
`POST /v1/orchestrator/reindex` drives — which needs scope
`data:orchestrate` or `data:admin`, and **no registered service principal has
either**. The three `data-plane` principals (`quarry-edge`, `finspo-core`,
`imports-core`) hold only `documents:write` / `data:read` /
`org:data:read_all`. Granting it means adding an entry to
`PLANE_SERVICE_PRINCIPALS_JSON` in Control Plane — a different plane, a new
privileged credential, and a registry where one malformed entry 503s token
issuance platform-wide. **Operator decision, not an unsupervised step.**

Verified available for that reindex when authorised: graph-index → inference-core
gRPC health probe returns **200** over `inter-plane-bus`, so the extraction hop
is live.

### 2026-08-05 — P1-3 ontology-constrained extraction

**Landed** in `graph-index-rs/src/store.rs` + a surgical prompt change in
`extractor.rs`. Deployed, 64 tests pass (5 new ontology tests), clippy clean,
0 ERROR lines.

**The defect, measured:** the free-text `entity_type` field had produced **61
distinct types across 547 entities** — a new label almost per extraction call.
Not cosmetic: `Aquatiq` existed 13× as `Organization` and 9× as `Company`, so
P1-2's deterministic identity alone could not merge it and multi-hop traversal
saw two unrelated nodes. `Claim` also appeared as an *entity* type, colliding
with the `graph_claims` table.

**Closed ontology of 18 types** (`ENTITY_TYPES`): Organization, Person,
Location, Product, Service, Process, Substance, Organism, Standard, Document,
Technology, Training, Event, ContactPoint, Date, Metric, Industry, Concept.

`canonical_entity_type()` maps ~180 observed/likely synonyms onto them and sends
**anything unrecognised to `Concept`** — never passed through, or the ontology
silently reopens. Lookup strips case, whitespace and punctuation, so
`"Phone Number"`, `"phone_number"` and `"phone-number"` all collapse.

**Enforced at persistence, not in the extractor**, deliberately: the extraction
"schema" is only prose in the prompt (no provider-side enum, and the Anthropic
path drops structured-output schemas entirely), and both the `model_plane` and
`azure_openai` backends must be constrained identically. The prompt now lists
the closed set to *steer* the model; `store.rs` is the authoritative gate. The
canonical type is written to `graph_entities.entity_type` **and** mirrored to
Neo4j, so the read-model cannot drift from Postgres on type.

**Measured impact (read-only, live 547 rows):**

| Stage | Identities | Reduction |
|---|---|---|
| baseline (random v4) | 547 | — |
| + P1-2 deterministic ids | 394 | 28.0% |
| + P1-3 closed ontology | **378** | **30.9%** |

Entity types: **61 → 18**. `Concept` share: **18%** (100/547).

Two judgement calls worth recording:

- **`Industry` was promoted out of `Concept`.** Without it, `Concept` absorbed
  25% of all entities — at that size a catch-all stops being an escape hatch and
  becomes an untyped bucket. `Industry` was the 4th most common observed label
  (32 rows) and is a real retrieval facet for this domain (aquaculture, food
  processing). Note the honest trade: this *reduced* merges slightly (375 → 378)
  because it stops distinct industries over-merging into `Concept` — better
  precision bought with three fewer merges.
- **`Page` maps to `Document`, not `Technology`.** In a web-crawl corpus a page
  is content; `Website` is the system hosting it. A unit test initially asserted
  the opposite and caught the inconsistency.

**Still gated on the reindex.** These 378 identities are what re-extraction
*would* produce; the live 547 rows keep their v4 ids and free-text types until
`POST /v1/orchestrator/reindex` runs — which needs the
`data:orchestrate` / `data:admin` principal that does not yet exist.

**Not attempted:** relation-type canonicalisation. `relationship_identity`
already case-normalises, and 61 entity types was the acute problem; a closed
relation vocabulary is a separate, larger design question.

### 2026-08-05 — P1-9 contradiction lint kind (dead wiring resolved)

Deployed (data-quality + data-orchestrator), build/vet/test clean, 0 errors.

`data-orchestrator/executor.go` forwarded a `"contradiction"` lint kind that
`data-quality/lint.go` never emitted — dead wiring, because until P1-4 the
column it would have read was permanently empty. Both halves are now resolved,
and the second half turned out to be a latent bug rather than dead weight.

**Emit it (data-quality).** New `findContradictions` check reads
`graph_claims.contradicted_by_claim_ids`, scoped to org-visible live documents
via the same provenance join the graph read path uses — a quality report must not
flag a claim whose only source document is deleted or private. Registered as
`contradictions` in `lint.Run`; `Issue.Kind` and the package doc updated.

**Stop forwarding it (data-orchestrator).** Removed `"contradiction"` from
`wikiKinds`. The forwarding maps `Issue.ID` → `sweepItem.PageID` and posts to
`/v1/wiki/maintenance/sweep`, but data-quality's contradiction findings are
**claim-scoped** — the ID is a `claim_id`. The sweep handler validates only that
`PageID` is non-empty and that the kind is allowed; **it does not verify the page
exists**, so every forwarded contradiction would have written a
`wiki_maintenance_logs` row against a page that never existed. Emitting the kind
without this fix would have turned dead wiring into live corruption.

`contradiction` deliberately **stays** in wiki-store's own `allowedKinds`: a
*wiki-page-level* contradiction from a Model Plane wiki-maintenance agent is a
legitimate producer for that endpoint (per the ownership matrix). This linter is
simply not that producer.

**Coverage note:** `internal/lint` has no test files (a pre-existing gap the
roadmap already tracks under database-backed coverage). The new query was instead
validated by executing it verbatim against the live database — valid SQL,
0 rows, consistent with P1-4's finding that this corpus has no contradictions.

### 2026-08-05 — P1-6 retrieval cache made safe to wire (not wired)

156 tests (4 new) + the Dragonfly integration test extended and **run green
against a real disposable Dragonfly**. Deployed.

**Outcome differs from the plan item, deliberately.** P1-6 said "wire one
response-cache tier". Inspecting it first showed that wiring it as written would
have been a **cross-user data leak**, so the work became making it impossible to
wire unsafely.

The Dragonfly retrieval tier (`cache/mod.rs`) keyed entries on
`{org_id}:v{org_version}:{cache_key}` — no viewer. But retrieval results are
viewer-dependent: `orchestrator.rs:895` resolves `effective_viewer` +
`granted_docs`, and the step-6 ownership gate keeps a document only if
`owner_id = viewer OR visibility = 'org' OR it is in the viewer's grants`. Two
users in one org, same query, same `org_version`, legitimately get **different**
result sets. Caching on org alone would serve user A's private and
specifically-granted documents to user B — the same class of defect as the
org-IDOR this plane has already fixed once. That is very likely why the tier was
never wired.

Changes:
- `get_retrieval`/`set_retrieval` now **require** a `scope` argument, and an
  empty scope **fails closed** (miss on read, no-op on write). It is no longer
  possible to use this cache without deciding whose results are being cached.
  Mirrors the sibling semantic cache's `semantic_cache_require_scope`.
- New `viewer_scope_token(viewer, granted_docs)` derives it: `"org-shared"` when
  there is no viewer (sound — no ownership filter runs, so every such caller sees
  the identical set), otherwise viewer id + a hash of the **sorted, deduplicated**
  grant set. Sorting matters because `user-core` returns grants in no guaranteed
  order; unsorted would fragment the cache per user. A domain separator between
  grants stops `["ab","c"]` colliding with `["a","bc"]`. A grant change alters
  the token, so newly granted or revoked access is never served from a
  pre-change entry.
- Integration test now asserts a second viewer and an empty scope both **miss**.

**Still not wired**, for the reason that has governed this whole pass: the two
call sites belong in `orchestrator.rs`, which a concurrent session is mid-rewrite
of (step-8 confidence gate). Wiring is now a small, obviously-correct change once
that lands — compute `viewer_scope_token(effective_viewer, &granted_docs)`
alongside the existing ownership resolution and pass it through.

Related observation: `cache/mod.rs` opens with
`#![allow(dead_code, unused_imports)]`, which is why a public cache tier with
zero call sites never produced a warning. Worth revisiting, but out of scope here.

### 2026-08-05 — P1-5 community summarisation (D9 closed)

78 tests (4 new) + a DB integration test, clippy clean, deployed.

`graph_communities.summary` and `entity_summary_embeddings` existed, detection
produced 13 live communities, and `retrieval-engine::community_summary_search`
was **already wired into `api/mod.rs:921`** — but `community.rs:85` hardcoded
`summary: None`, so GraphRAG "global search" (thematic questions no chunk can
answer) was scaffolded and inert. Same writer-missing shape as D6.

#### P1-5a — the blocking prerequisite nobody had noticed

Summarising was **unaffordable before it was possible**. `community_id` was
`Uuid::new_v4()` and `replace_communities` did `DELETE … WHERE org_id` then
re-`INSERT` — and detection runs on **every ingest that adds a relationship**.
So every summary would have been destroyed and re-paid-for on the next ingest,
forever.

Fixed first:
- **`community_identity(org_id, entity_ids)`** — UUID v5 over the **sorted,
  deduplicated** member set. Sorting is essential: `connected_components` yields
  no stable ordering, so an unsorted key would hash differently each run and
  defeat the purpose.
- **`replace_communities` is now upsert + prune**, not delete-then-insert.
  `summary` is deliberately absent from the `DO UPDATE SET` list, so an
  unchanged community keeps it. Communities that no longer exist are pruned via
  `NOT (community_id = ANY($2))`; an empty detection correctly clears the org.
- The per-org advisory lock is retained — with deterministic ids it now guards
  the upsert/prune interleaving rather than a UUID race.

Membership changes still mint a new identity, so a community that gained or lost
a member arrives without a summary and is re-summarised — correct, since the old
summary described a different set.

**Proven** by a new DB integration test: summary survives recompute (with member
order reversed, to prove order-independence end-to-end), stale community pruned
on membership change, empty detection clears the org.

#### P1-5b — the generator

- `GraphExtractor::summarize_community()` reuses the **same backend dispatch and
  ZDR guard** as `extract()`, including the hard refusal to egress restrictive
  content to the direct-Azure path. In practice graph entities only exist for
  non-restrictive content (the ingest consumer drops restrictive-ZDR events
  before extraction), so `zdr` is false here; the guard stays so that assumption
  cannot rot silently.
- Prompt is **grounding-constrained**: use only the listed entities, say so if
  they are too disparate to share a theme, answer in the dominant language
  (corpus is NO+EN mixed), return prose only. Capped at
  `MAX_SUMMARY_LABELS = 60` with the omission **disclosed to the model**
  ("and N further related entities") rather than silently truncated.
- `list_communities_needing_summary()` returns only `summary IS NULL` rows,
  largest-first, with member labels ordered by confidence descending — so
  truncation drops the weakest members and a backlog drains biggest-first.
- `set_community_summary()` is `AND summary IS NULL`, so two overlapping runs
  cannot both pay for and race on the same summary.
- Per-community best-effort: one failure is logged, the rest continue.

**Two controls, both conservative:**

| Variable | Default | Why |
|---|---|---|
| `COMMUNITY_SUMMARY_ENABLED` | **off** | Spends real inference budget on every ingest; must be an explicit operational opt-in, not something a deploy switches on. |
| `COMMUNITY_SUMMARY_MAX_PER_RUN` | 5 | Bounds cost/latency per ingest. A backlog drains over successive ingests instead of stalling one on many LLM calls. |

Runs only when detection **succeeded** — summarising against a half-updated
community set would key summaries to memberships that never existed.

**Deferred:** embedding summaries into `entity_summary_embeddings` (still 0
points). That collection is provisioned by embedding-engine, so the correct path
is graph-index publishing an event that embedding-engine consumes — a new
subject, stream, and signing-key pair. Postgres-side summaries already light up
the live `community_summary_search` endpoint, so this is a second increment, not
a prerequisite.

### 2026-08-05 — P1-7 similarity cutoff + P1-8 long-context reordering

Both are **relevance-affecting and therefore opt-in, default off.** There is no
scored golden set yet (P0.5 is seeded but not runnable), so neither may flip
silently — turn them on together with a before/after eval.

**Neither is wired into `orchestrator.rs`, on purpose.** A concurrent session is
mid-rewrite of exactly that region: its WIP at `orchestrator.rs:956` replaces the
step-8 confidence gate, having found that `rerank_score` is left at 0.0 by every
arm so the old gate marked *every* query low-confidence. Editing the same
score-gating block from two sessions is how a merge breaks. Both changes were
therefore placed in clean files.

#### P1-7 — dense similarity cutoff (`search/dense.rs`)

`DENSE_SCORE_THRESHOLD` (unset = previous behaviour) → Qdrant-native
`.score_threshold()` on the dense arm.

Applied at the Qdrant query rather than after fusion **because that is the only
place the score is calibrated.** Cosine similarity is an absolute 0..1 quantity,
so "below 0.30 is not a real match" means something. RRF's `final_score` is not:
rank 1 is ~0.03 (`1/(60+1)`), so any absolute threshold on it would be arbitrary
and would trip on good results. The concurrent WIP's own comment independently
reaches the same conclusion.

Fixes: when nothing in the corpus answers the query, the dense arm still returned
its `top_k` nearest neighbours — whatever they were — and RRF ranked them, so the
model received confident-looking irrelevant context instead of an honest empty
result. Values outside `0.0..=1.0` are rejected with a warning rather than
silently filtering everything or nothing.

#### P1-8 — long-context reordering (`context_pack/mod.rs`)

`LONG_CONTEXT_REORDER=true` (default off) reorders the packed facts so relevance
decreases toward the middle: `[1,2,3,4,5,6]` → `[1,3,5,6,4,2]`.

LLMs attend most reliably to the start and end of a long window and degrade in
the middle (Liu et al., *Lost in the Middle*). The packer fills in descending
relevance, so previously the **least**-relevant surviving fact always occupied
the tail — one of the two best-read positions — while mid-ranked evidence was
buried where it reads worst.

Properties held, each covered by a test:
- Runs **after** the budget loop, so what *fits* is still chosen in strict
  relevance order; only the survivors are reordered.
- Pure permutation — no fact added, dropped, or rescored, so citation rendering
  and any score-sorted consumer are unaffected.
- Packs under 4 facts are left alone (no meaningful "middle" to protect).
- Pinned CAG facts stay at the head: `pack_context_with_pins` prepends them
  after `pack_context` returns, so operator-decreed context is never shuffled.
- Default-off is itself asserted, so the opt-in contract cannot regress.

**Not added to `docker-compose.yml` / `.env.example`** — the WIP is actively
editing the retrieval-engine env block. Both read `std::env::var` with safe
defaults, so they work without compose entries; add them for discoverability
once that work lands.

### 2026-08-05 — P1-4 contradiction writer (D6 closed)

**Landed:** new `graph-index-rs/src/contradiction.rs` + `detect_claim_contradictions`
in `store.rs` + a post-ack trigger in `stream.rs`. Deployed, 74 unit tests
(10 new) + a DB integration test, clippy clean, 0 ERROR lines.

`contradicted_by_claim_ids` and `claim_status` had a **complete read path** —
`store::get_contradictions`, `GET /v1/graph/contradictions`, gRPC
`GetContradictions`, `retrieval-engine/search/contradictions.rs`, and Model Plane
`session-core::fetch_graph_segments` (which feeds flagged claims to the LLM as
context) — and no writer. Every consumer queried
`jsonb_array_length(...) > 0` against a permanently empty column. This is the
writer.

**Candidate selection, not all-pairs.** A claim is only compared against claims
sharing ≥1 entity, mirroring the `(entity, property)` key in the reference
implementation (`alash3al/stash`, Apache-2.0, `internal/brain/contradiction.go`).
Measured on the live 128 claims: **107 candidate pairs vs 8,128 all-pairs —
1.3% of the search space.** `MAX_CANDIDATES_PER_CLAIM = 200` bounds hub entities
("Aquatiq" links to most claims) from making one ingest quadratic.

**Detector: structural, high-precision, deliberately low-recall.**
1. *Negation asymmetry* — same assertion, opposite polarity
   ("anlegget er godkjent" vs "…er ikke godkjent"). Uses negation **parity**, so
   a double negation correctly cancels. Contractions are expanded first, so
   `isn't` compares equal to `is not`. Both corpus languages covered.
2. *Numeric divergence* — identical sentence skeleton, different figures
   ("below 4 degrees" vs "below 6 degrees"), gated on ≥5 shared tokens so
   `"price 4"` vs `"price 6"` is not trusted.

The precision/recall trade is intentional and tested as such
(`paraphrased_contradiction_is_missed_by_design`): for a citations product a
false contradiction flag is worse than a missed one, because it is surfaced to
the model as evidence the corpus disagrees with itself and pulls a human in to
adjudicate something that was never in conflict. Low recall is visibly
incomplete; low precision is quietly corrosive.

**No auto-supersede.** The reference implementation auto-demotes an older fact
when an LLM classifies the pair as a *replacement* with confidence ≥ 0.9. With
no confidence signal there is no safe threshold, so both claims stay live and
are only flagged. A semantic `ClaimAdjudicator` (Model Plane inference, 3-way
Replacement/Contradiction/Compatible) slots in behind the trait without touching
callers — that is the next increment.

**Safety properties verified:**
- Writes are **symmetric and additive** — both claims union the other's id
  (`jsonb_agg(DISTINCT …)`, never clobbered) and move to
  `claim_status = 'contradicted'`.
- **Retrieval is unaffected.** `GRAPH_CLAIM_SQL` in retrieval-engine selects
  `claim_status` but does **not** filter on it, so flagging is advisory and
  cannot silently remove content from answers.
- Candidates are restricted to org-visible live documents using the same join
  the read path applies: a claim sourced only from a private or deleted document
  is never used as evidence.
- Post-ack, like the community refresh — inside the ack window it would extend
  the JetStream deadline under bulk ingest and risk redelivery.

**Proven end-to-end against real Postgres.** The live corpus contains **no**
structurally-detectable contradiction (0 hits over those 107 candidate pairs),
so the write path could not be proven from production data. A new
`#[ignore]`d integration test (`GRAPH_TEST_DATABASE_URL`, isolated schema,
disposable `graph_test` DB) constructs the positive case and asserts:
detection count, symmetric flags, both statuses, untouched claims keep
`active`, **cross-tenant isolation**, private-document exclusion, the
pre-existing read path now returning the pair, and **idempotency** on re-run.
Verified passing, alongside the two sibling DB tests.

*Sibling note:* `neo4j::tests::dual_write_then_traverse_roundtrip_is_org_scoped`
still fails with `NEO4J_TEST_URL: NotPresent` — a pre-existing env gate, never
run against a real Neo4j. Not pointed at the live instance on purpose: it
writes, and that Neo4j is the serving read-model.

**Not done:** relation-type canonicalisation, and the `"contradiction"` lint kind
that `data-orchestrator/executor.go:261-266` forwards but
`data-quality/lint.go` never emits (dead pass-through, plan P1-9).

### 2026-08-05 — P0.5 golden set seeded

New: `services/retrieval-eval-py/` (was an empty directory) —
`seed_golden_set.py` + `README.md`. Stdlib-only, `--dry-run` by default.

**`eval_golden_judgments`: 0 → 60 rows.** Balanced 30 Norwegian / 30 English,
one `knowledge_id` answer each, single org.

Method is **known-item retrieval**: for each chunk, pick its most *distinctive*
terms (ranked `tf / (1 + df/N)` so corpus-wide boilerplate like "aquatiq" loses
to terms that actually single the chunk out) and record that chunk as the
answer. Measures "given a question drawn from chunk X, is X in the top 10" — a
real recall@10 / nDCG@10 / MRR signal replacing the
`min(candidates/10, 1)` proxy.

`normalize_query()` deliberately mirrors
`data-quality-go/internal/eval/golden.go:23`. A drift there means judgments
silently stop joining traces and everything falls back to the proxy.

Two bugs found and fixed while building it:
- A boilerplate filter was needed: the first run generated
  `"denne klikke klikk bildet teksten"` ("this click click the-image the-text")
  from image-caption furniture. A golden set built on page chrome reports
  confident numbers about nothing.
- `--apply` failed with `sh: syntax error: unexpected ")"`. Python's `repr()`
  switches to single quotes when a string contains a double quote and then
  backslash-escapes inner single quotes, which `sh` does not unescape the same
  way. Fixed with `shlex.quote()`, plus `jsonb_build_array('…')` instead of a
  JSON literal so no double quotes reach the shell at all.

**Honest limitation — seeded ≠ scored.** Verified: all 60 rows are loadable by
the runner's own query, but **0 of them match an existing `retrieval_runs`
trace** (116 traces exist, all from historical queries). The runner scores by
joining `golden[NormalizeQuery(trace.Query)]`, so these 60 queries must first be
*executed* through `/v1/retrieve` to produce traces. That path currently 401s
(`CONTROL_PLANE_ENFORCEMENT=strict`), so the instrument is **built and loaded
but not yet reading**. Same auth dependency as the reindex.

Also documented in the README: this is lexically biased and precision-blind by
construction. It detects regressions; it does not prove semantic quality. The
dimensions still to add are **consistency**, **false recall** (highest value for
a citations product), and **context-rot resistance**.

### P0 status

| Item | State |
|---|---|
| P0-1a durable consumers + DLQ | ✅ done, zero-loss proven live |
| P0-1b `Interest` retention migration | ✅ done, all 5 consumers bound |
| P0-1c backfill | ⛔ **blocked by design** — decision needed (see above) |
| P0-2 language-neutral FTS | ✅ done, verified in both languages |
| P0-3 page-image DLQ | ✅ done |
| P0-4 ColQwen no-op | ✅ done, verified |
| P0-5 failed embeddings | ⛔ rides with P0-1c |

**Next: P0.5 (golden set).** It gates every relevance change in P1/P2 — and
also gates knowing whether the `simple` FTS choice beats `norwegian` on real
queries.

---

### 2026-08-05 — P2-1 node-postprocessor chain

Extracted the post-fusion steps of `retrieval-engine-rs`
`RetrievalPipeline::retrieve` into an ordered chain of `NodePostprocessor`
stages in the new `src/pipeline/postprocess.rs`.

**The chain, in order** (the order *is* the policy):

| # | Stage | Kind | Notes |
|---|---|---|---|
| 1 | `visual_rerank` | refinement | ColQwen MaxSim; ZDR / missing client / <2 visual candidates / any error → input unchanged |
| 2 | `truncate:overfetch` | — | to `fetch_k` |
| 3 | `text_rerank` | refinement | cross-encoder; any failure → fused order, `rerank_used_count = 0` |
| 4 | `visibility_gate` | **enforcement** | liveness + per-user ownership; errors propagate |
| 5 | `truncate:top_n` | — | must come AFTER the gate |
| 6 | `zdr_filter` | **enforcement** | `Reject` drops `restricted`; records `zdr_actions_applied` |

**Why the order is not cosmetic.** The visibility gate must precede the
`top_n` truncation: truncating first would let a non-visible candidate consume
one of the caller's slots, so the response would undershoot *and* the slot
would be spent on a document the viewer may not read. ZDR must follow the
gate, so a document already dropped for visibility is not also counted as a
ZDR rejection. Both properties are now stated in the module docs, next to the
code that depends on them, rather than being implied by statement order in a
570-line function.

**Two motions, both deliberate and behaviour-neutral:**
- The viewer's grant lookup (`visible_documents`) moved *before* the chain. It
  depends only on the request, never on the candidate list, so it is an
  independent read.
- The cost-ledger publish moved *after* the chain, because
  `rerank_used_count` is now a chain output. The payload is unchanged: the
  later stages only drop candidates and cannot change how many the reranker
  scored.

**Trace fidelity was the real risk, and it is handled.** `sparse_ms` used to
mean "fusion + visual rerank" and `rerank_ms` "trim + cross-encoder call".
A single chain would have silently redefined both. `run_chain` therefore
returns a `StageTiming` per stage and the orchestrator re-sums them by name
(`elapsed_of`), so both fields keep their original meaning and stay comparable
with rows written before this refactor. Stage names are consts, so a typo is a
compile error rather than a silently-zeroed trace field.

`candidate_count_fused` is now computed *before* the visual stage instead of
after. Verified equivalent: `apply_colqwen_scores` preserves length on all
three of its paths (early return on empty/mismatched input, joint-mode
re-sort, band-mode write-back).

**Honest accounting of what improved.** `retrieve()` went 569 → 560 lines —
essentially unchanged, because ~165 lines of logic were replaced by ~120 lines
of chain construction. The win is not size:
- 6 stages that can each be constructed and driven against a hand-built
  candidate list. Previously every one of them required a live Postgres, a
  rerank provider, and a ColQwen endpoint to reach at all.
- 7 new unit tests, including the two that pin the confidence-gate contract
  (`rerank_used_count == 0` on both the absent-client and opted-out paths —
  reporting non-zero there is what previously made every query read as
  low-confidence).
- `orchestrator.rs` 1774 → 1603 lines; `postprocess.rs` +690 (roughly a third
  docs and tests).

**Verification:** `cargo fmt` clean; `cargo clippy --all-targets -- -D warnings`
clean; `cargo test` green — **149 lib tests pass, 0 failed**, including the 7
new stage tests. Integration tests pass, with the DB-gated ones
(`ownership_filter`, `zdr_behavior`, `pipeline_e2e`, `gdpr_erasure`) skipping
without `TEST_DATABASE_URL`, as designed.

Stale doc references to the two removed methods were repointed:
`tests/ownership_filter.rs` (×2) and the `apply_colqwen_scores` doc comment.

#### ⛔ Found while deploying P2-1: the Velion→Verevon rename breaks the GDPR erasure fan-out

Redeploying retrieval-engine surfaced a **cross-plane, compliance-critical**
regression that has nothing to do with P2-1. Reporting it here because this is
where it was found.

**The drift.** Every one of the 49 source files across all planes now says
`verevon.gdpr.erasure.requested`; **zero** still say `velion.*`. The *live*
broker is still provisioned for the old name:

| | Value |
|---|---|
| Stream `AQENCIA_CONTROLPLANE` subjects | `velion.gdpr.erasure.requested`, `velion.session.>`, `velion.agent.>`, `velion.org.deletion.>`, `velion.application.dlq.*`, `velion.gdpr.ownership.transferred` — **no `verevon.*` at all** |
| All 14 org-erasure consumers | `filter_subject = velion.gdpr.erasure.requested` |
| Source constant (e.g. `retrieval-engine-rs/src/gdpr/consumer.rs:56`) | `verevon.gdpr.erasure.requested` |

The stream subjects and consumer filters are deployment-provisioned by
`Control Plane/audit-core/internal/provisioner/provisioner.go`
(`ProvisionControlSharedRuntime`). Its **source** was renamed to `verevon.*`,
but the running `audit-core-service` image dates from 2026-07-21, so the live
broker still reflects the pre-rename provisioning.

**This is latent, and it activates per-service on rebuild.** Confirmed by
inspecting the running binaries and image timestamps: `org-core-service`
(image 2026-08-04) still has `velion.gdpr.erasure.requested` compiled in, so
today's fan-out still works for the 13 services running pre-rename images.
The renamed subject only takes effect in a service once that service is
rebuilt.

**Correction to my first read of this.** I initially assumed the failure was
pre-existing. It is not. `consumer.rs` was rewritten by the rename at 15:57
local; the previous retrieval-engine image was built at 14:51 local, so it
still carried `velion.*` and matched the broker. **My 17:36 rebuild is what
shipped the renamed subject**, and retrieval-engine is now the first — and
currently only — casualty:

```
ERROR retrieval-engine GDPR erasure consumer stopped; retrying
      error="pre-provisioned GDPR erasure consumer filter mismatch"
```

It retry-loops every 5s and consumes **no erasure events**. Verified that no
other DPv2 service was rebuilt after 13:57 UTC, so the blast radius today is
exactly one service.

**Why it matters.** Once Control Plane is rebuilt, `org-core`/`user-core`
publish to `verevon.gdpr.erasure.requested`, which the live stream does not
capture — so the event is not even stored, and the 14 consumers filtering
`velion.*` would never see it regardless. An org deletion would then purge
nothing in Data Plane, Model Plane, Ingestion, or Application. That breaks
constraint 4 (ZDR/GDPR propagation) and constraint 5 (policy metadata travels
with data).

#### Resolved 2026-08-05, in `apps/Control Plane/audit-core`

The user asked for this fixed directly, which crosses into Control Plane
source and running state. Three changes, in `internal/provisioner/provisioner.go`
plus tests, `cmd/nats-consumer-migrate` (new), and the Dockerfile:

1. **`ProvisionControlSharedRuntime`'s subject list now lists both names for
   every renamed subject on this bus**, not only the four GDPR ones — see
   below for why that widened mid-fix. `ensureControlStream` converges a
   stream's subjects to *exactly* what it is given (a wholesale replace, not
   a union — `TestProvisionIsIdempotentAndPreservesLegacyConsumer` pins this
   as intentional design, so that semantic was correctly left alone). The
   fix is additive at the literal level instead: eight `Legacy*` constants,
   each with a comment marking it for deletion once every publisher and
   consumer on the bus is confirmed on `verevon.*` source.
2. **`EnsureOrgErasureConsumer(js, stream, durable)`**, exported from
   `provisioner`: converges exactly one of the 14 named org-erasure
   consumers from its already-declared wanted config, independent of every
   other resource on the bus. Needed because `ProvisionControlSharedRuntime`
   stops at the first resource that fails to converge — see below.
3. **`cmd/nats-consumer-migrate`** (new binary, same image as
   `nats-provisioner`): given a stream and durable name, deletes the
   consumer if and only if it has zero pending and zero ack-pending messages
   (refuses otherwise — a hard safety gate, unit-tested), then calls
   `EnsureOrgErasureConsumer` to recreate it from current source. Self-healing:
   works whether the consumer currently exists with a stale filter or has
   already been deleted. Exists because `ensureFixedConsumer` correctly
   refuses to mutate a consumer whose config differs from wanted — a
   deliberate safety property, not a bug — so a consumer whose FilterSubject
   changed because the *source* was renamed never converges on its own.

**Applied to exactly one of the 14 consumers**, `retrieval-engine-gdpr-erasure-v1`
— the only one whose service has actually been rebuilt with `verevon.*`
source. Verified end-to-end: the stream now carries 21 subjects (13 original
+ 8 legacy-form duplicates for the renamed ones, since `application.dlq`/
`session`/`agent`/`org.deletion` needed the same treatment, not just GDPR);
all 13 untouched consumers still read `filter_subject: velion.gdpr.erasure.requested`
byte-for-byte; `retrieval-engine-gdpr-erasure-v1` now reads
`verevon.gdpr.erasure.requested`; and retrieval-engine-rs's own log went from
a 5-second retry loop straight to `retrieval-engine GDPR erasure consumer
ready` on its next cycle, with no container restart.

**Two mistakes made and corrected during this fix, recorded because both are
easy to repeat:**

- **First correction — the collateral damage was wider than GDPR.** The
  first version of this fix added `Legacy*` compatibility only for the four
  GDPR subjects. Running the (now-partially-fixed) provisioner nonetheless
  *silently dropped* `velion.session.>`, `velion.agent.>`,
  `velion.application.dlq.convex.controlplane`, and `velion.org.deletion.>`
  from the stream — `ensureControlStream`'s wholesale-replace semantics
  apply to the *whole* subjects slice, and those four subjects were also
  renamed in source but had no legacy pairing yet. Every publisher for
  session/agent/org-deletion is still on pre-rename source, so for the
  minutes this was live, those messages were not being persisted by the
  stream at all. Caught by re-reading the post-run subject list rather than
  assuming success from an idempotency test passing; fixed by extending the
  same `Legacy*` pattern to all four and rebuilding.
- **Second correction — the delete step outran the recreate step.**
  `nats-provisioner`'s `ProvisionControlSharedRuntime` returns on the first
  resource that fails to converge, and `control-shared-legacy-bridge` (an
  unrelated, pre-existing `_VELION.*` → `_VEREVON.*` DeliverSubject drift on
  a different consumer entirely — see below) sits earlier in that function
  than every GDPR consumer check. So deleting `retrieval-engine-gdpr-erasure-v1`
  and then re-running `nats-provisioner`, expecting it to recreate the
  consumer, left retrieval-engine with **no GDPR consumer at all** for
  several minutes — strictly worse than the original mismatch, since a
  missing consumer can't even retry. Caught by checking `consumer_count`
  after the "successful" run rather than trusting the exit code; fixed by
  adding `EnsureOrgErasureConsumer` so the migration tool never depends on
  unrelated resources converging first.

**Discovered, not fixed — same pattern, different consumers, out of the
scope actually asked for:** `control-shared-legacy-bridge` and
`billing-core-organization-plan-changed` fail the identical way
(`nats-provisioner` reports "exists with incompatible configuration;
refusing destructive replacement") because their `DeliverSubject` constants
were also renamed (`_VELION.CONTROL.SHARED.DELIVER.legacy` /
`_VELION.CONTROL.DELIVER.billing.organization-plan-changed`) and neither
consuming service has been rebuilt yet. `audit-nats-provisioner` currently
exits 1 on every run because of these two — confirmed pre-existing (identical
failure with every change in this section stashed out) and unrelated to
anything above. Same fix shape applies whenever those two services are
rebuilt: `EnsureOrgErasureConsumer`-style targeted migration, not a blind
provisioner re-run.

Delete the `Legacy*` constants and their four entries in the subjects slice
(all eight, not just the GDPR four) once every publisher and consumer on
`control-shared-nats` is confirmed on `verevon.*` source.

**Also fixed in passing:** my first redeploy used a bare
`docker compose up -d --no-deps retrieval-engine`, which resolved
`inter-plane-bus` through its base-file default name
(`${DPV2_CROSS_PLANE_NETWORK:-dpv2-cross-plane}`) and silently attached the
container to a fresh, empty, single-member network — cutting it off from
`control-shared-nats` entirely. **Recreating any single DPv2 service must use
the overlay**: `docker compose -f docker-compose.yml -f
docker-compose.cross-plane.yml up -d --no-deps <svc>`. Restored and verified
back on `dpv2-net` + `inter-plane-bus`.

**Not changed:** the confidence gate stays inline. It is a *verdict* computed
from the final list, not a transformation of it, so modelling it as a
postprocessor would have misrepresented what it does.

**P2-1's original scope listed `cutoff / decay / dedup / reorder` as stages
too.** Those are not in this chain, for concrete reasons: the similarity cutoff
landed in P1-7 as a Qdrant-native `score_threshold` (pre-fusion, in the dense
arm — moving it into a post-fusion stage would make it fire against
uncalibrated RRF scores); long-context reordering landed in P1-8 inside
`context_pack` (it reorders the *packed* context, not the candidate list);
recency decay landed in P2-3 (below) as a `NodePostprocessor` in this same
chain; and there is no dedup step in the pipeline today.

---

### 2026-08-05 — P2-3 `document_date` + recency decay (closed)

Spans two planes: Data Plane v2 (schema, embedding pipeline, retrieval) and
Ingestion Plane (`finspo-core`'s SharePoint connector). Five layers, in the
order data actually flows:

1. **Migration** (`20260805160000_document_date.sql`): `documents.document_date
   TIMESTAMPTZ`, nullable, no default — "unknown" must stay distinguishable
   from "known to be old," since the decay stage treats them oppositely. A
   one-time backfill joins `source_objects.modified_at` onto
   `type='sharepoint_file'` documents via `(org_id, drive_id, item_id)` — the
   only document type whose `metadata` carries that pair.
   `sharepoint_page`/`web_page` documents have no matching join key and stay
   NULL until re-ingested through the now-fixed connector. Live result:
   1/1 sharepoint_file backfilled; 175 sharepoint_page + 13 web_page correctly
   untouched.
2. **documents-api-go**: `CreateDocumentInput.DocumentDate` threads through
   both INSERT paths and both UPDATE (content-refresh) paths. The refresh
   paths use `COALESCE($N, document_date)`, not a blind overwrite — a caller
   that doesn't know this field (most callers, still) must not blank out a
   previously-known date on an unrelated re-ingest.
3. **finspo-core** (`internal/dataplane/documents.go` +
   `internal/content/{ingestor,pages_ingestor}.go`): `CreateDocumentInput`
   gains `ModifiedAt`, forwarded as `document_date` — a DIFFERENT wire name
   than `SourceObjectClient`'s existing `modified_at`, because they are
   different tables with different meanings (this row's content freshness vs.
   a file's own change-detection bookkeeping). Site pages prefer their own
   `LastModifiedDateTime`, falling back to the wrapping drive item's when the
   Pages API omits it.
4. **embedding-engine-rs**: `BatchItem.document_date`, sourced from
   `documents` in the SAME per-chunk query that already reads
   `zdr_classification` (one round trip serving both document-level facts,
   not two). Threaded into `EmbeddingPoint.metadata` as a plain RFC3339
   string — `qdrant_writer`'s existing generic passthrough (`for (k, v) in
   p.metadata { payload.insert(k, StringValue(v)) }`) already handles it; no
   new payload-writing code needed.
5. **retrieval-engine-rs**: a new `RecencyDecay` `NodePostprocessor` (the P2-1
   chain's first real payoff — this would have been ~40 more inline lines in
   `retrieve()` before that refactor). Exponential half-life decay on
   `final_score`, reading `document_date` out of the candidate's own Qdrant
   payload (`ScoredCandidate.metadata` already carries it — no new Postgres
   join). `RECENCY_DECAY_ENABLED` defaults **false**: this changes ranking for
   every query and has not been measured against the P0.5 golden set yet.
   Placed AFTER `text_rerank`, not before — the reranker overwrites
   `final_score` outright with its own cross-encoder score, so decay applied
   earlier would be erased the instant a reranker is configured and working,
   which is the common case in this stack. A missing/unparseable/future
   `document_date` is left UNCHANGED, never penalized: the corpus is
   overwhelmingly undated until re-ingestion catches up, and treating unknown
   as maximally-old would bury most of it.

**Verified:**
- Go: `cargo`-equivalent (`go build`/`go vet`) clean; full `-tags integration`
  suite (testcontainers-postgres, real disposable Postgres) green, including a
  new test proving the COALESCE preserve-on-refresh behavior end-to-end.
- Rust (embedding-engine-rs): `cargo check`/clippy clean; all 41 pre-existing
  tests still pass (this module's `process_batch` has zero unit tests of its
  own — it is inherently integration-level, matching existing convention; not
  a gap this change introduced).
- Rust (retrieval-engine-rs): `cargo check`/clippy/fmt clean; 156 lib tests
  pass, including 7 new ones covering the decay curve, the missing/
  unparseable/future-date non-penalty cases, the non-positive-half-life guard,
  and a reordering test proving decay can promote a fresher low scorer above a
  stale high scorer.
- Live: migration applied to the running Postgres, backfill counts confirmed
  by direct query. `documents-api`, `embedding-engine`, `retrieval-engine`
  rebuilt and redeployed (with the cross-plane overlay); all three healthy.
  The exact new `stream.rs` SELECT was run directly against the live
  `documents` table and returned the expected two-column shape with a real
  TIMESTAMPTZ value.
- **Not driven end-to-end live**: no real SharePoint sync ran during this
  session, and no synthetic signed NATS event was fabricated to force one, so
  the full connector→Qdrant-payload round trip was not observed on the wire.
  The remaining risk is narrow and low: sqlx decoding `TIMESTAMPTZ` into
  `Option<DateTime<Utc>>` and Go's `encoding/json` respecting a struct tag are
  both the same well-established mechanism already used identically
  elsewhere in each of these exact files (e.g. `embedded_at`, `Metadata`) —
  not new custom logic — but it is still an honest gap between "verified" and
  "fully proven on the wire."

**Self-inflicted incident, found and fixed mid-task:** rebuilding
`documents-api` for this change (unrelated to GDPR) triggered the *same*
Velion→Verevon subject-drift landmine as retrieval-engine's earlier fix
(1478b3fb) — except `documents-api-go` treats a mismatched pre-provisioned
GDPR consumer as `log.Fatal`, so it crash-looped rather than retry-looping.
Fixed the same way: `nats-consumer-migrate` against its two consumers
(`documents-api-gdpr-erasure-v1`, `documents-api-org-erasure`), both
confirmed zero-backlog first. Recovered within one migration cycle; no
message loss. This is exactly the recurrence the earlier fix's memory note
predicted — every one of the other 12 GDPR consumers will hit the same thing
the moment its own service is next rebuilt for an unrelated reason, not only
when someone deliberately revisits the rename.

### 2026-08-05 21:19 CEST — GDPR consumer migration, requested directly: 2 more done, 9 correctly left alone

Asked to migrate "the other 12" remaining consumers. Checked first, rather
than executing the count as given: migrating a consumer is only safe once its
*owning service's currently-running image* was rebuilt after the rename
commit (`0c2d4358`, 2026-08-05T16:49:11+02:00) — migrate one whose service is
still pre-rename and it breaks a consumer that was working a moment earlier,
which is exactly what happened to `documents-api` two sections up.

Checked every remaining consumer's owning service by image `Created`
timestamp, then binary-verified two of them directly (`grep` the running
executable for the compiled-in subject string, via `docker top` to find the
real PID since PID 1 is `docker-init`, not the app) rather than trust the
timestamp alone. Result: only 2 of the 11 still-`velion.*` consumers had an
owning service actually rebuilt post-rename —
`embedding-engine-org-erasure` (rebuilt this session for P2-3) and
`conversation-core-org-erasure` (rebuilt by a concurrent session, for a
reason unrelated to any of this). Both confirmed zero backlog, migrated with
`nats-consumer-migrate`, both landed atomically this time — no repeat of the
delete-without-recreate gap, since `EnsureOrgErasureConsumer` runs inside the
same tool invocation now.

**State: 5 of 14 migrated** (`retrieval-engine-gdpr-erasure-v1`,
`documents-api-gdpr-erasure-v1`, `documents-api-org-erasure`,
`embedding-engine-org-erasure`, `conversation-core-org-erasure`). The
remaining 9 (`cost-core-org-erasure`, `data-orchestrator-org-erasure`,
`data-quality-org-erasure`, `graph-index-gdpr-erasure-v1`,
`index-engine-org-erasure`, `quarry-control-org-erasure`,
`quickwit-adapter-gdpr-erasure-v1`, `session-core-gdpr-erasure-v1`,
`wiki-store-org-erasure`) all binary- or timestamp-confirmed still pre-rename
— deliberately not touched. None of those 9 services were rebuilt to make
this number go up faster: that would be nine unrelated rebuilds across three
other planes, a much larger action than "migrate the GDPR consumers" asked
for. Each becomes safe to migrate the moment its own service is next rebuilt
for its own reason — exactly how the 5 done so far each became safe.

### 2026-08-06 — P2-6 Dragonfly-backed rate limiter (closed)

Replaced the in-process `governor::RateLimiter` in `rate_limit/mod.rs` —
correct for one replica, wrong the moment there's more than one, since each
replica held an independent bucket and the *effective* per-org limit
silently scaled with replica count. New `PerOrgLimiter` keeps bucket state
in Dragonfly (already this service's cache backend) and evaluates it with a
Lua `EVAL` script (`TOKEN_BUCKET_SCRIPT`) so concurrent replicas checking the
same org cannot both observe the same pre-decrement token count. Refill is
continuous (elapsed-time-proportional), not fixed-window, so there's no
window-boundary double-burst. `governor` dropped from `Cargo.toml` — no
longer used anywhere in the crate.

New env vars: `DPV2_RATE_LIMIT_PER_ORG_RPS` (default 20),
`DPV2_RATE_LIMIT_PER_ORG_BURST` (default 40); reuses the existing
`DRAGONFLY_URL`/`CACHE_URL` lookup order. Fails open on any Dragonfly error
(unreachable, connection refused, script failure) — a rate limiter is a
fairness mechanism, not a security boundary, matching this crate's existing
`CacheLayer` convention of degrading to a no-op rather than rejecting
traffic.

**Two real bugs found by testing, not shipped:**
1. `redis::aio::ConnectionManager::new()` does not fail fast against an
   unreachable address — measured empirically at **473s** to exhaust its own
   internal reconnect attempts. Without an outer bound, "fail open" would
   have meant an ~8-minute stall per request through a dead Dragonfly,
   indistinguishable from a total outage. Fixed with a 250ms
   `tokio::time::timeout` (`PerOrgLimiter::BACKEND_TIMEOUT`) around the whole
   connect-and-check round trip, not just the script call.
2. The original 3 unit tests drove config through `std::env::set_var`/
   `remove_var`, which is process-global — Rust's default parallel test
   runner raced two tests' env vars mid-read, occasionally computing a
   nonsensical TTL. Fixed by extracting `PerOrgLimiter::with_config(rps,
   burst, redis_url)` as a pure, explicitly-parameterized constructor and
   rewriting all 3 tests to call it directly, with zero env-var mutation.

**Verification:**
- `cargo test --lib rate_limit::` — 3/3 pass in 0.38s (was hanging to 473s
  before fix #1).
- **Live, against the real deployed Dragonfly** (not a mock): ran the exact
  `TOKEN_BUCKET_SCRIPT` via `redis-cli EVAL` inside
  `data-plane-v2-dragonfly-1`, keyed on a throwaway
  `dpv2:ratelimit:__p26_live_verify__` bucket, rps=5/burst=5 — 6 rapid calls
  returned `1,1,1,1,1,0` exactly as the token-bucket math predicts, then a
  7th call after a 1.2s sleep returned `1` (refill confirmed); test key
  deleted afterward. Proves the atomic script logic is correct against
  Dragonfly specifically (not just generic Redis), which is the actual novel
  risk in this change.
- Confirmed route ordering live: `/health` → 200 without auth (outside the
  `authed` router, never touches the limiter); `/v1/knowledge/search` → 401
  without a bearer token (`auth_middleware` rejects before the request can
  reach `per_org_rate_limit`, matching the documented `route_layer`
  ordering).
- Redeployed `retrieval-engine` with the cross-plane overlay; container
  healthy, no startup errors; GDPR consumer and every other subsystem came
  up normally.
- **Gap, explicitly flagged, not silently skipped**: did not observe a real
  `429` through the HTTP middleware chain with a genuine org JWT — that
  needs a real auth-core-issued bearer token, and this session has no live
  user/org credentials to mint one. What that gap would additionally cover
  (org_id extraction from `AuthContext`, HTTP status mapping) is simple,
  pre-existing, unchanged-by-this-phase code; the part that is actually new
  and risky — the distributed token-bucket algorithm itself — is the part
  verified live above, against the real backend.

### 2026-08-06 — P2-7 HyDE / query-expansion blend (D14 closed)

`RetrievalRequest::query_expansion` (D14) was accepted on HTTP and gRPC but
consumed by nothing. DPv2 must not generate this text itself — producing a
hypothetical-document expansion is a reasoning call, and constraint 3/Rule 7
reserve reasoning for Model Plane. So this phase only wires up
*consumption*: if a caller supplies `query_expansion`, blend its embedding
with the literal query's; if not, behavior is byte-for-byte unchanged.

Rejected the alternative of adding a second fused arm to `fuse_arms` (RRF
over a 4th ranked list) — much larger surface, more test-sensitive, and the
embedding-blend approach gets the same "the expansion should steer
retrieval" effect at lower risk. Implementation: extracted the existing
query-embed block into `embed_text_cached` (cache lookup/store, ZDR-gated
cache admission, `embed_query`, empty/dimension validation — unchanged
behavior, just named and reusable), then call it twice when
`query_expansion` is present (trimmed, non-empty) and blend with a new free
function `blend_vectors(query, expansion, weight)` — `query * (1 - weight) +
expansion * weight` per dimension. Safe without renormalization because all
DPv2 Qdrant collections use `Distance::Cosine`, which is scale-invariant. New
config `query_expansion_blend_weight` (default 0.5, `config.rs`) — no
separate enabled flag, since a caller populating the field is itself the
opt-in. Added a `MAX_QUERY_LEN` bound on `query_expansion`, matching the
existing query-length guard.

**Verification:**
- `cargo test` (full lib suite) — 4 new unit tests: weight=0 returns the
  query vector unchanged, weight=1 returns the expansion vector unchanged,
  weight=0.5 is the exact per-dimension midpoint, and a mismatched-length
  pair truncates to the shorter input rather than panicking. All pass.
- Deployed in the same rebuild as P2-6; container healthy, started with no
  errors.
- **Gap, explicitly flagged, not silently skipped**: did not drive a real
  `query_expansion` value through a live HTTP or gRPC request end-to-end.
  Both `/v1/retrieve` (HTTP) and the gRPC `Retrieve` RPC require a bearer
  token authorized through auth-core's real `AuthContext` flow — confirmed
  by reading `grpc/retrieval_svc.rs`'s `authorize()`, and by a live 401
  against `/v1/retrieve` with only an `X-Org-Id` header (the existing
  `tests/pipeline_e2e.rs` scaffold's comment assumes an open dev mode that
  does not match this stack's actual `control-plane enforcement: strict`
  config — worth reconciling separately, not part of this phase). Minting a
  genuine token needs a real Control-Plane user/org login; fabricating one,
  or spinning up a throwaway signup, is a disproportionately large and
  riskier action for verifying a vector blend, so it was not attempted. What
  is covered instead: the blend math itself (unit-tested exhaustively
  above), plus the service compiling, linking, and starting cleanly with it
  wired in. The one path unit tests cannot reach — whether `embed_text_cached`
  called twice against the real Model Plane embedding backend behaves as
  expected under real network conditions — is unverified live, the same
  class of gap as P2-3's connector→Qdrant wire proof.

---

## 7. Architecture constraints this plan honours

1. No direct database crossing between planes.
2. No independent embeddings/reranking outside isolated labs — embedding routes
   through Model Plane `inference-core` (`EMBEDDING_PROVIDER=model_plane`).
3. Model Plane proposes browser actions; Quarry executes or rejects (kills the
   CRAG-triggers-browser design).
4. ZDR propagates through every content-persisting boundary.
5. GDPR policy metadata travels with data.
6. Durable knowledge assets are Data-Plane-owned.
7. Reasoning — including per-user conversational memory — is Model-Plane-owned.
