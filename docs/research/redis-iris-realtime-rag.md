# Real-time RAG on Redis — Iris Context Engine Reference

Source labels (indexed in context-mode KB, searchable):
- `Redis Context Engine docs` — https://redis.io/docs/latest/develop/ai/context-engine/
- `Redis Iris landing` — https://redis.io/iris/
- `Redis RAG get-started` — https://redis.io/docs/latest/develop/get-started/rag/
- `Redis context-is-all-you-need blog` — https://redis.io/blog/context-is-all-you-need/
- `redis-iris-demos repo` — https://github.com/redis/redis-iris-demos

## TL;DR

Classic RAG = vector DB + LLM. **Real-time RAG (Redis Iris)** = a *context engine* with **four** composable services on Redis Cloud — LangCache, Agent Memory, Context Retriever, Data integration (RDI) — all accessible via REST; LangCache and Agent Memory also ship Python/Node SDKs. **Redis Search is the vector/structured/full-text substrate underneath, not a fifth service.** The shift: agents don't fail from lack of model power — they fail from stale/fragmented context. Iris solves it by treating context as a runtime service tier.

> Verified against [Redis Context Engine docs] 2026-05-28: the docs say "Context engine includes four services." Earlier framing here counted Redis Search as a fifth — corrected.

## Iris — the 4 services (+ Redis Search substrate)

| Service | Role | When to use |
|---|---|---|
| **LangCache** | Semantic LLM-response cache (managed embeddings, similarity threshold, TTL, eviction) | Cut LLM cost/latency on repeated/paraphrased queries |
| **Agent Memory** | Two-tier memory: session (TTL, working) + long-term (vector-embedded, auto-promoted from sessions asynchronously) | Multi-turn chat, user preferences, learned patterns |
| **Context Retriever** | Define entities once (customers, orders…); auto-generates safe tools agents call instead of hitting DB directly. Per-agent keys + access tags | Tool-using agents that need governed, navigable access to operational data |
| **RDI (Redis Data Integration)** | Keeps Redis in sync with source systems (Oracle/MySQL/Postgres/SQL Server) via initial sync + CDC; updates land in Redis within seconds | Fresh operational state, not yesterday's dump |
| _Redis Search / Vector_ (substrate, **not** one of the four) | Underlying vector + structured + full-text index the four services run on | The retrieval substrate — in CoreSystem this role is already filled by Qdrant + Quickwit |

Optional 6th in demos: **Semantic Routing** — blocks off-topic queries before they hit the LLM.

## Classic RAG vs Iris context engine

Classic Redis RAG (the `/get-started/rag/` doc):
1. **Retrieve** — vector search + filters in Redis
2. **Augment** — assemble prompt with user query + retrieved chunks
3. **Generate** — LLM completes; optionally cache via semantic cache

Iris extends this with:
- **Navigable data** — agents traverse entity relationships, not flat chunks
- **Compound memory** — sessions promote to long-term knowledge automatically
- **Live freshness** — RDI feeds operational data continuously
- **Governance** — tools instead of raw DB access; access tags scope what each agent sees

Blog framing — context engines must deliver: navigable data, instant retrieval, real-time freshness, and compounding memory.

## Reference architecture (from `redis-iris-demos`)

FastAPI backend, LangGraph ReAct agent, React+SSE frontend. 7-phase streaming pipeline.

```
backend/app/
  main.py                       # FastAPI, SSE streaming, 7-phase pipeline
  langgraph_agent.py            # LangGraph ReAct agent
  guardrail_service.py          # Semantic Routing (blocks off-topic)
  langcache_service.py          # Semantic cache lookup before LLM
  memory_service.py             # Agent Memory client (session + long-term)
  context_surface_service.py    # MCP-style tool wrapper over Context Retriever
  core/
    domain_contract.py          # DomainPack protocol
    domain_schema.py            # EntitySpec
domains/                        # reddash, electrohub, finance-researcher, healthcare, radish-bank
```

Pipeline order (inferred from naming):
1. Guardrail → 2. LangCache lookup → 3. Memory recall → 4. Context Retriever tool calls → 5. LLM (ReAct) → 6. Memory write-back → 7. LangCache write.

## Mapping to CoreSystem (Model Plane v1 + Data Plane v2)

**Scope correction (2026-05-28):** earlier this section proposed a greenfield `mp-context` crate. That's obsolete. Model Plane **v1** is the canonical target (Model Plane v2 is being deprecated once v1 reaches parity), and the seams Iris needs already exist in v1 — the work is integration, not a new crate. Do **not** build on v2 (agent-core/Letta). Best-tool principle: Redis for cache/memory, Qdrant + Quickwit stay the retrieval substrate, no duplicate systems.

Verified mapping, ranked by fit:

- **LangCache → model-gateway `Proxy.Invoke`/`InvokeStream`** (`services/model-gateway/internal/proxy`). _Implemented:_ a `SemanticCache` interface + `internal/langcache` REST client, config-gated by `LANGCACHE_URL` / `LANGCACHE_CACHE_ID` / `LANGCACHE_API_KEY`; nil/no-op when unset. Lookup runs after `prepare`, before `Inference.Infer`; a hit skips the token-billed LLM call and returns zero token counts. Attributes scope by `org_id` (tenant isolation) + `model`. No semantic cache existed before (0 hits repo-wide), so this is purely additive.
- **Agent Memory → `letta-bridge`** long-term tier (`services/letta-bridge`). Its `memstore` is an in-memory substring stub today; back it with Iris Agent Memory (vector long-term + session tier via session-core), promotion fired async over the NATS inter-plane bus (non-blocking). Extend the existing store interface — do not add a parallel memory system.
- **Context Retriever → `mcp-bridge` / `ProxyMcpToolSvc`** (ModelGateway). Define an EntitySpec once for Postgres entities → Iris generates MCP tools → serve through the existing proxy, governed by **`mp-authctx`** JWKS per-agent identity → access tags. Replaces hand-coded tool defs.
- **RDI → operational lane only.** CDC-sync Postgres (finspo-core, audit-core via pgxpool) → Redis for entity tools. Keep **Qdrant** (vector, 175 refs) + **Quickwit** (FTS) in Data Plane v2 for document RAG. Two retrieval lanes, parallel, each on its best engine.
- **Redis Search/Vector → skip.** Competes with Qdrant; adopt only if consolidating engines is an explicit goal.

## Skill references (local)

The `.agents/skills/redis-*` directories exist but `SKILL.md` was empty in this pull — only the heading rendered. Local skill bodies may not be checked in yet. The relevant ones to populate or consult elsewhere:

- `iris-development` — Iris orchestration patterns
- `redis-vector-search` — HNSW vs FLAT, FT.CREATE with VECTOR fields, hybrid filters
- `redis-semantic-cache` — LangCache SDK/REST, similarity threshold, per-task caches
- `redis-core` — key naming, data-structure choice (Hash vs JSON vs Stream)
- `redis-query-engine` — FT.SEARCH, aggregations, hybrid query
- `redis-connections` — pooling, pipelining, RESP3 client-side cache
- `redis-clustering` / `redis-observability` / `redis-security` — prod hardening

To populate them, look at how the Iris demo backend wires `langcache_service.py`, `memory_service.py`, `context_surface_service.py` — those are canonical reference impls.

## Key takeaways

1. **Don't build raw vector search and call it RAG** — wrap it as a context engine: cache + memory + retrieval-as-tools + live sync.
2. **Memory is two-tier and async** — session (TTL) auto-promotes to long-term vector store in the background. Do not block the request path on memory writes.
3. **Generate tools from a data model, not per-agent** — Context Retriever pattern: one EntitySpec → many tools, governed by access tags.
4. **Cache by *semantic similarity*, not exact match** — LangCache with tuned threshold + per-task cache separation (don't pool unrelated workloads in one cache).
5. **Freshness is a first-class concern** — RDI / change-data-capture > scheduled dumps. Without it, Iris degrades to a fast stale store.
