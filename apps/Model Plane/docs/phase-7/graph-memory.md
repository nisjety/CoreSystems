# Graph memory

## Product role

Graph memory captures **entities**, **relationships**, and their **provenance** as
a typed, queryable graph derived from the event log and memory store. It supports
graph-augmented retrieval (GraphRAG-style), multi-hop reasoning, and structured
context assembly. It is additive: the graph can always be rebuilt from raw events.

## Transport

- **Ingest**: derived asynchronously from events on `/v1/memory/*` and run
  transcripts; no direct write API. Materialization is triggered by
  `capability-core` scheduling hints.
- **Materialize**: Rust extractors produce nodes (entities) and edges
  (relationships) with provenance labels (source event IDs) and confidence
  labels (extractor score, corroboration count).
- **Query**: `GET /v1/knowledge/graph/*` for node lookup, neighborhood
  expansion, and typed traversal. HTTP is GET-only; non-GET returns 405 with
  `Allow: GET`.

## Backend ownership

| Concern | Owner |
|---------|-------|
| Entity / relationship extraction | `inference-core` |
| Graph materialization and storage | `inference-core` |
| Query engine (traversal, GraphRAG) | `inference-core` |
| Catalog, policy, retention, scope, scheduling metadata | `capability-core` |

## `capability-core` responsibilities

- **Catalog** — register graph extractors and materialization jobs as capabilities
  with explicit versions.
- **Policy** — enforce scope (run/thread/workspace/user/org/global) on graph
  reads and writes; enforce retention on derived nodes/edges.
- **Metadata** — durable records of extraction runs, extractor versions, and
  provenance pointers to source events.
- **Scheduling hints** — when to re-extract, re-materialize, or expire graph
  partitions, delegated to Rust workers.

## Reference inputs

- `graphify` — entity/relationship extraction pipelines.
- `GraphRAG` — graph-augmented retrieval semantics.

References only; no code is copied.

## Out of scope

- Replacing the event log or memory store as source of truth.
- Free-form user writes to the graph (all nodes/edges are derived).
- Graph engine implementation details (owned by `inference-core`).
