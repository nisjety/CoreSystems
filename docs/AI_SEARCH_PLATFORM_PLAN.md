# AI Search & Answer Platform — Cross-Plane Implementation Plan

Status: **DRAFT — awaiting confirmation**
Owner: Verevon / Quarry-v2
Scope: Turn the search box into a whole-system AI answer engine: web + own-corpus
retrieval → grounded LLM answer → follow-up chat → metered, org-scoped, persisted,
and auditable across all planes.

---

## 1. Requirements (restated)

1. **Optimally implement** the features discussed: enable AI answers, hybrid search
   (lexical ⊕ Data-Plane vectors via RRF), Data-Plane-first RAG, answer streaming,
   and a tiered cache (browser → Redis → Data-Plane-as-durable-corpus).
2. **Enhance further**: deep research, saved searches/alerts, source promotion into
   the durable corpus, intent routing for the Info/Bilder/Videos/Kart/Shopping tabs.
3. **Make the system whole**: wire the other planes so search/answer is a first-class
   product surface — Convex (persistence + realtime), Integrations (answer over
   connected apps), User/Org/Session (tenancy), Billing/Lago (metering + quotas),
   Notification, Audit.

## 2. Current state (verified this session)

- **Quarry-v2 edge** has the full engine already coded:
  - Provider chain: Tantivy own-corpus → Stract → SearXNG → Brave/Serper (paid backup).
  - `HybridSearchProvider` (lexical ⊕ `DataPlaneVectorIndex` `/v1/retrieve`, RRF-fused) —
    **gated on a Data-Plane retrieval URL config field**.
  - `AnswerPipeline` (search + Model Plane synthesis → `{answer, citations}`) —
    **gated on `model_plane_url`**.
  - `PageCache` (Redis, `quarry:page:{blake3}`, TTL 3600s) — **scrape cache only**;
    no SERP or answer cache yet.
  - Intent classifier stack (`CachedClassifier(HybridClassifier(MpIntentClassifier))`),
    per-provider circuit breakers, billing meter hooks (`quarry.search.query`,
    `answer-{}`), ZDR awareness.
- **BOTH answers and hybrid are OFF today** due to env-var/config-field mismatches:
  container sets `QUARRY_EDGE__MODEL_PLANE_BASE_URL` / `_BEARER_TOKEN` /
  `_DATA_PLANE_RETRIEVAL_BASE_URL`, but `EdgeConfig` reads `model_plane_url` /
  `model_plane_token` (+ the retrieval field). Result: web-only, blank answer card.
- **verevonv2 BFF** already routes to every plane (envs present): `QUARRY_EDGE_URL`,
  `MODEL_PLANE_URL`, `AUTH_CORE_URL`, `USER_CORE_URL`, `ORG_CORE_URL`,
  `BILLING_CORE_URL`, `SESSION_CORE_URL`, `INTEGRATION_CORE_URL`, `GRAPH_INDEX_URL`,
  `DATA_PLANE_DOCUMENTS_URL`, `NOTIFICATION_CORE_URL`. `SearchAnswerView` (1126 LoC)
  already does turn-0 answer card + grounded, streamed follow-up thread
  (`streamChat` → `/api/chat/stream` → Model Plane `/v1/invoke/stream`).

**Implication:** most of the engine exists. The work is *activation*, *orchestration*,
*caching*, and *cross-plane wiring* — not greenfield.

## 3. Target architecture (whole-system flow)

```
                         ┌────────────── verevonv2 (Frontend Plane) ──────────────┐
  user query ──▶ SearchAnswerView ──▶ BFF /api/v1/search/web (SSE)               │
                         │  ▲  follow-up thread ──▶ /api/chat/stream (SSE)        │
                         │  │  browser query cache (SWR, 30–60s)                  │
                         └──┼──────────────────────────────────────────────────-┘
                            │ org/user JWT (audience tokens)
        ┌───────────────────┼───────────────────────────────────────────────┐
        ▼                    ▼                                                ▼
  Control Plane         Quarry-v2 edge  /v1/search?include_answer        Model Plane
  (auth/user/org/    ┌─ Redis cache (SERP + answer, org-scoped, TTL) ─┐  (gateway:8080)
   session)          │  HybridSearchProvider:                         │   - LLM synth
   - tenancy         │    lexical/SERP  ⊕  Data-Plane /v1/retrieve     │   - follow-up
   - JWT audience    │    (RRF; DP-first short-circuit)                │   - intent class
        │            │  AnswerPipeline → context → Model Plane synth   │
        ▼            └────────────────────┬───────────────────────────┘
   Billing/Lago  ◀── meter (query, synth, scrape, tokens) │ promote high-value pages
   - quotas/plans                                          ▼
   Audit  ◀── log(who/what/sources/answer)        Data Plane (dpv2-*)
   Notification ◀── deep-research done / alerts    - Qdrant vectors, retrieval_v2
   Convex ◀── persist threads + realtime           - documents, embeddings, graph
        ▲                                           - ingest target (web + integrations)
        └── Integrations (integration-api) ── sync connected apps ──▶ Data Plane corpus
```

## 4. Phased plan

### Phase 0 — Activate what's built (config + verify) — **LOW risk, highest value**
Goal: AI answer + hybrid live, end to end.
- Read `config.rs` to confirm exact field names for model-plane + DP-retrieval; reconcile
  the quarry-edge env (`QUARRY_EDGE__MODEL_PLANE_URL`, `_TOKEN`, `_MODEL`, and the
  retrieval URL field) → point at `model-gateway:8080` + `dpv2-retrieval-engine:8004`.
- Confirm the Model Plane synthesis API shape + model name the `AnswerPipeline` expects;
  confirm the Data-Plane `/v1/retrieve` request/response contract.
- Restart edge; verify logs (`answer pipeline: wired`, `hybrid search: … wired`) and a
  real `/v1/search?include_answer=true` returns `answer` + `citations` with DP hits fused.
- Files: `quarry-edge/docker-compose*`/env, `config.rs` (only if field rename needed).

### Phase 1 — Caching layers (faster queries)
- Extend Redis cache: **org-scoped SERP cache** + **answer cache**; intent-driven TTL
  (news 2–5 min, default 15 min, evergreen 6–24 h, answer 1–6 h); stale-while-revalidate;
  negative caching (~30 s); **ZDR gating** (skip durable cache when ZDR on).
- Browser query cache (SWR/React Query, staleTime 30–60 s) in verevonv2 search hooks.
- Files: `cache.rs`, `search_routes.rs`, `answer_routes.rs`, `config.rs`, verevonv2
  `search-v2` hooks.

### Phase 2 — Data-Plane-first RAG + richer grounding
- Add **DP-first short-circuit**: query Data Plane first; if internal hits clear a
  confidence/count threshold → synthesize from org knowledge, skip web; else fuse/fallback.
- Route via the existing intent classifier (internal vs web-looking queries).
- Feed **full DP chunk content** (not just snippets) into the answer context; citations
  tag internal-doc vs web source.
- Files: `answer.rs`, `hybrid.rs`, `search_routes.rs`.

### Phase 3 — Streaming answers
- Stream the **initial** answer token-by-token (parity with follow-ups). Option A: edge
  `/v1/answer/stream` (SSE) → BFF re-stream. Option B: BFF builds context via edge then
  streams Model Plane directly. (Lean A — keeps synthesis server-side + metered.)
- Files: `answer_routes.rs` (stream variant), BFF `/api/v1/search/web` → SSE, `SearchAnswerView` (turn-0 consumes stream).

### Phase 4 — Cross-plane "whole system" wiring
- **User/Org/Session (Control Plane):** enforce org scope end-to-end — cache keys,
  DP retrieval org filter, citations, audit. (Audience-token minting already present.)
- **Billing/Lago:** wire the existing meter hooks to billing-core/Lago metrics
  (`quarry.search.query`, answer synth, scrape, tokens); enforce plan quotas + rate caps;
  surface usage in UI.
- **Convex:** persist search/chat threads + saved searches; realtime subscriptions for
  multi-device / collaborative answer threads; store streaming turn state.
- **Integrations (integration-api):** connected-source sync → ingest into Data Plane →
  "answer over your connected apps + the web" (one unified retrieval).
- **Notification:** deep-research completion, saved-search alerts, quota warnings.
- **Audit:** record query/answer/sources per user+org for compliance.

### Phase 5 — Enhancements ("enhance further")
- **Deep research mode** (use merged `deep_research.rs`): multi-step, async, notify on
  done, persist to Convex.
- **Promote high-value finds** into the durable Data-Plane corpus (scrape → ingest →
  future semantic recall) — the DP layer earns its keep over time.
- **Tab intent routing**: Info→web/answer, Bilder→images (live), Videos/Kart/Shopping→
  specialized providers.
- **Quality**: reranking, dedup, freshness boost, source-trust scoring; Norwegian-first
  UX; an **answer-quality eval harness** (groundedness + citation accuracy).

### Phase 6 — Observability, scale, prod
- Metrics/traces via the model-plane otel-collector; latency budgets; reuse circuit
  breakers + `AutoscaledPool`. Optional verevonv2 prod mode for zero compile latency.

## 5. Cross-plane integration matrix

| Plane | Service(s) | Role in the answer engine |
|---|---|---|
| Frontend | verevonv2 | UI, BFF, browser cache, streaming |
| Ingestion | quarry-edge/runtime | search, scrape, hybrid, answer pipeline, Redis cache |
| Model | model-gateway | LLM synthesis, follow-up chat, intent classify |
| Data | dpv2-retrieval/qdrant/documents/embeddings | own-corpus retrieval + durable ingest |
| Control | auth/user/org/session | tenancy, JWT audience, org scoping |
| Billing | billing-core + Lago | meter queries/synth/tokens, quotas |
| Convex | convex-backend/gateway/subscriber | thread persistence + realtime |
| Integration | integration-api | connected-source ingest → searchable |
| Notification | notification-core | alerts / deep-research done |
| Audit | audit-core | compliance log of query/answer/sources |

## 6. Dependencies & sequencing
- Phase 0 unblocks everything (answers + hybrid). Phases 1–3 are parallel-able after 0.
- Phase 4 sub-streams are mostly independent (org-scope first — it's a correctness gate).
- Phase 5/6 follow.

## 7. Risks
- **HIGH** — Model Plane synthesis API shape + model name mismatch (Phase 0 spike).
- **HIGH** — Tenant isolation: org-scoped cache/retrieval/citations must be airtight
  before any shared cache ships (privacy).
- **MED** — Data-Plane `/v1/retrieve` contract drift vs `DataPlaneVectorIndex`.
- **MED** — Streaming through two BFF hops (SSE) + abort/cancel correctness.
- **MED** — ZDR: durable cache + DP promotion must honor zero-data-retention.
- **LOW** — TTL tuning; dev-mode compile latency (already mitigated).

## 8. Open questions (resolve in Phase 0)
1. Exact `EdgeConfig` field names for model-plane + DP retrieval (env reconciliation).
2. Correct Model Plane endpoint/path + model name for synthesis.
3. Does verevonv2 already use Convex for chat-v2 persistence, or is it net-new here?
4. What does integration-api currently ingest, and into which Data-Plane API?

## 9. Complexity (rough)
- Phase 0: S (config + verify). Phase 1: M. Phase 2: M–L. Phase 3: M.
- Phase 4: L (spans planes). Phase 5: L. Phase 6: M.

## 10. Success metrics
- AI answer renders with citations on first search; warm repeat < 300 ms.
- Hybrid: DP hits appear in citations for org-knowledge queries.
- Every query metered + org-scoped + audited; ZDR respected.
- Follow-ups grounded; deep research persists + notifies.
