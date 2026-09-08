# ADR-0004: Graphiti is not adopted; take validity intervals only

**Date**: 2026-09-07
**Status**: proposed — records a recommendation with its evidence; no
implementation is authorized by this document
**Scope**: Data Plane v2 (`graph-index-rs`). No other plane is affected.
**Deciders**: Data Plane v2 implementation owners (pending)

## Context

[getzep/graphiti](https://github.com/getzep/graphiti) was raised as a candidate
addition to Data Plane v2's knowledge-graph stack. Verified against the upstream
repository on 2026-09-07:

- Apache 2.0. **Python only** (3.10+; some features require 3.12+).
- Backends: Neo4j 5.26+, FalkorDB 1.1.2+, Amazon Neptune, Kuzu (deprecated
  upstream, slated for removal).
- Data model: Entities with evolving summaries, Facts/Relationships as edges
  **carrying temporal validity windows**, Episodes retaining raw source as
  ground truth, custom types via Pydantic.
- **Bi-temporal**: facts record when they became true and when superseded;
  superseded facts are *invalidated rather than deleted*.
- Incremental updates without batch recomputation (contrast Microsoft GraphRAG).
- **Requires an LLM with Structured Output.** Defaults to OpenAI; supports
  Anthropic, Gemini, Groq, and OpenAI-compatible endpoints. Upstream warns that
  smaller/local models frequently emit non-conformant JSON and break extraction.
- Multi-tenancy via a `group_id` property, with minimal detail in the docs.
- Deployable as a library, an MCP server, or a FastAPI service.
- Concurrency defaults to 10 operations to avoid provider rate limits.

The proposal is attractive at the feature level because Graphiti's headline
capabilities read as a superset of ours. Read against the actual code, that is
mostly not true — and where it is true, the mechanism Graphiti uses is one this
service has already considered and declined in writing.

### What `graph-index-rs` already does

All verified by reading the service on 2026-09-07:

| Concern | Where | State |
|---|---|---|
| Entity/relationship/claim extraction | `src/extractor.rs` | **LLM-based**, routed through Model Plane `inference-core` gRPC (`Backend::ModelPlane`), with `inference_auth::RetentionPosture` threaded through. Not a direct provider call. |
| Graph traversal store | `src/neo4j.rs` | Neo4j already integrated as a **rebuildable, org-scoped read-model** mirroring canonical Postgres. Explicitly "never an authorization source"; every statement carries an `org_id` predicate; `MERGE` on the Postgres PK. |
| Canonical persistence | `src/store.rs` | Postgres (`graph_entities`, `graph_relationships`, `graph_claims`, `graph_communities`, `graph_text_units`). Org-visibility + restrictive-ZDR gated in `persist_extraction`. |
| Contradiction detection | `src/contradiction.rs` | Live writer for `contradicted_by_claim_ids` / `claim_status`. Structural, high-precision: negation-swap and number-swap, bilingual (12 negation markers, EN + NO). |
| Communities | `src/community.rs` | Live. Visibility-gated connected components, two bulk queries, member-set-derived ids so summaries survive re-detection. |
| Entity identity | `src/store.rs` | `normalize_identity` (trim, collapse whitespace, lowercase) → UUID v5 over composite key. Closed 18-type ontology. |
| Extraction healing | `src/reconcile.rs` | Reads database truth rather than replaying events, because envelope TTL cannot cover extraction latency. |
| Retrieval fusion | `retrieval-engine-rs` | Graph arm folded into RRF; `w_graph` 0.2 by default, raised by smart hybrid on relational queries. |

`docs/graphrag-neo4j-plan.md` §0 records the governing decision — *extend*
`graph-index-rs`, do not stand up a parallel extraction pipeline or a second
store of record — and §11 lists per-org physical Neo4j isolation and
LLM→Cypher (`text2cypher`) as explicit **non-goals**, the latter for
Cypher-generation accuracy and cost reasons.

### The measured constraint

`src/reconcile.rs` documents the throughput reality: graph extraction is **one
inference call per chunk, tens of seconds per document**, against a 120s signed
envelope TTL (300s ceiling). Measured 2026-08-26: **12 documents announced, 2
extracted, 10 discarded** as expired. Three alternatives (accepting expired
envelopes on `jti` replay protection, minting a fresh envelope per chunk, and
re-announcing) were each evaluated and rejected as unsound, and a DB-truth
reconciler was built instead.

This is the decisive fact. Graphiti performs *more* LLM work per episode than
the pipeline that already loses 10 of 12 documents — extraction, plus entity
resolution, plus invalidation adjudication, plus community rebuild — with
concurrency capped at 10 to avoid rate limits.

## Decision

**Do not adopt Graphiti**, in any of its three deployment shapes, into Data
Plane v2's serving or ingestion path.

**Adopt one idea from it**: validity intervals on `graph_claims`, and
explicitly *not* its automatic invalidation.

These two are separable, and conflating them is the trap. Graphiti bundles a
storage model (facts carry validity windows; superseded facts remain queryable)
with a mechanism (an LLM decides what supersedes what, and the edge is
invalidated automatically). The storage model is compatible with this service's
posture. The mechanism is one `src/contradiction.rs` already rejected, with a
rationale written into the module:

> No auto-supersede. The reference implementation this borrows from
> (`alash3al/stash`, Apache-2.0) demotes an older fact automatically when an LLM
> classifies the pair as a *replacement* with confidence ≥ 0.9. Without a
> confidence signal there is no safe threshold, so both claims stay live and are
> merely flagged.

and, on the precision/recall trade:

> A false contradiction flag is worse than a missed one … Low recall is visibly
> incomplete; low precision is quietly corrosive.

That reasoning is correct for a citations product and this ADR does not reopen
it. The correct seam for a semantic pass already exists: the
`ClaimAdjudicator` trait, which slots a Model-Plane-backed adjudicator behind
the current structural detector without touching callers, once inference can
return a calibrated confidence.

## What Graphiti would and would not add

| Graphiti capability | Data Plane v2 today | Verdict |
|---|---|---|
| Temporal validity windows on facts | **Absent.** `graph_claims` has `claim_status` (`active`/`contradicted`) + `contradicted_by_claim_ids` — a status flag, not an interval. No `valid_at`/`invalid_at`/`valid_from` in any DP2 migration. Unplanned: no bi-temporal or claim-validity item anywhere in `apps/Data Plane v2/docs/` — the only "point-in-time" references there are Postgres PITR for backup/restore, which is unrelated. | **Real gap. Take the schema.** |
| Alias/entity resolution beyond normalization | **Partially absent, and already designed.** `normalize_identity` handles exact-match-after-normalization only; the module says merging `"Sarah Chen"` with `"SC"` "needs the embedding clustering stage (plan P1-2 step 2) and must not be faked here." | **Real gap, already sequenced.** Do not import a competing implementation. |
| Incremental update without batch recompute | Already incremental — per-chunk extraction on a JetStream consumer; `community.rs` is idempotent at row level. | No gain. |
| Hybrid retrieval (semantic + BM25 + graph) | Already multi-arm RRF fusion with a graph arm at `w_graph` 0.2. | No gain. |
| Communities | Live, visibility-gated. Community *summarisation* (P1-5) is the pending piece, not detection. | No gain. |
| Episode-as-ground-truth provenance | Covered by `graph_text_units`, `source_refs`, and the provenance column. | No gain. |
| LLM extraction | Already LLM-based **and** sovereign-routed via Model Plane with retention posture. | No gain; Graphiti is a regression here (defaults to direct OpenAI). |
| Graph database backend | Neo4j already integrated, as a non-authoritative read-model. | No gain; Graphiti is weaker (graph is authoritative, partitioned by a soft `group_id` property). |

Both genuine gaps map onto work this codebase has already identified. P1-2
(`docs/retrieval-quality-and-durability-plan-2026-08-05.md:538`) specifies the
entity-resolution ladder in full — *UUID v5 over normalized `(org, text, type)`
→ alias clustering via `entity_summary_embeddings` → LLM adjudication on
borderline pairs* — at priority D7, noting it requires a coordinated re-extract
plus `POST /v1/graph/rebuild` because it changes every `entity_id`, which
`graph_relationships`, `graph_text_units`, `graph_communities` and Neo4j's
unique constraint all reference. Rung 1 shipped; rungs 2 and 3 are the gap.

Graphiti would therefore not supply a missing idea. It would supply a competing
implementation of a designed-but-unbuilt rung, in a second language, using the
LLM-adjudication mechanism this service defers until it has a confidence signal.

## Alternatives considered

**(a) Adopt as a FastAPI service.** Rejected. The graph algorithms are not the
cost; the plane contract is. Every DP2 service implements strict JWT/JWKS with
fail-closed startup, claim-pinned org (never a body `org_id`), per-org RLS via
`begin_org_scoped`, Ed25519-signed event envelopes with a `jti` replay cache,
durable outbox with PubAck, DLQ on every consumer, scoped GDPR erasure
consumers, and `admin_audit_log` receipts. Graphiti knows none of it, and it
would become the plane's first **stateful, tenant-scoped** Python service.
Python already runs here in production, but only as a stateless inference
sidecar: `colqwen-reranker` is a FastAPI app exposing `POST /rerank
{query, image_urls}` → scores plus `/healthz`, with no Postgres, NATS, JWT,
`org_id`, or ZDR handling anywhere in it, because it holds no tenant data.
(`retrieval-eval-py` is a scaffold; `data-quality-go` is the live eval
surface.) That distinction is the whole argument: the contract obligations
above attach to services that persist tenant data, and Graphiti would be the
first Python service on that side of the line.
Additionally, ZDR would have to be threaded through episode ingestion, entity
resolution, and community building, with a purge path satisfying the erasure
saga — and DP2's own status docs record that strict per-operation mutation
telemetry is still missing for Postgres/Qdrant/Quickwit/MinIO, so adding a
store widens an unproven ZDR surface before it is closed.

**(b) Adopt as a library inside a Python sidecar.** Rejected for the same
contract reasons, plus the throughput finding above.

**(c) Adopt the MCP server for agent memory.** Out of scope here, and not
rejected on the merits. If the requirement is per-user agent memory rather than
corpus RAG, that belongs near Model/Application plane agent state, not inside
the durable-knowledge plane, and it would still need its own ZDR and tenancy
story. A separate ADR should decide it if the requirement materialises.

**(d) Take the validity-interval schema; build it in `graph-index-rs`.**
Chosen. It is a schema decision, not a dependency.

## Consequences

- `graph_claims` gains a validity interval. Both sides of a contradiction stay
  live and queryable, each with a window, enabling "what did we believe on date
  X" without demoting anything. `claim_status` semantics are unchanged, and the
  existing rule that re-extraction must not revert a status is preserved.
- No new language, service, container, CI surface, or supply chain.
- No change to authority, tenancy, or ZDR posture; Postgres stays canonical and
  Neo4j stays a non-authoritative read-model.
- The entity-resolution gap remains open and stays sequenced as P1-2 rungs 2–3
  at D7. This ADR does not accelerate it and explicitly does not substitute
  Graphiti for it.
- Extraction throughput remains the binding constraint on graph quality. Any
  future proposal that adds per-chunk or per-pair LLM calls must state its
  effect on the 2026-08-26 measurement (12 announced / 2 extracted) or it
  cannot be evaluated.
- Reversible. Nothing here forecloses adopting Graphiti later if the throughput
  picture changes and a calibrated-confidence adjudicator lands.

## Note for future reviewers

An initial informal review of this proposal reached partly the wrong conclusion
by reading `index-engine-rs/src/extract/markdown.rs` — a regex heading/link
extractor — and inferring that graph entity extraction was regex-based. It is
not: the graph's extractor is `graph-index-rs/src/extractor.rs`, and it is LLM
based via Model Plane. Two separate extractors exist for two separate purposes.
Reviewers comparing DP2 against an external graph framework should read the
graph service's own extractor, not the index engine's.
