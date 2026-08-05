# Plane Communication & Freshness

Verified 2026-05-29 from compose build wiring, `.proto` service defs, NATS subject
constants, and pub/sub call sites. Scope: **Model Plane v1** (canonical; v2 deprecated),
**Data Plane v2**, **Ingestion Plane** (finspo, integration, imports), **Quarry v2**.

## Transports

| Transport | Used for |
|---|---|
| **gRPC** | Sync service-to-service. Intra-Model-Plane (model-gateway → session-core:9091, inference-core:9092, execution-core:9093, capability-core:9097, orchestrator-core:8084, sandbox-manager, browser-broker, letta-bridge:9096). Model→Data via `DATAPLANE_RETRIEVAL_ADDR`, `DATAPLANE_GRAPH_ADDR`. |
| **NATS (core + JetStream)** | Async events on the shared `nats:4222`. `verevon-nats:4222` is a **separate** bus for Verevon/Frontend integration + usage/audit (`verevon.usage.v1.*`). |
| **HTTP/REST** | Cross-plane edges: Quarry-edge → Model (`ai-core:8001`) + Data (`dpv2-documents-api:8010`, `dpv2-retrieval-engine:8004`); Model gateway → Quarry-edge (`QUARRY_EDGE_URL`); finspo/integration → `dpv2-documents-api`. |
| **Temporal** (`temporal:7233`) | Workflow orchestration: orchestrator-core, quarry-control/orchestrator. |
| **Postgres / Redis** | Per-service DBs (session_core, quarry_v2, finspo, imports, integration). Redis: Quarry job store + (new) Model-Plane LangCache/Agent Memory. Vectors = **Qdrant**; FTS = **Quickwit**. |

## Cross-plane flows

1. **Ingestion → Data v2** — quarry-edge, finspo-api, integration-api push docs (HTTP) to `dpv2-documents-api`. Quarry also feeds via NATS `quarry.documents.crawled` (documents-api subscribes).
2. **Data v2 indexing pipeline (NATS)** — `documents.created` → **index-engine-rs** (chunk) → `knowledge.units.created` → **embedding-engine-rs** (embed → Qdrant) → `documents.indexed` → **quickwit-adapter-rs** (FTS). `documents.deleted` fans out to all. Wiki: `wiki.version.published` → embedding wiki_consumer.
3. **Model v1 → Data v2** — agent → gRPC `RetrievalService.Retrieve` / `DocumentService` / `KnowledgeService.CheckPermissions`.
4. **Model v1 → Ingestion** — model-gateway → HTTP Quarry-edge (live Fetch/ExtractStructured). **Quarry → Model** — quarry-edge → `ai-core` (LLM extraction).
5. **Events** — `mp.v1.*` run/feedback, `verevon.usage.v1.<plane>.<op>` usage, `dataplane.cost.ledger` (data-orchestrator consumes).

## Document lifecycle subjects (verified)

| Subject | Publisher | Consumers |
|---|---|---|
| `dataplane.documents.created` | documents-api `PublishDocumentCreated` | index-engine-rs |
| `dataplane.documents.deleted` | documents-api `PublishDocumentDeleted` | embedding-engine, index-engine, quickwit-adapter |
| `dataplane.knowledge.units.created` | index-engine-rs | embedding-engine, quickwit-adapter |
| `dataplane.documents.indexed` | embedding-engine-rs | quickwit-adapter; data-orchestrator |
| `dataplane.wiki.version.published` | wiki-store-go | embedding-engine wiki_consumer, quickwit-adapter |
| `dataplane.source_objects.changed` | **NONE** ⚠️ | quickwit-adapter (dangling) |
| `dataplane.source_objects.deleted` | **NONE** ⚠️ | quickwit-adapter (dangling) |
| `quarry.documents.crawled` | Quarry | documents-api subscriber |

Reindex is **HTTP-only**: `data-orchestrator-go` `POST /reindex`, `/jobs`, `/stale-embeddings` (`JobType` = reindex|graph_build|wiki_refresh). No event triggers a reindex.

## Freshness gap (RDI)

**There is no `documents.updated` / `source_objects.changed` *publisher*.** `documents-api/internal/events/publisher.go` exposes only Created + Deleted; it declares `SubjectSourceObjectChanged/Deleted` consts + `DefaultSourceObjectSubjects()` but **no publish method**, and quickwit-adapter already subscribes to `source_objects.changed`. So when a source doc's **content changes** (e.g. finspo SharePoint delta re-sync of a modified file), nothing re-embeds or re-indexes it — retrieval serves stale text until the doc is deleted+recreated. This is the "fast stale store" failure mode.

## Implemented (2026-05-29)

End-to-end content-update freshness now flows on the shared `nats:4222`, with each retrieval surface purging superseded content:

- **P1 (documents-api-go)** ✅ — `repo.Create` now updates the row in place when a same-idempotency-key re-ingest carries changed content (was a silent `Reused` no-op → permanent staleness) and emits **`dataplane.documents.updated`** (`Create` via publisher, `BulkIngest` via outbox). New subject + `DocumentUpdatedEvent` + `PublishDocumentUpdated`. Unit-tested gating helper.
- **P2 (index-engine-rs + embedding-engine-rs)** ✅ — index-engine consumes `documents.updated` (routed through `process_document`), now **fetches content from Postgres** (events carry no body — fixed a latent gap), re-chunks, and publishes **`dataplane.knowledge.units.deleted`** for orphaned chunks (old−new, deterministic `stable_chunk_id`). embedding-engine re-embeds (Qdrant upsert by id) and adds `delete_vectors_by_ids` to purge orphan vectors. Unit-tested orphan diff.
- **P3 (quickwit-adapter-rs)** ✅ — on `documents.indexed` (re-fires after a re-embed) it now **deletes the doc's FTS entries before re-indexing** from the DB → no duplicate or orphaned chunks; needs no extra event.
- **P4 (documents-api-go + finspo)** ✅ — `source_objects.changed` now carries `content_hash` + `content_changed` (repo reports `ContentChanged` via a prior-hash CTE) so consumers skip vector work on metadata-only delta touches while FTS still refreshes. finspo's delta sink already posts source-object upserts on modified items (verified).
- **P5 (graph-index-rs)** ✅ — re-extraction already re-fires via `documents.indexed`; added a core-NATS subscriber for `knowledge.units.deleted` that purges orphaned `graph_text_units` mappings (`delete_text_unit_mappings`), keeping superseded entities/relationships out of graph retrieval. (Chose this over a `data-orchestrator` reindex job, which would double-process and risk a publish loop with P2.)

All on shared `nats:4222`; keeps Qdrant + Quickwit + graph; adds no new system.

**Deploy note:** the JetStream durable consumers `index-engine` (added `documents.updated`) and `embedding-engine` (added `knowledge.units.deleted`) changed their `filter_subjects`, and the `DATAPLANE_DOCUMENTS` / `DATAPLANE_KNOWLEDGE` streams gained subjects. `get_or_create_consumer` won't mutate an existing durable consumer's filter — on an existing deployment, delete + recreate those two consumers (and update the stream subject lists) so the new subjects are delivered. graph-index uses core NATS for `knowledge.units.deleted` (no consumer change needed).

**Follow-ups:** GC for now-unreferenced graph entities/relationships/claims; fold cache-hit + cost events into one ledger; NATS bus unification (`nats` vs `verevon-nats`).

## Related

- NATS bus fragmentation (`nats` vs `verevon-nats` vs shared/local) — separate "bus unification" track; not required for RDI above.
- Cost-ledger unification (`dataplane.cost.ledger` + `verevon.usage.v1.*` + LangCache cache-hit events) — fold into one schema/consumer.
- LangCache (Rust model-gateway) + Agent Memory (Go letta-bridge) + retrieval router (retrieval-engine-rs) already landed.
