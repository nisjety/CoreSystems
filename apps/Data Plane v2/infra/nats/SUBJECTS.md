# NATS subject + stream contract — DPv2

> **Source of truth** for every JetStream stream and every NATS subject
> DPv2 uses. Adding a new subject or stream means editing this file
> first. CI runs `scripts/check-subjects.sh` to assert every constant
> defined in source matches a row below; drift fails the build (§17.3.3).

## Hard rule: one subject → one stream

JetStream rejects a subject that maps to more than one stream. The
DATAPLANE_KNOWLEDGE / DATAPLANE_GRAPH collision during Docker bring-up
was a missing-contract bug. This file makes it impossible to land
another one without a contract update.

## Streams

| Stream | Subjects | Retention | Owner | Notes |
|---|---|---|---|---|
| `DATAPLANE_DOCUMENTS` | `dataplane.documents.created`, `dataplane.documents.deleted`, `dataplane.documents.updated` | WorkQueue, 7d | documents-api-go (producer) → index-engine-rs, retrieval-engine-rs cache invalidator (consumers) | Document lifecycle |
| `DATAPLANE_KNOWLEDGE` | `dataplane.knowledge.units.created` | WorkQueue, 7d | index-engine-rs (producer) → embedding-engine-rs (consumer) | Chunk-level events |
| `DATAPLANE_GRAPH` | `dataplane.graph.extraction.requested` | WorkQueue, 7d | (planned) → graph-index-rs (consumer) | Entity extraction trigger |

## Core (non-JetStream) subjects

These are core-NATS subjects — no durable consumer, no persistence.
Best-effort delivery is acceptable for the use case.

| Subject | Producer | Consumer(s) | Payload schema | Notes |
|---|---|---|---|---|
| `dataplane.wiki.version.published` | wiki-store-go | embedding-engine-rs (`wiki_consumer`) | [`docs/schemas/wiki_events.md`](../../docs/schemas/wiki_events.md) | Wiki publish → write-through to `wiki_block_embeddings` |
| `dataplane.wiki.page.deleted` | wiki-store-go (reserved) | embedding-engine-rs (reserved) | TBD | Not yet emitted |
| `dataplane.source_objects.changed` | connector ingesters / documents-api-go outbox | quickwit-adapter-rs | `{source_object_id, org_id}` or full source-object payload | Best-effort live update; adapter can rebuild from Postgres |
| `dataplane.source_objects.deleted` | connector ingesters / documents-api-go outbox | quickwit-adapter-rs | `{source_object_id, org_id}` | Best-effort live delete; adapter can rebuild from Postgres |
| `dataplane.search.rebuild.requested` | operator/admin | quickwit-adapter-rs | `{org_id?, clear?: boolean}` | Triggers derived Quickwit rebuild from Postgres |
| `dataplane.cost.ledger` | retrieval-engine-rs, embedding-engine-rs | data-orchestrator-go (`cost.consumer`) | `{event_type, model, count, estimated_tokens, org_ids, user_id, idempotency_key}` | Per-call cost events |
| `dataplane.documents.indexed` | embedding-engine-rs, data-orchestrator-go | graph-index-rs (`stream.rs`) | `{document_id, org_id, indexed_at}` | Signal that a document's vectors are now queryable |
| `dataplane.dlq.embedding-engine` | embedding-engine-rs | (operator via `dlq-replay`) | Original payload + `{error, attempts}` | Dead-letter |
| `dataplane.dlq.index-engine` | index-engine-rs | (operator via `dlq-replay`) | Same shape | Dead-letter |
| `dataplane.dlq.graph-index` | graph-index-rs | (operator via `dlq-replay`) | Same shape | Dead-letter |
| `dataplane.dlq.*` | any consumer at max-retries | (operator via `dlq-replay`) | Same shape | DLQ convention |

## Constants

Every constant below is the canonical Go/Rust name → subject mapping.
Source files MUST use these names; CI rejects ad-hoc string literals.

### Go (`services/*/internal/events/publisher.go`)
- `SubjectDocCreated` = `dataplane.documents.created`
- `SubjectDocDeleted` = `dataplane.documents.deleted`
- `SubjectWikiPublished` = `dataplane.wiki.version.published`
- `SubjectWikiDeleted` = `dataplane.wiki.page.deleted`
- `SubjectSourceObjectChanged` = `dataplane.source_objects.changed`
- `SubjectSourceObjectDeleted` = `dataplane.source_objects.deleted`

### Rust (`services/*/src/stream/mod.rs`, etc.)
- `STREAM_NAME` = one of `DATAPLANE_DOCUMENTS`, `DATAPLANE_KNOWLEDGE`, `DATAPLANE_GRAPH`
- `SUBJECT_CREATED` = depends on stream (see above)
- `SUBJECT_DELETED` = `dataplane.documents.deleted`
- `SUBJECT_DOC_UPDATED` = `dataplane.documents.updated`
- `SUBJECT_WIKI_PUBLISHED` = `dataplane.wiki.version.published`
- `SUBJECT_SOURCE_OBJECT_CHANGED` = `dataplane.source_objects.changed`
- `SUBJECT_SOURCE_OBJECT_DELETED` = `dataplane.source_objects.deleted`
- `SUBJECT_SEARCH_REBUILD_REQUESTED` = `dataplane.search.rebuild.requested`
- `DLQ_SUBJECT` = `dataplane.dlq.<consumer-name>` (convention: replace `-` with `_` only if the consumer name has it)

## Adding a new subject

1. Pick a stream (or decide it's core-NATS).
2. Add a row to the relevant table above.
3. Define a typed constant in the producing service's `internal/events`
   (Go) or `src/stream/mod.rs` (Rust) — never inline the string.
4. Re-run `make check-subjects` locally.
5. PR includes the contract diff + the producer change; CI gate enforces.

## Anti-patterns rejected by `check-subjects`

- Inline `nc.publish("dataplane.foo", ...)` with a literal — must use a named constant.
- Two streams claiming the same subject — JetStream will reject at runtime; we want to catch it at PR time.
- A subject name that doesn't start with `dataplane.` — DPv2 owns that prefix.
