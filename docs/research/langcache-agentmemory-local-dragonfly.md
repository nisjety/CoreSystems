# Can LangCache + Agent Memory run local on Dragonfly instead of Redis?

> Feasibility finding (2026-06-13). Question: now that the self-hosted cache
> fleet is **Dragonfly**, can the two Redis-Cloud-flavoured AI services —
> **Redis LangCache** (semantic cache) and **Redis Agent Memory** — also be
> moved off Redis Cloud and onto local Dragonfly?
>
> Short answer: **Not onto Dragonfly directly.** Both services' value is in a
> *vector* layer, and Dragonfly does not provide the RediSearch/RedisVL surface
> they need. They *can* go fully local — but the vector arm must come from
> **Qdrant** (which Data Plane v2 already runs) or from a dedicated local
> **Redis Stack / Redis 8** sidecar. Dragonfly stays the KV/cache/session
> substrate; vectors are Qdrant's job. This matches the existing
> "redis for redis, qdrant for vectors" split and the retrieval-router design.

---

## 0. Why Dragonfly can't be the backend for either

Dragonfly is wire-compatible with the **Redis 5 core** command set (~185 Redis
commands per the upstream README) plus all Memcached commands. That covers
everything our self-hosted usage needs — strings, hashes, sets, sorted sets,
streams, pub/sub, TTLs, `requirepass` auth, `redis://` URLs, `redis-cli`. It
does **not** ship the **RediSearch / RedisVL** module surface (`FT.CREATE`,
`FT.SEARCH`, vector KNN indexes) that Redis Stack / Redis 8 provide. Both
LangCache and Agent Memory's long-term store are built on exactly that vector
surface. (We verified there is **no** `FT.*` / RediSearch / RedisVL usage in
the first-party codebase today — our own Redis usage is plain KV/cache/queue/
session, which is why the fleet migration to Dragonfly was clean.)

So the constraint is specific: **KV → Dragonfly is fine; vector search → not
Dragonfly.**

---

## 1. Redis LangCache (semantic cache)

**What it is in our stack.** A *managed* Redis Cloud semantic-cache service,
reached over REST. The model-gateway calls it from `langcache.rs`
(`crate::langcache::global()`) on the `invoke` / `invoke_stream` paths, behind a
`SemanticCache` seam. Config is REST-only:
`LANGCACHE_URL`, `LANGCACHE_CACHE_ID`, `LANGCACHE_API_KEY`, `LANGCACHE_THRESHOLD`.

**Can it be self-hosted on Dragonfly?** **No.** LangCache is a proprietary Redis
Cloud offering — there is no open-source LangCache server image to point at a
local Redis or Dragonfly. Internally it embeds the prompt, does a vector
similarity lookup against a RediSearch index, and returns the cached LLM
response above a similarity threshold. Dragonfly can't host that index.

**Local replacement (recommended).** We already own the seam. Implement the
`SemanticCache` trait against our own stack instead of the Redis Cloud REST API:

| Concern              | Local component (already deployed)                       |
|----------------------|----------------------------------------------------------|
| Embed the prompt     | Data/Model-plane embedding service (Azure/Cohere)        |
| Vector similarity    | **Qdrant** (`dpv2-qdrant`) — a small `langcache` collection |
| Cached payload + TTL | **Dragonfly** (KV: `langcache:{hash}` → response, TTL)   |

That keeps the exact same gateway behaviour (and ZDR locality) with no Redis
Cloud dependency. Dragonfly alone is insufficient (no vector arm); Qdrant
supplies it. This is a build behind the existing seam, **not** a new system.

---

## 2. Redis Agent Memory (agent-memory-server)

**What it is in our stack.** An **open-source** server by Redis
(`redislabs/agent-memory-server` Docker image; REST + MCP). It is the long-term
memory backend that `letta-bridge` can use via `AGENT_MEMORY_URL` /
`AGENT_MEMORY_TOKEN`; when unset, letta-bridge falls back to an in-memory store.
It is **two-tier**:

- **Working memory** — session-scoped, structured/KV. Plain RESP operations.
- **Long-term memory** — semantic (vector similarity) + full-text + hybrid
  search, with topic/entity extraction and recency boost.

**Can it point at Dragonfly?** It connects via a generic `REDIS_URL=redis://…:6379`,
so the *working-memory* tier would likely run against Dragonfly. But the
*long-term* tier does vector similarity + hybrid search, which needs the
RediSearch/RedisVL surface (Redis 8 / Redis Stack). That is **not** a supported
Dragonfly target, so the headline feature (semantic long-term recall) would not
work reliably. **Treat Agent-Memory-on-Dragonfly as unsupported.**

**Local options (all keep data local — no Redis Cloud):**

1. **Dedicated `redis/redis-stack` (or Redis 8) sidecar** *only* for
   agent-memory-server's index. Lowest friction if we want upstream's exact
   semantics. Trade-off: one Redis-Stack instance lives alongside the Dragonfly
   fleet — i.e., "everything is Dragonfly" is no longer literally true.
2. **Qdrant-backed memory (recommended).** Build long-term agent memory on the
   components we already run — **Qdrant** (vectors) + **Dragonfly** (working-set
   KV) + our embedding service — instead of agent-memory-server. Reuses the
   stack, avoids a second datastore, and follows the "harmonize, don't
   duplicate; best tool for the job" directive. Pairs with the retrieval router.
3. **Dev**: keep letta-bridge's in-memory fallback; wire option 2 for prod.

---

## 3. Bottom line

| Service        | On Dragonfly directly? | Local without Redis Cloud?                            |
|----------------|------------------------|-------------------------------------------------------|
| **LangCache**  | ❌ (no vector surface; managed-only) | ✅ via own `SemanticCache` seam → **Qdrant** + Dragonfly |
| **Agent Memory** | ❌ for the long-term/vector tier | ✅ via **Qdrant**+Dragonfly memory, *or* a local Redis-Stack sidecar |

**Decision rule:** Dragonfly owns KV / cache / session / queue. Vector search
(semantic cache, semantic memory) belongs to **Qdrant**, which Data Plane v2
already runs. Neither AI service needs Redis Cloud to stay local — but neither
runs purely on Dragonfly either. Keep the substrate split explicit.

## 4. Status — implemented 2026-06-13

Both the local exact-match tier **and** the Data-Plane-owned semantic tier are
now wired:

- **Gateway `SemanticCache` seam**
  (`apps/Model Plane/rust/services/model-gateway/src/langcache.rs`). Hosted
  LangCache wins outright when configured; otherwise the gateway composes the two
  local tiers. With both `SEMANTIC_CACHE_URL` (Dragonfly exact-match, ON by
  default) and `SEMANTIC_CACHE_DATAPLANE_ENABLED=true` set, it **layers** them: a
  fast Dragonfly exact `(org_id, model, prompt)` KV hit first, then a Data Plane
  v2 semantic fallback over HTTP (`DATAPLANE_RETRIEVAL_HTTP_URL` +
  `DATAPLANE_INTERNAL_KEY`); a semantic hit is **written through** to the exact
  tier so a repeat of that exact prompt stays local, and writes populate both
  tiers. Either tier alone is used solo. The gateway still hosts no vector store.
- **Semantic tier owned by Data Plane v2.**
  `retrieval-engine-rs/src/cache/semantic.rs` adds `search`/`store`: embed the
  prompt via the engine's `EmbeddingClient`, ANN over a dedicated
  `semantic_response_cache` Qdrant collection filtered by org + model +
  embedding-namespace, gated by `SEMANTIC_CACHE_MIN_SCORE` (cosine, default 0.95)
  with an in-process TTL (`SEMANTIC_CACHE_TTL_SECS`, default 1 day); the response
  rides in the point payload, idempotent upsert via a deterministic point id.
  Exposed as authed `POST /v1/cache/semantic/{search,store}`; opt-in via
  `SEMANTIC_CACHE_ENABLED=true`. Both crates `cargo check` + `--lib` tests green.
  - **Pruning:** Qdrant has no native point TTL, so authed
    `POST /v1/cache/semantic/prune` deletes points older than `older_than_secs`
    (default `SEMANTIC_CACHE_TTL_SECS`) via a `created_at` range filter — point a
    cron at it to keep the collection bounded.
- **Agent Memory → local agent-memory-server + Redis Stack.** The Model Plane
  deploy compose now runs `agent-memory-server` on a dedicated local
  `redis-stack` (RediSearch vector index); `letta-bridge` points
  `AGENT_MEMORY_URL` at it — the existing Go client already speaks the protocol,
  so **no Go change**. Fully local (no Redis Cloud); embeddings/generation via
  LiteLLM (OpenAI by default; Azure et al. supported).

**Decision rule (unchanged):** Dragonfly owns KV / cache / session / queue;
vector search belongs to **Qdrant** (LangCache semantic tier) or the
agent-memory-server's Redis-Stack index. Keep the substrate split explicit.
