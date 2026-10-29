# Phase 7 — Knowledge Plane

This directory documents the **knowledge layers** that sit on top of raw events and
memory. Knowledge artifacts are treated as **additive** — they enrich retrieval and
reasoning, but never replace the source-bearing truth stored in the event log,
memory store, or run transcripts.

## Scope

Phase 7 defines the contracts for graph memory, wiki memory, contradiction
handling, and knowledge lint. It does **not** add new source-of-truth stores.
Every knowledge artifact is derived, reproducible from raw events, and carries
explicit provenance. Materialization is owned by the Rust data planes;
`capability-core` owns the catalog, policy, retention, scope control, and
scheduling metadata for knowledge jobs.

## Layers

| Layer | Document | Primary backend owner |
|-------|----------|-----------------------|
| Graph memory | [`graph-memory.md`](./graph-memory.md) | `inference-core` (materialization) + `capability-core` (catalog/policy) |
| Wiki memory | [`wiki-memory.md`](./wiki-memory.md) | `inference-core` (materialization) + `capability-core` (catalog/policy) |
| Contradiction handling | [`contradiction-handling.md`](./contradiction-handling.md) | `inference-core` (verb evaluation) + `capability-core` (policy) |
| Knowledge lint | [`knowledge-lint.md`](./knowledge-lint.md) | `capability-core` (scheduling) + `inference-core` (checks) |

## Reference inputs

Prior art consulted when drafting these layers:

- `graphify` — entity/relationship extraction into typed graphs.
- `GraphRAG` — graph-augmented retrieval over extracted entities.
- `LLM Wiki` — topic- and concept-page materialization patterns.
- `logseq` — block-level, back-linked knowledge log structure.

These are **references only**. No code is copied; the contracts described here are
owned by the Model Plane.

## Acceptance

Phase 7 is complete when every layer above:

1. Produces **additive** knowledge artifacts, never replacements for source-bearing
   truth (events, memory, run transcripts).
2. Carries explicit provenance (source event IDs) and confidence labels.
3. Is reproducible from raw events; the knowledge store can be dropped and
   rebuilt without loss.
4. Names an explicit backend owner and enumerates the `capability-core`
   responsibilities it depends on (catalog, policy, retention, scope, scheduling).
5. Does **not** require changes to the frozen Phase 0 specifications.
