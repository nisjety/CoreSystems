# Wiki memory

## Product role

Wiki memory materializes **topic pages**, **concept / entity pages**,
**summaries**, and **knowledge logs** from events, memory, and graph memory. It
provides human- and agent-readable synthesized views of what the system "knows"
about a subject, with back-links to source events. It is additive and fully
reproducible from raw events.

## Transport

- **Ingest**: derived asynchronously from events, memory, and graph memory; no
  direct page-write API.
- **Materialize**: Rust synthesizers produce pages with block-level back-links
  (logseq-style), section summaries, and provenance labels.
- **Query**: `GET /v1/knowledge/wiki/*` for page lookup, back-link expansion,
  and summary retrieval. HTTP is GET-only; non-GET returns 405 with
  `Allow: GET`.

## Backend ownership

| Concern | Owner |
|---------|-------|
| Topic / concept / entity page synthesis | `inference-core` |
| Summary generation and knowledge-log assembly | `inference-core` |
| Page storage and back-link index | `inference-core` |
| Catalog, policy, retention, scope, scheduling metadata | `capability-core` |

## `capability-core` responsibilities

- **Catalog** — register synthesizers and page templates as capabilities with
  explicit versions.
- **Policy** — enforce scope (run/thread/workspace/user/org/global) on page
  reads; enforce retention on derived pages.
- **Metadata** — durable records of synthesis runs, template versions, and
  provenance pointers to source events and graph nodes.
- **Scheduling hints** — when to re-synthesize pages, refresh summaries, or
  expire stale knowledge logs, delegated to Rust workers.

## Reference inputs

- `LLM Wiki` — topic- and concept-page materialization patterns.
- `logseq` — block-level, back-linked knowledge log structure.

References only; no code is copied.

## Out of scope

- Free-form user-authored wiki pages (all pages are derived).
- Replacing the event log or memory store as source of truth.
- Synthesizer and storage implementation details (owned by `inference-core`).
