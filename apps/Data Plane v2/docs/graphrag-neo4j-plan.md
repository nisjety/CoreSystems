# GraphRAG with Neo4j + Qdrant — Data Plane v2 Plan

> Status: **PLAN** (Phase 1 of the program). Reference architecture:
> <https://qdrant.tech/documentation/examples/graphrag-qdrant-neo4j/>.
> This document is the contract for the incremental build that follows; each
> subsequent phase lands with its own build + tests + conventional commit.

## 0. TL;DR

Add a **Neo4j-backed native multi-hop knowledge-graph traversal** capability to the
Data Plane and fuse it into the existing 4-arm retrieval pipeline, **without**
standing up a parallel extraction pipeline or a second store of record.

- **Decision:** *extend* the existing `graph-index-rs` service rather than add a new
  service. It already extracts entities/relationships/claims via the Model Plane
  inference contract, consumes ingest events over JetStream, enforces org-visibility
  + ZDR before persistence, and persists the canonical graph to Postgres. Neo4j is
  added there as a **rebuildable, org-scoped graph read-model** — the same role
  Qdrant/Quickwit already play for vectors/sparse.
- **Roles (as specified in the task):**
  - **Neo4j = Knowledge Graph** — extracted entities/concepts as **nodes**, explicit
    relationships as **edges**; answers complex multi-hop queries via **Cypher**.
  - **Qdrant = Vector Engine** — dense embeddings of chunks; semantic-similarity
    retrieval (already live).
  - **GraphRAG retrieval fuses them:** Qdrant vector-retrieves chunks → resolve their
    entities (via `graph_text_units` / `source_refs`) → traverse Neo4j (multi-hop
    Cypher) for connected facts → map connected entities back to chunks → RRF-fuse a
    `w_graph`-weighted graph arm into the fused candidate list.
- **Why Neo4j when Postgres already has a graph:** `graph-index-rs`'s
  `store.rs::get_graph_expansion` already does multi-hop expansion — but as
  application-level BFS issuing N+1 `get_relationships`/`get_entity` queries **per
  hop**. That does not scale past shallow hops. Neo4j does the same traversal as a
  single index-backed Cypher round-trip (`MATCH (e)-[*1..N]-(n)`), which is the
  honest, measurable value-add. Postgres stays canonical; Neo4j accelerates
  traversal.

## 1. What already exists (do not duplicate)

| Concern | Where it lives today | Reuse plan |
|---|---|---|
| Entity/relationship/claim extraction (LLM) | `graph-index-rs/src/extractor.rs`, routed through **Model Plane inference-core gRPC** (`Backend::ModelPlane`, `config.rs`) | **Reuse as-is.** No new LLM calls. |
| Ingest → extraction trigger | `graph-index-rs/src/stream.rs` (NATS JetStream consumer off `index-engine-rs` progression + orphan cleanup) | **Reuse as-is.** Neo4j dual-write hooks the same code path. |
| Canonical graph persistence | `graph-index-rs/src/store.rs` (`graph_entities`, `graph_relationships`, `graph_claims`, `graph_communities`, `graph_text_units`) | **Stays canonical.** Neo4j mirrors it. |
| Org-visibility + ZDR gate on persist | `store.rs::persist_extraction` gates on `knowledge_unit_is_org_visible`; `stream.rs`/`extractor.rs` drop restrictive-ZDR events before any write | **Inherit.** Neo4j write runs *downstream* of a successful Postgres persist, so it inherits both gates automatically. |
| Auth (verified RS256/JWKS user or scoped service; tenant pinned; cross-tenant denied) | `graph-index-rs/src/auth.rs` + `inference_auth.rs` (2026-07-10 remediation) | **Mirror** for the new traverse endpoint. |
| 4-arm retrieval fusion (dense/sparse/wiki/visual) with concurrent `tokio::join!` + sequential RRF | `retrieval-engine-rs/src/pipeline/orchestrator.rs` (`fuse_arms`, `arm_*`), `search/fusion.rs` (`reciprocal_rank_fusion`) | **Extend** with a 5th arm; mirror the wiki/visual arm shape. |
| Postgres graph arm (1-hop expansion) | `retrieval-engine-rs/src/search/graph.rs` (`graph_expansion_search`) + separate `/v1/retrieve/graph` endpoint | **Keep**; the new fused arm reuses its entity-resolution SQL for seeds and its source-visibility joins for the candidate mapping. |
| `w_graph` weight | Captured in `ResolvedWeights` / `mode_mix` trace but **never consumed by `fuse_arms`** (gap 16.1.1 "mode_mix trace lies") | **Close it** — the graph arm folds `w_graph` into RRF so trace == scoring. |

This directly advances two already-documented roadmap items:

- gap-data §16.1.1 — *"`w_graph`/`w_wiki` recorded but scoring ignores them"* → graph
  signal now folded into the scalar score path.
- Sovereign plan Phase 4 — *"fold graph into the fused RRF path … needs a shape
  adapter from entities/claims → candidates"* → this arm is that adapter.
- gap-data §13.6 wave-3 candidate — *"4-way scalar blend that actually consumes
  graph+wiki signals at scoring time."*

## 2. Non-negotiable contracts (Data Plane authority rules)

Every design element below is checked against these:

1. **Identity + org-scoping at every boundary.** The traverse endpoint takes
   `Extension<AuthContext>` and pins `org_id` from the *verified* context (mirror
   `retrieval-engine-rs/src/authz/context.rs` `pin_org_from_ctx` and graph-index-rs
   `auth.rs`). gRPC path asserts the request org against the JWT claim and rejects
   body/bearer mismatch (mirror `grpc/interceptor.rs`). Every Cypher query is
   parameterised **and** filtered by `n.org_id = $org` — Neo4j never sees a query
   without an org scope. Cross-org traversal is impossible by construction (org is a
   node property AND a query predicate) and is defended a second time by the
   Postgres provenance re-join (§6).
2. **ZDR propagates, fail-closed.** A restrictive-ZDR (ephemeral/reject) ingest never
   persists to Postgres today; because Neo4j writes are strictly downstream of the
   Postgres persist, restrictive content **never** reaches Neo4j. A restrictive-ZDR
   *query* must not egress to a retaining provider and must not persist — the graph
   arm performs no embedding/LLM egress (it reuses the already-computed dense arm's
   results as seeds and traverses the local Neo4j read-model), and it is disabled on
   the same restrictive posture that already gates the embed/rerank egress in
   `orchestrator.rs`. Neo4j connection secrets fail startup closed when required.
3. **No cross-plane DB access.** Neo4j is a Data-Plane-internal store owned by
   `graph-index-rs`. Only `graph-index-rs` opens a Bolt connection to it.
   `retrieval-engine-rs` reaches graph traversal **through graph-index-rs's HTTP
   endpoint** (internal service auth), never a direct Bolt connection — keeping Neo4j
   access encapsulated exactly as the task's step 4 specifies.
4. **Embeddings/inference through Model Plane only.** Extraction keeps routing through
   inference-core. Neo4j adds **zero** inference. No independent embedding/rerank.
5. **Retrieval stays auditable.** The graph arm's contribution is recorded on the
   retrieval trace (`mode_mix` now honestly reflects that `w_graph` was applied;
   candidate counts include the graph arm).

## 3. Neo4j data model

Neo4j mirrors the canonical Postgres graph. IDs are the **same** Postgres primary
keys, so the two stores cross-reference exactly as the reference architecture
interlinks Qdrant payload `id` ↔ Neo4j node `id`.

### 3.1 Node labels & properties

```
(:Entity {
    entity_id:   String,   // = graph_entities.entity_id (Postgres PK, the join key)
    org_id:      String,   // tenant scope — property AND every-query predicate
    entity_type: String,   // Person, Organization, Product, Concept, ...
    entity_text: String,   // canonical mention text
    confidence:  Float,
    updated_at:  Integer   // epoch millis, for read-model freshness/rebuild
})
```

- No document text, no chunk bodies, no PII beyond the entity mention already present
  in the canonical Postgres row. Neo4j holds **structure**, not content — content
  stays in Postgres/Qdrant. This keeps the ZDR blast radius minimal.
- `claims` and `communities` are **not** mirrored to Neo4j in this phase; they are
  not traversal primitives and Postgres serves them well (`graph_claims`,
  `graph_communities`). Revisit only if multi-hop claim reasoning is needed.

### 3.2 Relationship type & properties

The reference architecture uses a single `[:RELATIONSHIP {type}]` edge. We keep the
`relation_type` as a **property** (not a dynamic Neo4j label) so the mirror is
idempotent and rebuildable without schema churn, and so parameterised Cypher stays
injection-safe (Neo4j cannot parameterise relationship *types*; using a property
avoids string-building Cypher from extracted text):

```
(:Entity)-[:REL {
    rel_id:        String,  // = graph_relationships.rel_id (Postgres PK)
    org_id:        String,  // tenant scope
    relation_type: String,  // "works_at", "located_in", ... (from extraction)
    confidence:    Float,
    updated_at:    Integer
}]->(:Entity)
```

Direction follows Postgres `entity_a_id → entity_b_id`. Traversal queries are
undirected (`-[:REL]-`) to mirror `get_graph_expansion`'s undirected neighbour walk.

### 3.3 Constraints & indexes (bootstrap on startup)

```cypher
CREATE CONSTRAINT entity_id_unique IF NOT EXISTS
  FOR (e:Entity) REQUIRE e.entity_id IS UNIQUE;
CREATE INDEX entity_org IF NOT EXISTS       FOR (e:Entity) ON (e.org_id);
CREATE INDEX entity_org_text IF NOT EXISTS  FOR (e:Entity) ON (e.org_id, e.entity_text);
CREATE INDEX rel_org IF NOT EXISTS          FOR ()-[r:REL]-() ON (r.org_id);
```

`entity_id` uniqueness makes `MERGE (e:Entity {entity_id})` idempotent — re-processing
the same chunk never creates duplicate nodes (the reference architecture's bug of
using `CREATE` with random UUIDs is avoided by reusing the Postgres PK).

### 3.4 Linkage to Qdrant / chunks

`Entity.entity_id` → Postgres `graph_text_units.entity_id` → `knowledge_id` →
`knowledge_units` (chunk) → Qdrant point. The graph arm resolves this join in
Postgres (which also enforces org-visibility), so Neo4j never needs to store the
chunk/knowledge linkage. This is the deliberate split: **Neo4j = topology, Postgres =
provenance + authorization + content pointers.**

## 4. Extraction (reuse; add dual-write)

No change to *how* entities are extracted. The only addition is a mirror write:

1. `stream.rs` consumer receives a signed, org-scoped, non-restrictive-ZDR ingest
   event and calls `extractor.rs` → `store.persist_extraction(org, knowledge_id, result)`.
2. `persist_extraction` returns the freshly-minted `(entity_ids, rel_ids, claim_ids)`
   **only when the knowledge unit is org-visible** (existing gate). On the empty
   return (restrictive/invisible), **nothing** is mirrored.
3. **New:** when `entity_ids` is non-empty and Neo4j is enabled, mirror the just-persisted
   entities + relationships to Neo4j via idempotent `MERGE` (see §3), scoped to `org`.
   The write reads back the canonical rows (or reuses the in-memory `ExtractionResult`
   + minted ids) so node/edge properties match Postgres exactly.
4. **Best-effort, non-fatal:** a Neo4j write failure logs a warning + increments a
   metric and **does not** fail ingest (Postgres remains the source of truth; the
   read-model is rebuildable). Mirrors how trace-persistence failure is non-fatal in
   the retrieval path.

ZDR: because step 3 only runs after step 2's gate, **restrictive-ZDR content is never
mirrored** — no extra ZDR code needed on the Neo4j path, and we add a test asserting a
restrictive event produces zero Neo4j nodes.

Idempotent MERGE (per relationship, batched with `UNWIND`):

```cypher
UNWIND $entities AS e
MERGE (n:Entity {entity_id: e.entity_id})
SET n.org_id = e.org_id, n.entity_type = e.entity_type,
    n.entity_text = e.entity_text, n.confidence = e.confidence,
    n.updated_at = e.updated_at;

UNWIND $rels AS r
MATCH (a:Entity {entity_id: r.entity_a_id})
MATCH (b:Entity {entity_id: r.entity_b_id})
MERGE (a)-[e:REL {rel_id: r.rel_id}]->(b)
SET e.org_id = r.org_id, e.relation_type = r.relation_type,
    e.confidence = r.confidence, e.updated_at = r.updated_at;
```

## 5. Cypher multi-hop query layer + traverse endpoint

New module `graph-index-rs/src/neo4j.rs` (Bolt client via the `neo4rs` crate) exposing:

```rust
async fn traverse(
    org_id: &str,
    seed_entity_ids: &[String],
    max_hops: u8,          // clamped to [1, config.max_hops] (default cap 3)
    max_entities: usize,   // clamped
) -> anyhow::Result<GraphTraversal>  // { entities: Vec<TraversedEntity>, rels: Vec<TraversedRel>, hop: u8 }
```

Backing Cypher (single round-trip vs. Postgres BFS's N+1-per-hop):

```cypher
MATCH (seed:Entity)
WHERE seed.entity_id IN $seed_ids AND seed.org_id = $org
MATCH path = (seed)-[rels:REL*1..$max_hops]-(reached:Entity)
WHERE ALL(r IN rels WHERE r.org_id = $org) AND reached.org_id = $org
WITH reached, rels
LIMIT $max_entities
RETURN collect(DISTINCT reached) AS entities,
       collect(DISTINCT rels)    AS relationships
```

(`*1..$max_hops` is bounded from config; variable-length upper bound is validated to a
small constant to avoid pathological traversals.)

### 5.1 Endpoint

`POST /v1/graph/traverse` on graph-index-rs (`api.rs`), org-scoped + auth'd:

- Request: `{ seed_entity_ids?: [String], seed_query?: String, max_hops?: u8, max_entities?: u32 }`
  (`org_id` comes from the verified `AuthContext`, never the body).
- If `seed_query` is given and `seed_entity_ids` is empty, resolve seeds via the
  existing entity full-text search (org-scoped) first.
- Response: `{ entities: [...], relationships: [...], hops, backend: "neo4j"|"postgres" }`.
- **Fallback:** if Neo4j is disabled/unreachable, the handler transparently calls the
  existing `store.get_graph_expansion` (Postgres BFS) and sets `backend: "postgres"`.
  The endpoint is therefore always available; Neo4j only changes *how fast*.

### 5.2 Provenance / visibility re-join (security-critical)

Neo4j is a topology accelerator, **not** an authorization source. The traverse
result's entity ids are re-validated against Postgres before they leave the plane:
only entities that (a) belong to `org`, and (b) have a `graph_text_units` mapping to a
**live, org-visible** document survive. This reuses the exact `EXISTS (… d.visibility
= 'org' AND d.deleted_at IS NULL)` predicate already in `store.rs`. Consequence: even
a stale or over-broad Neo4j read-model can never leak — the canonical gate is in
Postgres. This mirrors how `orchestrator.rs` step-6 re-gates Qdrant/Quickwit hits
against canonical `documents`.

## 6. Fusion into retrieval (the graph arm)

New arm in `retrieval-engine-rs/src/pipeline/orchestrator.rs`, mirroring `arm_wiki`
/`arm_visual` (non-fatal, gated on `w_graph > 0`) and joining the existing
`tokio::join!` fan-out:

1. **Seed resolution.** Use the dense arm's top chunk `knowledge_id`s → resolve seed
   `entity_id`s whose `source_refs`/`graph_text_units` contain them (org-scoped
   Postgres query; reuses `search/graph.rs` visibility joins). Fallback seed: the
   query-text entity full-text search already in `graph_expansion_search`.
2. **Traverse.** Call graph-index-rs `POST /v1/graph/traverse` (internal service auth,
   org header pinned) for multi-hop neighbours. Non-fatal: any error → empty arm.
3. **Shape adapter (entities → candidates).** Map each traversed entity → its
   org-visible `source_refs` (knowledge_ids) → `knowledge_units` chunks →
   `ScoredCandidate`s (score = entity confidence × hop-decay, so 1-hop facts outrank
   3-hop). This is the "shape adapter from entities/claims → candidates" the Sovereign
   plan calls for.
4. **RRF fuse.** Extend `fuse_arms` with a graph step:
   `if mix.w_graph > 0 && !graph.is_empty() { fused = rrf(&fused, &graph, 60.0, mix.w_graph) }`,
   layered after dense+sparse (peer of wiki/visual). `w_graph` now drives scoring, so
   the `mode_mix` trace stops lying (closes §16.1.1).
5. **Same downstream gates.** The graph candidates flow through the identical step-6
   canonical visibility gate, ZDR filter, and source join as every other arm — no arm
   bypasses ownership/liveness.

ZDR: the graph arm runs no embedding/LLM/rerank egress (it reuses dense results +
local traversal), and it is **skipped** under the same restrictive posture that gates
the embed path (`zdr_mode.restricts_egress()`), so an ephemeral query neither egresses
nor persists via the graph arm.

## 7. Infrastructure

### 7.1 Neo4j in `docker-compose.yml`

- Service `neo4j` on the `aquatiq-local` network (internal DNS `neo4j`), Bolt `7687`,
  optional host-mapped HTTP browser for dev only.
- **Auth on** (`NEO4J_AUTH=neo4j/<secret>`), secret from env, no default in the
  committed file (fail-closed, mirrors the plane's secret handling).
- Healthcheck (`cypher-shell` ping / HTTP `:7474` readiness); `graph-index` depends on
  it `condition: service_healthy` **only when Neo4j is enabled** (compose profile so
  text-only deployments don't require Neo4j).
- Tenant isolation is **logical** (org_id property + query predicate + Postgres
  re-join), not a Neo4j-per-org database — consistent with how Postgres/Qdrant
  multi-tenant here (shared store, org-scoped rows). Documented explicitly as the
  isolation model.

### 7.2 Config (envy, mirrors existing `config.rs` pattern)

`graph-index-rs`: `NEO4J_ENABLED` (bool, default false so the change is additive/safe),
`NEO4J_URL` (`bolt://neo4j:7687`), `NEO4J_USER`, `NEO4J_PASSWORD`, `NEO4J_DATABASE`,
`GRAPH_MAX_HOPS` (default 3), `GRAPH_TRAVERSE_MAX_ENTITIES` (default 100). When
`NEO4J_ENABLED=true` but connection secrets are missing, startup fails closed.

`retrieval-engine-rs`: `GRAPH_INDEX_URL` (traverse endpoint base), reuse existing
internal-service-auth token plumbing; `w_graph` already exists in config.

## 8. Rebuild / backfill

Neo4j is rebuildable from Postgres (it is a read-model): an admin
`POST /v1/graph/neo4j/rebuild?org_id=` (org-scoped, auth'd) streams
`graph_entities`/`graph_relationships` for the org through the same MERGE path. Used
for first-time backfill and after a Neo4j reset. Out of scope for MVP beyond a minimal
org-scoped rebuild; no cross-org bulk in this phase.

## 9. Testing strategy

- **Unit (no infra):** MERGE Cypher builders (param shaping, org scoping present in
  every statement), hop clamping, entities→candidates adapter (hop-decay ordering,
  empty/degenerate inputs), `fuse_arms` graph step (mirrors existing `fuse_*` tests:
  graph layered after dense+sparse; skipped when `w_graph=0` or arm empty; RRF oracle
  matches hand-composed order).
- **ZDR:** restrictive event → zero Neo4j writes; restrictive query → graph arm
  skipped (no traverse call).
- **Org-scoping:** every generated Cypher string contains an `org_id` predicate
  (static assertion test, mirroring `graph.rs`/`wiki.rs`'s `sql.contains(...)` tests);
  traverse endpoint rejects body-org ≠ verified-org.
- **Integration (gated on `NEO4J_TEST_URL`, `#[ignore]` by default like the existing
  disposable-Postgres tests):** dual-write then traverse returns the mirrored subgraph;
  cross-org traversal returns empty; Postgres fallback path returns the same shape when
  Neo4j is off.
- **Gates:** `cargo fmt`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test --workspace`, compose config validation, tenant-isolation static check.

## 10. Phase plan (each: build + tests + conventional commit, no push)

All six phases landed on `claude/priceless-cray-e89074` as incremental
conventional commits; `cargo clippy --workspace -- -D warnings` and
`cargo test --workspace --lib` are green.

1. **Design doc** (this file). ✅
2. **Neo4j infra + client** ✅ — `neo4j` compose service (org-scoped, auth'd,
   healthchecked); `graph-index-rs/src/neo4j.rs` (`neo4rs` Bolt client) + config +
   idempotent constraint/index bootstrap. Behind `NEO4J_ENABLED` (default off);
   fail-closed on missing secret; boot-retry then Postgres-fallback degrade.
3. **Dual-write** ✅ — `merge_extraction` batched `UNWIND`/`MERGE` hooked downstream
   of `persist_extraction`, inheriting the org-visibility + restrictive-ZDR gates;
   idempotent on the Postgres PKs; non-fatal.
4. **Cypher + endpoint** ✅ — `neo4j::traverse` (`*1..N`) + `POST /v1/graph/traverse`
   (auth'd, org-pinned via `require_org`, `store::get_subgraph_visible` provenance
   re-join, transparent Postgres-BFS fallback).
5. **Fusion** ✅ — `arm_graph` in the `tokio::join!` fan-out + `fuse_arms` graph step;
   `w_graph` now drives scoring (closes gap-data §16.1.1); `search/graph.rs::
   graph_arm_candidates` is the entities→candidates adapter.
6. **Eval + docs reconcile** ✅ — gated Neo4j roundtrip test
   (`dual_write_then_traverse_roundtrip_is_org_scoped`, `#[ignore]` on
   `NEO4J_TEST_URL`); roadmap/gap/graph-index notes updated.

### 2026-07-17 follow-up program (RAG full-support)

7. **Remote deep-hop arm** ✅ — `arm_graph` now consumes `POST /v1/graph/traverse`
   over HTTP (`search/graph_remote.rs`): seed entities from the query text →
   traverse (caller's verified bearer forwarded; graph-index re-verifies +
   org-pins) → `(entity, hop)` grounded to org-visible chunks nearest-hop-first;
   every failure degrades to the in-process 1-hop tier. `GRAPH_INDEX_URL` on by
   default in compose.
8. **Communities wired** ✅ — `community.rs` detection is live (visibility-gated
   connected components, two bulk queries, transactional delete-and-replace),
   runs post-extraction + via org-pinned `POST /v1/graph/communities/rebuild`.
9. **Defaults on** ✅ — `NEO4J_ENABLED` default true in compose (fallback posture
   unchanged); `w_graph` 0.2 drives scoring by default; smart hybrid raises it
   on relational queries. `mode_mix_applied` now records the real applied
   weights (the zeroed-graph/wiki note is historical).

## 11. Risks & non-goals

- **Risk: read-model drift.** Mitigated by best-effort dual-write + rebuild endpoint +
  Postgres-as-canonical + Postgres re-join on read (drift can only *lose* recall
  temporarily, never leak or corrupt).
- **Risk: traversal latency inside the retrieval hot path.** Mitigated by the arm
  being non-fatal + bounded hops/entities + concurrent `tokio::join!` (overlaps the
  dense/sparse round-trips) + the p95<800ms gate covering it.
- **Risk: Neo4j as a new SPOF/infra dependency.** Mitigated by the transparent
  Postgres-BFS fallback in the endpoint plus boot-retry — nothing breaks if Neo4j
  is absent. (`NEO4J_ENABLED` defaults ON in the composed stack since 2026-07-17;
  the env flag remains the off-switch and the code default stays false for bare
  binaries.)
- **Non-goals (this program):** Neo4j-per-org physical isolation; LLM→Cypher natural-
  language querying (reference architecture's `text2cypher` — we use structured,
  parameterised traversal only, avoiding the reference's noted Cypher-generation
  accuracy/cost risk); mirroring claims/communities to Neo4j; GDS community detection
  in Neo4j (Postgres `community.rs` stays).
