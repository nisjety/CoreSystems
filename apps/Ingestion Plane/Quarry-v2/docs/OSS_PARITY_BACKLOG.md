# Quarry-v2 — OSS Parity & Ideas Backlog

> Method (opensrc-style): read the *real* current sources of 5 reference projects, map each
> capability against what Quarry-v2 already ships (DoD 10/10), and emit a prioritized backlog.
> Sources read 2026-05-30: `vercel-labs/opensrc`, `browser-use`, `firecrawl`, `tavily-python`,
> `apify/crawlee`, `elastic/elasticsearch`.
> Lenses applied: search-first · api-design · backend/multi-backend · benchmark · browser-qa ·
> rust-patterns · golang-pro · python-expert · context7-mcp.

Legend: ✅ parity · 🟡 partial · ❌ gap · ⛔ out-of-scope (evidence engine ≠ reasoning/cloud).

`opensrc` itself = an npm-source-fetcher skill for grounding agents in real package code. Not a
runtime dep for us (Rust/Go), but its *practice* — read source, not marketing — is what this doc applies.

---

## 1. Per-project capability map

### Firecrawl (closest competitor: scrape/crawl/map/search/extract/agent/batch)
| Capability | Quarry-v2 today | Verdict | Action |
|---|---|---|---|
| scrape → markdown/json/screenshot/links/html/summary | full (quarry-transform + AiFormatRunner) | ✅ | — |
| actions (click/scroll/write/wait/press) before extract | ActionRuntime, 13 variants | ✅ | — |
| batch scrape | `/v1/batch` | ✅ | — |
| change tracking | change_history + baseline store + `/v1/change/*` | ✅ | — |
| **`/map`** — fast whole-site URL discovery, relevance-ranked | sitemap.rs + frontier exist, **no endpoint** | ❌ | **P0 `/v1/map`** (reuse sitemap+robots+links+LexicalRanker) |
| **`/extract`** — prompt+schema across `domain/*` wildcards | per-page StructuredExtractClient only | 🟡 | **P1** multi-URL/wildcard extract fan-out |
| `agent` (FIRE-1) NL extraction | AgentLoop + Planner exist, not exposed as one call | 🟡 | fold into P1 `/v1/extract {prompt,schema}` |
| media parsing (PDF/DOCX) | pdf.rs (text) | 🟡 | P2 add DOCX |
| MCP server (agents connect in 1 cmd) | none | ⛔ | deferred — **Verevon** is Quarry-v2's client, not external agents |
| reliability claims (96% web, P95 3.4s) | unproven | 🟡 | benchmark lens → P2 live runs |

### Tavily (LLM-optimized search/extract/crawl/map/research)
| Capability | Quarry-v2 today | Verdict | Action |
|---|---|---|---|
| `/search` + `/answer` (citations) | `/v1/search` + `/v1/answer` (AnswerPipeline) | ✅ | — |
| `search_depth` basic/advanced | single depth | 🟡 | P0 param |
| **`topic`** general/news/finance routing | none | ❌ | **P0** vertical routing (already have intent_classifier to build on) |
| **`time_range` / `days`** recency filter | none | ❌ | **P0** recency filter |
| **`chunks_per_source`** (return best chunks, not full page) | returns full page | ❌ | **P0** chunk-ranked results (chunks.rs exists) |
| `exact_match` quoted-phrase | none | ❌ | P0 (Tantivy phrase query) |
| `include_answer` flag fused into search | separate endpoints | 🟡 | P0 flag on `/v1/search` |
| **RAG context mode** (token-bounded context string) | answer only | ❌ | **P0 `format=context`** |
| research reports | ResearchExecutor + MP `/v1/research` | ✅ | — |
| structured rate-limit error (code/window/retry_after/next_actions) | generic 429 | 🟡 | P2 api-design polish |

### browser-use (agentic browser, Python)
| Capability | Quarry-v2 today | Verdict | Action |
|---|---|---|---|
| Agent + Controller + LLM-agnostic loop | AgentLoop + Planner (Mock/ModelPlane) | ✅ | — |
| DOM-for-LLM serialization (indexed clickables, a11y tree) | build_dom_summary | 🟡 | P2 refine (viewport-aware, indexed click-map) |
| structured output (schema) | StructuredExtractClient | ✅ | — |
| agent memory / persistent scratch FS | none | ❌ | P2 agent scratchpad/memory |
| stealth / proxy rotation / captcha | TLS impersonation + Browserbase/Kernel | 🟡 | captcha = paid-provider; otherwise ✅ |
| sensitive_data masking, file outputs | none | 🟡 | P2 sensitive_data redaction in agent |
| cloud, 1000+ integrations (Gmail/Slack) | — | ⛔ | not an evidence-engine concern |

### Apify / Crawlee (crawl infra)
| Capability | Quarry-v2 today | Verdict | Action |
|---|---|---|---|
| durable request queue + dedupe | PostgresRequestQueue + frontier + checkpoints | ✅ | — |
| **AutoscaledPool** (global concurrency by CPU/mem) | per-host AIMD only | 🟡 | **P1** global resource-aware autoscaler |
| SessionPool + auto fingerprint rotation on block | profiles + TLS fp, manual | 🟡 | P1 auto-rotate on block detection |
| **enqueueLinks glob/regex + strategy** (same-domain/subdomain) | domain scope only | ❌ | **P1** crawl include/exclude path patterns |
| Dataset / KeyValueStore / RequestQueue storages | artifact store + control-plane stores | ✅ | — |
| statistics / failedRequestHandler | retry + StepReceipts | ✅ | — |

### Elasticsearch (retrieval engine) — the search-first goldmine
| Capability | Quarry-v2 today | Verdict | Action |
|---|---|---|---|
| BM25 lexical | TantivyLocalIndex | ✅ | — |
| **vector / kNN (HNSW) semantic** | none | ❌ | **P0** embeddings (Model Plane) + vector index |
| **hybrid BM25+vector + RRF fusion** | lexical-only router | ❌ | **P0** RRF in SmartRouter |
| highlighting / snippets | none | ❌ | P1 matched-term snippets |
| aggregations / facets (host/date/type) | org_id facet only | 🟡 | P1 facets |
| analyzers: stemming/synonyms/per-lang | lang.rs detect only | 🟡 | P2 analyzers + synonyms |
| index lifecycle: snapshot/reindex/aliases | none | ❌ | P2 corpus index lifecycle |

---

## 2. Prioritized backlog

### P0 — highest leverage (search-first + agent adoption)
1. **Hybrid search (BM25 + vector + RRF).** Embed scraped corpus via Model Plane; store vectors
   (pgvector on existing Postgres, or qdrant as `SearchProvider`); fuse with Tantivy via Reciprocal
   Rank Fusion in `smart_router`. *Crates:* quarry-runtime (`local_index`, `smart_router`, new `vector_index`). *Lens:* search-first, multi-backend.
2. **`/v1/map`.** Whole-site URL discovery (sitemap + robots + link graph), optional `search=` relevance
   rank via LexicalRanker. *Crate:* quarry-edge + reuse transform. Low effort. *Lens:* api-design.
3. **Tavily search-param parity:** `topic` (general/news/finance), `time_range`/`days`, `exact_match`,
   `chunks_per_source`, `include_answer` flag, `format=context` (token-bounded RAG context).
   *Crate:* quarry-edge `search_routes` + chunks.rs. Mostly param plumbing. *Lens:* search-first, api-design.
   *Consumer:* shape these for **Verevon**'s search/answer UX (Verevon drives Quarry-v2).

### P1
5. **`/v1/extract`** — prompt+schema over `domain/*` wildcards (crawl→extract fan-out, FIRE-1-style).
6. **Crawl path include/exclude glob/regex** + enqueue strategy (same-domain/subdomain/all).
7. **Search highlighting/snippets + facets** (host/date/content-type aggregations).
8. **AutoscaledPool** — global concurrency governed by CPU/mem, layered over per-host AIMD.
9. **Auto session/fingerprint rotation on block detection** (429/403 → rotate profile+TLS).

### P2
10. Tantivy index lifecycle (snapshot/reindex/aliases) + per-language analyzers + synonyms.
11. DOM-for-LLM serialization upgrade + agent memory/scratchpad + sensitive_data redaction.
12. DOCX/extended media parsing.
13. Structured rate-limit error envelope (code/window/retry_after/next_actions).
14. **Live benchmark corpus** vs Firecrawl/Tavily/Trafilatura/Readability → publish SCOREBOARD.md
    (reference projects publish benchmarks; we must too to claim parity). *Lens:* benchmark.

### ⛔ Skip (out of scope for an evidence engine)
Cloud hosting, captcha solving, 1000+ SaaS integrations, keyless public tier, reasoning/knowledge
ownership (stays in Model/Data planes).
**MCP server (deferred):** Verevon is Quarry-v2's driver/client — Quarry serves Verevon over its
REST/GraphQL surface, not external AI agents over MCP. Revisit only if third-party agent access
becomes a goal.

---

## 3. Quick wins (≤1 cycle each, do first)
- `/v1/map` (#2) — pure recombination of existing sitemap/robots/links/ranker.
- Tavily params `exact_match` + `time_range` + `include_answer` flag (#4) — param plumbing.
- `format=context` RAG mode (#4) — wrap existing chunk ranker, token-budget the output.
- Crawl glob include/exclude (#6) — add patterns to frontier scope check.

## 4. Strategic bet
**Hybrid retrieval (#1)** + Verevon-facing API parity (`/v1/map`, Tavily params, `format=context`)
turn Quarry from "scraper with a lexical index" into the self-hosted web-context engine **Verevon
drives** — the Firecrawl/Tavily positioning, but org-isolated, ZDR-aware, on your own infra.
(MCP server deferred: Verevon is the client, not external agents.)
