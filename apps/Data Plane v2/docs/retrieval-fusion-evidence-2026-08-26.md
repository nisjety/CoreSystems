# Retrieval fusion & rerank evidence — 2026-08-26

Every tunable named here is set by measurement, and this file is the record the
config comments point at. Re-run `make eval-retrieval` before moving any of
them; the instrument and its known noise floor are documented below so a future
run is comparable.

## Instrument

- **Golden set v2**: 87 hand-authored judged queries (`scripts/eval-build-golden-set.py`),
  a verified strict superset of the original 28. Hand-authored deliberately:
  model-generated questions inherit the source document's vocabulary, which
  inflates lexical retrieval specifically and would bias any `w_bm25`
  measurement. Two eval orgs hold the identical 1,164-chunk corpus —
  `org-corpus-baseline` (raw chunks) and `org-corpus-contextual` (contextual
  retrieval applied, 97.9% coverage).
- **Metrics**: recall@10 / nDCG@10 / MRR via the corrected `goldenMetrics`
  accounting (one gain per judged id at its first rank — the deployed formula
  previously saturated at 1.0 and hid its own signal).
- **Noise floor** (measure it, don't assume it): one query = 1.15 recall
  points. Re-running an identical cell moved recall by one query and nDCG by
  ~0.008 **when the reranker was off or fully failing**. With the reranker on,
  cell-to-cell variance rises to **±0.04 nDCG**, because ~19% of queries lose
  the cross-encoder to provider 429s per run (the "rerank lottery" — which
  queries lose it differs per run). Any single-pair comparison smaller than the
  applicable noise is not a finding.

## Findings that survived confirmation

1. **Contextual retrieval helps, everywhere.** +0.033..+0.048 nDCG and
   +0.040..+0.061 MRR over the raw org at every fusion weight tested (five for
   five on sign), and it holds with the reranker both on (+0.028 MRR pair) and
   off. It is ON for this deployment (`.env`: `CONTEXTUAL_RETRIEVAL_ENABLED`),
   one `gpt-4o-mini` call per chunk at ingest, with `Retry-After`-aware retries
   so rate limits cost latency instead of coverage.

2. **`w_bm25 = 0.1`.** Sweeping 0.0→0.4 (87 queries, both orgs, all arms
   live): recall statistically flat across the whole range, nDCG declines
   monotonically (~5 points 0.0→0.4) in both orgs independently. On
   natural-language questions the lexical arm re-surfaces documents the dense
   arm already found, ranked worse. 0.1 rather than 0.0 because the golden set
   under-represents exact-term and Norwegian-morphology lookups — the queries
   BM25 exists for. (Caveat: the sweep predates the rerank fixes, so it
   describes the fused order the reranker consumes, not the post-rerank list.)

3. **The cross-encoder was silently dead; it is now live and worth +0.025 nDCG
   on raw corpora.** Rerank failure deliberately degrades to fused order, so
   nothing logged loudly when Azure Foundry S0 429'd nearly every call: local
   backoff (300/600ms) sat inside the provider's 1-second window, the sharded
   path (`rerank_single`) had **no retry at all**, and six fused arms × top_k
   routinely exceeded the 50-candidate split — so sharding was the common path.
   Measured 88–99% of queries degrading. Fixes, all deployed:
   - honor `Retry-After` (capped at 5s; only ever lengthens the local backoff),
   - retries inside every shard, with staggered shard starts,
   - **`RERANK_TOP_K=50`**: rerank a window, not the world — one provider call
     per query, one comparable scoring pass. Degradation went ~90% → 19% under
     burst eval load; production pace should be near zero.
   Measured contribution (rr-on vs rr-off): baseline org +0.0245 nDCG / +0.0297
   MRR (7 queries up, 2 down); contextual org +0.005 (wash — contextual
   embeddings already did the ordering work). Latency cost under 429 pressure:
   p50 170ms → ~3.4s; at a provider tier that doesn't throttle, the window
   makes the cost one call.

4. **No auxiliary-arm weight change is justified — and one dramatic finding was
   successfully killed by its confirmation run.** Leave-one-out cells for
   graph/wiki/visual/keyword all landed within the ±0.04 rerank-lottery noise.
   One cell (dense+bm25 only, "floor") initially beat the full blend by +0.064
   nDCG with a 15/3 per-query count — a cache-busted re-run collapsed it to a
   wash (floor 0.7326 vs full 0.7531) while the full blend reproduced within
   0.008. The 15/3 count was the lottery, not the arms. The blend therefore
   stays: dense .45 / bm25 .1 / graph .2 / wiki .1 / visual .2 / keyword .05.
   Note the eval corpus cannot value wiki/visual/keyword arms anyway (it has no
   wiki pages, images, or keyword-style queries) — a measured case for zeroing
   them would be overfitting to this corpus even if it had survived.

## What this leaves open, in priority order

1. **Deterministic rerank coverage for the eval.** The lottery is the dominant
   noise source (±0.04). Either a provider tier that doesn't 429, or an eval
   mode that retries until reranked, would cut cell noise ~5x and make per-arm
   attribution measurable.
2. **Keyword-style and Norwegian-morphology queries in the golden set** — the
   lexical and keyword arms' upside is structurally invisible today.
3. **Content-bearing corpora** (wiki pages, page images) before judging
   `w_wiki` / `w_visual` — the current numbers only bound their cost on
   text-only corpora (≤ noise), not their value.
4. **Agentic tool routing** (14 typed retrieval tools, ~1 used) — Model Plane
   concern, unchanged; the audit's highest-confidence priority.
5. **CI gate**: `make eval-retrieval` exists; nothing blocks a regression yet.

## Config in force (2026-08-26)

| knob | value | where | evidence |
|---|---|---|---|
| `W_BM25` | 0.1 | compose default | sweep, finding 2 |
| `RERANK_TOP_K` | 50 | compose default | finding 3 |
| rerank retry | Retry-After-aware, per-shard, staggered | `search/rerank.rs` | finding 3 |
| `CONTEXTUAL_RETRIEVAL_ENABLED` | true | `.env` | finding 1 |
| `W_DENSE/W_GRAPH/W_WIKI/W_VISUAL/W_KEYWORD` | .45/.2/.1/.2/.05 | code default / `.env` | finding 4 (no change justified) |

---

# Addendum — capability census, 2026-08-26 (round 4)

Rounds 1–3 optimized what the eval could see. This round audited what it
cannot: which arms have data at all, and which wired-but-dormant surfaces are
actually reachable.

## Arm data census — 3 of 8 arms have data

| arm | store | rows/points | state |
|---|---|---|---|
| dense | `dataplane_knowledge_embedv4` (Qdrant) | 2,328 | **live** |
| sparse | `dataplane-corpus` (Quickwit) | 1,164 / 8,509 | **live** (contextual org 7.31x duplicated) |
| keyword | `dataplane-knowledge` (Meilisearch) | 2,328 | **live** |
| graph | `graph_entities` / `graph_relationships` (Postgres) | 0 / 0 | empty — see below |
| wiki | `wiki_block_embeddings` (Qdrant) | 0 | empty (no wiki pages ingested) |
| visual | `dataplane_page_images` (Qdrant) | 0 | empty (needs Ingestion-Plane PageRenderer) |
| audio | `dataplane_audio_segments` (Qdrant) | 0 | empty, `w_audio=0` |
| video | `dataplane_video_segments_siglip2` (Qdrant) | 0 | empty, `w_video=0` |

**`w_graph` (0.2) + `w_visual` (0.2) = 40% of nominal blend weight is assigned
to arms that cannot return a candidate on this deployment.** This does not
corrupt results — `fuse_arms` skips an arm whose candidate list is empty, so the
weights simply never apply — but it makes the mix recorded in every trace
misleading, and it means round 3's leave-one-out study measured four arms with no
data. Their deltas were not "within noise"; they were **structurally zero**, and
what the study actually recorded was 100% rerank lottery. The round-3 conclusion
(no auxiliary weight change justified) stands, for a far more definite reason.

Correction to round 3: the per-arm liveness probe there was invalid for
graph/wiki/visual/keyword. `EngineRoute::from_weights` contains
`if !dense && !sparse { dense = true }` ("never route to nothing"), so isolating
an auxiliary arm silently falls back to the DENSE arm — the uniform "10
candidates" read as liveness was dense answering every time. Only the dense and
sparse rows were meaningful. Verify arm liveness by counting rows in its store,
not by weighting it alone.

## Why the graph is empty (not a bug)

Graph extraction ran on all 190 documents and reported
`entities=0, relationships=0, claims=0` in ~3ms each. Cause: every document is
`visibility='private'`, and `load_org_visible_chunks` reads only
`visibility='org'`. That filter is **correct and must stay** — graph entities are
org-shared, so extracting a private document would leak its content org-wide
through the graph arm.

What was wrong was the silence: "complete, entities=0" is indistinguishable from
extraction failing, and the `all_failed` DLQ guard is gated on
`!chunks.is_empty()` so it stayed quiet too. Now logged explicitly as a policy
skip, and a DB error is no longer collapsed into "no chunks" by
`unwrap_or_default()`.

To populate the graph arm: ingest with `visibility='org'`, or re-classify
existing documents deliberately (an authority decision, not a config tweak).

## Fixed this round

1. **Document text was written to logs on every cache hit.**
   `#[tracing::instrument]` on `respond_from_cache` skipped only `self`/`req`, so
   the `cached: CachedRetrieval` argument — every candidate's full `text` plus the
   packed context — was recorded via `Debug` at INFO. 1 of 12 instrument sites in
   the service; every other one correctly records counts only. Restrictive ZDR
   postures never populate that tier, so exposure was non-ZDR org content —
   still org-scoped material in an operator-readable stream.
2. **`data-quality` and `data-orchestrator` rejected every service-principal
   token** with a silent 401. Both required `user_id == sub`; auth-core's
   `issuePlaneToken` mints `sub` + `service_id` + `principal_type: service` and no
   `user_id`. Ported wiki-store-go's discriminator verbatim (which is *stricter*
   for service tokens: consistent principal_type, `sub == service_id`, no
   `user_id`, non-empty scopes). **This is why `eval_golden_judgments` and
   `quality_eval_runs` sat at zero rows** — and for the orchestrator it meant
   unattended sweeps and reindex jobs were unreachable by their intended caller.
3. **Graph extraction skip is now explicit** (above).

## In-service eval harness: now exercised

The audit's load-bearing gap is closed *in the service*, not only in
`scripts/`: `eval_golden_judgments` 0 → **87**, `quality_eval_runs` 0 → **1**
(`completed`). Its own scorecard, on the deployed config:

| source | recall@10 | nDCG@10 | MRR |
|---|---|---|---|
| in-service (`quality_eval_runs`, 100 traces scored) | 0.8500 | 0.7168 | 0.6747 |
| local scorer (`scripts/`, 87 queries) | 0.8851 | 0.7452 | 0.6999 |

Two independent implementations of the same corrected metric agreeing within the
known rerank-lottery noise is the strongest validation the harness has had.

## Still dormant / open

- **Semantic cache**: `SEMANTIC_CACHE_ENABLED=true` but the Qdrant collection
  `dataplane_semantic_cache` **does not exist** — the tier is dead exactly as the
  original audit reported. Either provision the collection or turn the flag off;
  a flag that claims a live cache and has no store is the worst of both.
- **Quickwit duplication**: contextual org at 7.31x (1,164 chunks → 8,509 docs),
  grown by repeated re-announces. Query correctness is preserved by `dedup_hits`,
  but the index needs a rebuild and the adapter still has no synchronous delete.
- `context_pins` (CAG), `agent_retrieval_configs`, `space_retrieval_bindings`:
  0 rows — built, never used.
- HyDE / query expansion: blend math real, no caller (Model Plane owns it).
- Agentic tool routing: 14 typed tools, ~1 used. Unchanged, still the audit's
  highest-confidence priority.
- Recency decay / temporal: off by default, never validated.
- CI gate on `make eval-retrieval`: still absent.

---

# Addendum — round 5: closing the open items

## Semantic cache — verified working, no change needed

Round 4 called this "dead". That was wrong, and the check behind it was looking
at the wrong collection (`dataplane_semantic_cache`; the real default is
`semantic_response_cache`). Driven end to end for the first time:

| step | result |
|---|---|
| search on empty tier | clean MISS, no error, collection not created by looking |
| store | 200, collection lazily created |
| search same prompt | HIT, score 1.000 |
| search a paraphrase | HIT, score 0.967 — genuinely semantic, not exact-match |
| search with a different `scope_key` | MISS — no cross-principal leak |

`min_score` 0.95, TTL 24h, and `semantic_cache_require_scope` defaults to
**true** (a caller that omits a scope fails closed). The tier is a correct
external seam awaiting its Model Plane caller, not a defect.

## Retrieval quality gate — `make eval-gate`

`scripts/eval-gate.py` + committed `scripts/eval-baseline.json`. Exits 1 when a
metric regresses past the measured noise floor, or when the eval did not really
run (fewer than 80 queries scored, or >2% of queries returning nothing — the
signature every silent-stage failure here has had). Tolerances are deliberately
loose (recall 0.030, nDCG/MRR 0.045) because a gate tighter than the
instrument's ±0.04 rerank-lottery noise would be flaky, and a flaky quality gate
gets disabled. Verified in both directions: passes on the committed baseline,
and exits 1 on a synthetic regression (dropped judged docs) and on a
synthetic dark stage (all-empty results). `make eval-check` runs measure → score
→ gate.

## Graph arm — lit up, and two more bugs on the way

Flipped the 12 most-judged documents in `org-corpus-baseline` to
`visibility='org'` (reversible; ids in the eval scratchpad). Extraction then ran
for real: **273 entities, 87 relationships, 72 claims, 492 text-unit mappings**,
with genuinely useful structure — e.g. `stale-embedding detection` (Process,
conf 0.8) --performs--> `data-orchestrator-go`.

Two defects surfaced doing it:

1. **A third and fourth instance of the conjunction bug.** All three graph FTS
   sites and the contradictions claim search used `plainto_tsquery`, which ANDs
   every word. Entity texts are short phrases like `"embeddings"`, so a
   five-word question could not match an entity *even when an entity by exactly
   that name existed* — measured 0 rows AND vs 3 rows OR. This is why the
   contradiction category was never emitted, too. All four now use
   `websearch_to_tsquery` over an OR-joined term list, and the sanitizer is
   consolidated into `search::textquery` so there is one definition instead of
   four that can drift.

2. **The envelope-expiry data-loss bug, again — in graph-index this time.**
   Extraction is one inference call PER CHUNK (~30s/document), so a backlog of
   more than about two documents always outlives the 120s envelope TTL. Of 12
   documents announced, 2 extracted and **10 were dropped and acked**, logged as
   `rejected unauthorized graph event` — which reads as an attack rather than
   latency. This consumer could never have populated the graph under any real
   load. `EnvelopeError::Expired` is now distinguished from a forgery (it is
   produced only after signature, issuer, scope and digest have all passed) and
   logged as an actionable ERROR naming the `document_id` to re-announce.
   Dropping remains the behaviour — accepting expired envelopes would weaken a
   real replay control and this subject has no reconciler — so the durable fix
   is one of: a reconciler for `dataplane.documents.indexed`, a fresh envelope
   per chunk, or an explicit decision to trust `jti` replay protection instead.

Post-fix, the dedicated `/v1/retrieve/graph` endpoint returns real entities and
relationships, and the graph arm changes the FUSED result set on 2 of 8 queries
whose judged document was extracted (it previously changed nothing, because the
store was empty). That establishes participation, not quality: extraction covers
12 of 95 documents, so `w_graph` still cannot be tuned on evidence.

## Correction to round 4's arm census

Round 4 said 3 of 8 arms have data. With graph now populated for 12 documents,
it is 4 of 8 — dense, sparse, keyword, graph(partial). Wiki, visual, audio and
video remain empty.

## Deliberately not done

**Lighting up the wiki arm requires granting `wiki.write` / `wiki.approve` to
the `corpus-seeder` service principal** in the Control Plane credential
registry. Expanding a principal's authority is not a config tweak and was left
for an explicit decision rather than done silently. Everything else needed for
that arm is in place: wiki-store is healthy, accepts service principals, and
`embedding-engine`'s wiki consumer writes `wiki_block_embeddings` on
`dataplane.wiki.version.published`.

---

# Addendum — round 6: the lexical arm was measured on the wrong queries

## Headline: `w_bm25` is worth +0.37 nDCG — on the queries it exists for

Rounds 2-3 swept `w_bm25` against 87 long natural-language questions and found
it only ever *cost* nDCG. That conclusion was correct for that query class and
badly incomplete, because the set contained **zero** queries of the kind a
lexical index exists to serve: 0 queries of 3 words or fewer, no identifier
lookups, no config keys.

`scripts/eval-mine-lexical-queries.py` mines those queries instead of inventing
them: tokens matching identifier shapes (snake_case, SCREAMING_CASE, paths,
crate names) that occur in **exactly one document** in the corpus — so the
judgment is correct by construction rather than by my belief. 1,381 unambiguous
candidates; 25 selected, ranked for realism (compound domain identifiers such as
`idx_ku_content_tsv_gin`, `COHERE_EMBED_V4_API_KEY`, `pg_try_advisory_xact_lock`,
`INDEX_ENGINE_PARENT_CHUNK_SIZE` — not 60-character paths nobody types, and not
accidentally-unique generics like `mod.rs`).

Sweep on that set (baseline org, rerank OFF to isolate fusion):

| w_bm25 | recall@10 | nDCG@10 | MRR | found |
|---|---|---|---|---|
| 0.0 (dense only) | 0.8000 | 0.6147 | 0.5553 | 20/25 |
| 0.1 (static default) | 0.8400 | 0.7660 | 0.7440 | 21/25 |
| 0.3 | 0.8400 | 0.7888 | 0.7740 | 21/25 |
| **0.6** | **1.0000** | **0.9852** | **0.9800** | **25/25** |
| 1.0 | 1.0000 | 0.9852 | 0.9800 | 25/25 |

**+0.3705 nDCG and +0.2000 recall** over dense-only; five queries go from missed
to found. An order of magnitude larger than any effect measured on the
natural-language set, and far outside the noise floor.

## But the system already handles it — and the eval had been suppressing that

Weight precedence is **request `mode_mix` > agent config > smart hybrid >
static defaults**. Every measurement in rounds 2-5 sent an explicit `mode_mix`,
which by design *disables* `pipeline/smart_mix.rs`. So all of it measured the
static fallback, not what a real caller gets.

`smart_mix` raises `w_bm25` to 0.45 and `w_keyword` to 0.15 when a query is
code-like (`looks_code_like`), <= 6 tokens, and not a question. Run the same 25
queries with NO `mode_mix`:

| config | recall@10 | nDCG@10 | found |
|---|---|---|---|
| explicit `w_bm25=0.1` (static) | 0.8400 | 0.7660 | 21/25 |
| explicit `w_bm25=0.6` (hand-tuned) | 1.0000 | 0.9852 | 25/25 |
| **no `mode_mix` — smart hybrid decides** | **1.0000** | **0.9652** | **25/25** |

The adaptive layer reaches the hand-tuned optimum on its own. So:

* **`W_BM25 = 0.1` is correct and should stay.** It is the *static fallback*,
  which applies to natural-language queries — exactly the class it was measured
  on and is right for. Identifier queries never use it, because smart hybrid
  overrides first.
* The audit's "Adaptive RAG: Live" entry is not just present but **load-bearing
  and measurably correct**, which nothing had previously verified.
* `scripts/eval-run-retrieval.py` gained `EVAL_NO_MIX=1`. **Evaluate the default
  path as the primary configuration**; pin weights only to isolate one arm, and
  label those runs as static-path measurements.

## Graph arm: reconciler built, extraction extended

`scripts/graph-reconcile.sh` re-drives extraction for org-visible documents with
no `graph_text_units` rows, one document at a time, waiting for each to land so
every envelope is fresh when its turn comes. This is the standing answer to the
envelope-expiry problem for this subject (announce in bulk and roughly the first
four extract while the rest expire).

All 39 judged documents in `org-corpus-baseline` were flipped to
`visibility='org'` and reconciled. One document reported no rows within the
window — extraction legitimately yields nothing for some content, and the
reconciler retries it on the next run rather than hiding it.

## Deep graph tier needs a `graph:read` scope

`/v1/graph/traverse` requires `graph:read`; the eval principal does not have it,
so the deep multi-hop tier returns 403 and falls back to the in-process 1-hop
arm (which works — the 1-hop tier is what served every successful graph probe).
A circuit breaker then holds the remote tier off after repeated failures, which
is correct but means one scope gap disables the tier for a while. Worth knowing:
**any caller lacking `graph:read` silently gets 1-hop only.** Exercising the deep
tier needs the same kind of registry scope grant as the wiki arm.

## Default path vs static path on natural-language questions

Completing the picture — the same 87 questions, default path (no `mode_mix`,
smart hybrid decides) against the pinned static mix:

| org | path | recall@10 | nDCG@10 | MRR | n |
|---|---|---|---|---|---|
| contextual | static (pinned) | 0.8736 | 0.7732 | 0.7394 | 87 |
| contextual | **DEFAULT (smart hybrid)** | 0.8736 | 0.7713 | 0.7370 | 87 |
| | delta | +0.0000 | −0.0018 | −0.0024 | |

Deep inside the noise floor. (The baseline org's default-path run lost one
25-query chunk to token expiry, n=62, so its delta is not like-for-like and is
not reported.)

So the adaptive layer is **neutral on natural-language questions and decisive on
identifier queries** — precisely what an adaptive router should be: it costs
nothing where it does not apply. Combined with the lexical sweep above, the
weight question is settled: keep `W_BM25 = 0.1` as the static fallback and leave
`SMART_HYBRID_ENABLED=true` to handle the query classes that need more.

## Harness robustness

The data-plane internal token lives ~5 minutes; a full 87-query run with the
cross-encoder on exceeds it, and a background refresher racing the run proved
unreliable (43, then 4, tail failures). `eval-run-retrieval.py` now takes
`EVAL_OFFSET`/`EVAL_LIMIT`, and `scripts/eval-default-path.sh` runs the set in
25-query chunks with one freshly minted token each — deterministic instead of
racing.
