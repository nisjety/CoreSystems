# Quarry v2 — Cross-Plane Integration

This document shows how Quarry v2 connects to the other CoreSystem planes (Control Plane, Data Plane v2, Model Plane) and the trust/auth boundaries between them.

## Plane responsibilities (canonical)

```
Quarry v2  → captures evidence (fetch, render, observe, fingerprint, diff)
Data Plane → owns durable knowledge (chunks, embeddings, graph, wiki, retrieval)
Model Plane → owns reasoning (planning, agent loops, structured extract, synthesis)
Control Plane → owns identity, org, billing, auth, NATS event spine
App / Shell → human-facing UX (CLI, IDE, channels, dashboards)
```

## Wire diagram

```
┌─────────────┐      AcquireGrant / ValidateGrant (gRPC)       ┌──────────────┐
│ Model Plane │  ◄────────────────────────────────────────►   │  Quarry edge │
│ browser-broker                                                 │              │
│             │                                                  │  /v1/scrape │
│             │  /v1/invoke (HTTP) — planner, structured,        │  /v1/crawl  │
│             │   summary, json, query, ranker                   │  /v1/agent  │
│             │                                                  │  /v1/search │
└─────────────┘                                                  └──────┬───────┘
                                                                        │
                                                                        │ /v1/ingest
                                                                        │  (HTTP or
                                                                        │   gRPC DocumentService)
                                                                        ▼
                                                                ┌──────────────┐
                                                                │ Data Plane v2│
                                                                │ documents-api│
                                                                │ index-engine │
                                                                │ retrieval    │
                                                                └──────┬───────┘
                                                                       │
                                                                       │
                ┌─────────────────────────────────┐                    │
                │ NATS JetStream                  │ ◄──────────────────┘
                │ subjects:                       │ ◄── quarry.run.<id>.<event>
                │   quarry.events.*               │ ◄── quarry.events.<event>
                │   organization.*                │ ◄── (Control Plane)
                │   user.*                        │ ◄── (Control Plane)
                └─────────────┬───────────────────┘
                              │
                              ▼
                ┌─────────────────────────────────┐
                │ Convex / App Plane              │
                │ (read-only projection)          │
                └─────────────────────────────────┘
```

## Auth + identity flow

1. **Org / User → Control Plane**: auth-core + org-core own identity. Quarry never authenticates users directly.
2. **Quarry edge** validates `Authorization: Bearer <token>` against Control Plane (or accepts internal-network tokens).
3. **Quarry → Data Plane**: Bearer-tokened, scoped to the request's `org_id`. Data Plane re-validates org status.
4. **Quarry → Model Plane**: Bearer-tokened, gateway returns an `org_id`-scoped session.
5. **Model Plane → Quarry browser**: `BrowserBrokerService.AcquireGrant` issues a short-lived `grant_id`. Quarry validates each action with `ValidateGrant` before execution.

## ZDR propagation

ZDR (zero data retention) flows down every contract:

| Boundary | ZDR field | Effect when on |
|---|---|---|
| Quarry edge route | `request.zdr: true` | Skip Redis cache write |
| `PageRunner` | `pipeline.zdr` | Skip artifact persistence |
| `IngestClient` | `DataPlaneIngestRequest.zdr` | Reject with `Forbidden` when payload present |
| `StructuredExtractClient` | `StructuredExtractRequest.zdr` | Validate inline-only mode |
| `AgentActionRequest` | `request.zdr` | AgentLoop skips persistent profile snapshots |

Bottom line: a single `zdr: true` at the boundary propagates through every cross-plane call so no plane accidentally persists.

## Event flow

### Quarry → NATS JetStream

Stream: `QUARRY_EVENTS`
Subjects: `quarry.run.<run_id>.<event_type>` (per-run scope) AND `quarry.events.<event_type>` (aggregate).

Event types:

| Event | When | Payload |
|---|---|---|
| `run_started` | Run accepted | run_id, request hash |
| `page_fetched` | Page fetched OK | url, status, fingerprint, driver, latency_ms |
| `page_failed` | Page fetch failed | url, error_code, attempt count |
| `artifact_written` | Artifact stored | artifact_id, format, bytes |
| `change_detected` / `change_unchanged` | Diff vs previous | prev_fingerprint, new_fingerprint |
| `store_record_written` | Data Plane ingest acknowledged | document_id, index_status |
| `agent_started` / `agent_completed` / `agent_failed` | Agent loop lifecycle | run_id, step_count, termination |
| `action_started` / `action_completed` / `action_failed` | Per-step in agent loop | action variant, observation snapshot |
| `lease_acquired` / `lease_released` | Browser lease lifecycle | lease_id, profile_id |
| `profile_captured` / `profile_restored` | Profile lifecycle | profile_id, viewport |

### Control Plane → Quarry

Subjects: `organization.*`, `user.*`. Quarry does NOT re-publish these — it consumes them when an org changes plan or is deleted (to gate or revoke active runs).

## gRPC service map

| Service | Owner | Endpoint | Quarry caller |
|---|---|---|---|
| `BrowserBrokerService` | Model Plane | `:9090` | `GrpcGrantValidator` |
| `DocumentService` (v2) | Data Plane | `:9001` | `GrpcDataPlaneClient` |
| `InferenceCoreService` | Model Plane | `:9092` | (via gateway HTTP) |

When the `grpc` feature is enabled:

```bash
cargo build -p quarry-runtime --features grpc
```

The protos live vendored at `crates/quarry-runtime/proto/`. Update via `tools/sync-protos.sh` if the upstream contracts change.

## Data Plane ingest contract

Request (`DataPlaneIngestRequest`):

```json
{
  "run_id": "run_01HX...",
  "org_id": "org_acme",
  "source_url": "https://example.com/page",
  "title": "Example",
  "markdown": "# Hello\n...",
  "html_ref": "artifact_01HX...",
  "raw_ref": "artifact_01HY...",
  "chunks": [...],
  "metadata": {...},
  "fingerprint": "blake3:abc...",
  "zdr": false,
  "retention_policy": null,
  "source_trace": {
    "fields": {
      "title": {"selector": "head > title", "extracted_at": "..."},
      "body": {"selector": "article", "extracted_at": "..."}
    }
  }
}
```

Response (`DataPlaneIngestResponse`):

```json
{
  "document_id": "doc_01HX...",
  "index_status": "indexed",
  "knowledge_unit_count": 24,
  "embedding_status": "embedded",
  "retrievable_after": "2026-05-08T10:00:00Z",
  "trace_id": "trace_..."
}
```

When ZDR=on, the request must omit `markdown`, `html_ref`, `raw_ref`. Data Plane returns `index_status: "skipped"`, `embedding_status: "skipped"`.

## Model Plane structured extract contract

Request (`StructuredExtractRequest`):

```json
{
  "run_id": "run_01HX...",
  "org_id": "org_acme",
  "source_artifact_ref": "artifact_01HX...",
  "markdown": "# Hello...",
  "structured_output_schema": {"type": "object", ...},
  "source_trace_required": true,
  "max_cost_usd": 0.10,
  "max_tokens": 2000,
  "zdr": false
}
```

Response includes `data` (validated JSON), `schema_valid`, `usage.cost_usd`. If `usage.cost_usd > max_cost_usd`, Quarry rejects with `Forbidden` and surfaces the budget violation in events.

## Operational checks

| Check | Expectation |
|---|---|
| Quarry edge healthy | `GET http://edge:8082/health` → 200 |
| NATS reachable | `nats stream info QUARRY_EVENTS` succeeds |
| Browser broker reachable | `grpcurl model-plane-broker:9090 model_plane.v1.BrowserBrokerService/Health` |
| Data Plane ingest reachable | `curl http://documents-api-go:9001/health` |
| Model Plane gateway reachable | `curl http://model-gateway:8080/health` |

## Failure boundaries

- Data Plane ingest failure → scrape still succeeds; retried later via Temporal workflow.
- Model Plane invoke failure → AgentLoop falls back to `LexicalRanker` for ranking; surfaces `agent.failed` for active loops.
- NATS unavailable → events buffered in process up to channel capacity, then dropped (logged at WARN).
- Browser broker unavailable → Quarry refuses agent action with `Forbidden`. Production deploys never set the validator to `Noop`.
