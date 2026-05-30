# Wire reconciliation — proto contracts vs. real HTTP / storage surface

> Companion to `WIRE_SURFACE_PLAN.md`. As we landed gRPC alongside HTTP, several
> proto messages turned out not to match what the service actually stores or
> exposes. This catalogs every gap and the decision for each.

Statuses:
- ✅ **matched** — proto and reality agree.
- ⚠ **drift** — proto and reality differ; decision recorded below.
- 🚧 **missing** — endpoint shipped over HTTP but not in proto.

## graph_v1 (graph-index-rs)

| Concern | Status | Detail |
|---|---|---|
| GraphService.GetEntity / ListEntitiesByType / GetRelationships / GetClaims / GetContradictions / ExpandGraph | ✅ matched | gRPC server now serves all 6 against the same `GraphStore` the HTTP uses. |
| `GraphEntity.created_at` (proto: `Timestamp`) | ⚠ drift | model `Entity` has no `created_at` column. gRPC returns `None`. **Decision**: add `created_at` to `graph_entities` table + model in a follow-up migration; proto is the desired contract. |
| `GraphEntity.metadata` (proto: `Struct`) | ⚠ drift | model `Entity` has no metadata column. gRPC returns `None`. **Decision**: optional, low-priority addition. |
| `GraphRelationship.created_at` / `metadata` | ⚠ drift | Same as Entity — model lacks both. Same decision. |
| `GraphClaim.created_at` / `metadata` | ⚠ drift | Same as Entity. |
| `ExpandedGraph.claims` / `communities` | ⚠ drift | `GraphStore::get_graph_expansion` returns `(entities, relationships)` only; gRPC sends empty `claims` + `communities`. **Decision**: extend the store to surface claims + communities for an expansion seed set in a follow-up (the `community.rs` module already builds communities at extraction time; just needs query support). |
| **HTTP-only** `POST /v1/graph/exports` (GraphML/Markdown export) | 🚧 missing | Not in proto. **Decision**: add `rpc ExportGraph(ExportGraphRequest) returns (ExportGraphResponse)` returning rendered bytes + format. |
| HTTP `GET /v1/graphs/{org_id}` (snapshot view) | 🚧 missing | Not in proto. **Decision**: add `rpc GetOrgGraphSnapshot(...)` — pre-aggregated view for ops dashboards. |

## wiki_v1 (wiki-store-go)

| Concern | Status | Detail |
|---|---|---|
| WikiService.GetPage / GetPageByPath / ListPageVersions / GetBacklinks / CreatePage / UpdatePageVersion / SubmitProposal / ReviewProposal | ✅ matched | gRPC server now serves these end-to-end against `WikiRepo`. |
| `WikiSourceLog` message shape | ⚠ **major drift** | Proto carries `{original_chunks, processing_model, synthesis_prompt_hash}` — designed for AI-synthesis source tracking. Model `SourceLog` carries `{source_type, source_ref, sync_status, details}` — generic ingest sync tracking. They are *different concerns* sharing one name. **Decision**: rename proto to `WikiSynthesisSourceLog` for the synthesis path (not yet implemented), and add a separate `WikiIngestSourceLog` message matching the model. The current `GetPageSources` gRPC returns a minimal subset (log_id/page_id/created_at/metadata only) for now. |
| `WikiMaintenanceLog` proto vs model `MaintenanceLog` | ⚠ drift | Proto: `{issue_type, proposed_fix, status, resolved_at}` — issue-tracker shape. Model: `{action, actor}` — audit-log shape. **Decision**: same as sources — split into `WikiMaintenanceIssue` (proto's shape, for a future issue tracker) and `WikiMaintenanceAudit` (model's shape, for the action log). Today the gRPC maps `action → issue_type` as a stopgap. |
| **HTTP-only** `POST /pages/{pageID}/source-logs` + `GET .../source-logs` | 🚧 missing | Not in proto. **Decision**: add `rpc CreateIngestSourceLog` + `rpc ListIngestSourceLogs` once the source split above is in. |
| **HTTP-only** `POST /pages/{pageID}/maintenance-logs` + `GET .../maintenance-logs` | 🚧 missing | Same as above. |
| **HTTP-only** `POST /maintenance/sweep` (batch lint ingest: orphan / stale / weak-citation / contradiction) | 🚧 missing | Not in proto. **Decision**: add `rpc MaintenanceSweep(MaintenanceSweepRequest) returns (MaintenanceSweepResponse)` — this is the orchestrator-facing batch ingest. |
| `WikiPageVersion.safe_html` (HTTP applies bluemonday-scrubbed HTML at serialize time) | 🚧 missing | The HTTP handler sets `version.SafeHTML` after the repo read. Proto has no `safe_html` field. **Decision**: add `optional string safe_html = 13;` to `WikiPageVersion` — both wires should be able to return the pre-scrubbed render. |

## documents_v2 (documents-api-go)

| Concern | Status | Detail |
|---|---|---|
| Service shape | 🚧 paper only | Service is HTTP-only; proto exists. **Decision**: add gRPC server in a follow-up PR following the wiki pattern. |

## retrieval_v2 (retrieval-engine-rs)

| Concern | Status | Detail |
|---|---|---|
| All RPCs | ✅ matched | This is the only fully-paired wire today. |
| `RetrieveRequest.agent_id` | ✅ available | Now in the Model Plane (synced last session); not yet sent by callers. |
| `RetrieveStream` server-stream | ✅ available | Defined; not yet consumed by the Model Plane. |

## knowledge_v2

| Concern | Status | Detail |
|---|---|---|
| Server | 🚧 paper only | Need to audit which Data Plane v2 service implements it (no obvious service name maps). **Decision**: separate triage task. |

## Reconciliation order of operations

1. **Quick wins** (proto-only edits, no model migration):
   - ✅ **Done in this round**: `safe_html` (+ `safe_html_ok`) added to
     `WikiPageVersion` and populated by the wiki gRPC mapper from
     `model.WikiPageVersion.SafeHTML` / `SafeHTMLOK`. Backward-compatible
     (additive fields). Model Plane proto copy not yet synced — additive so
     safe to do in a sequenced follow-up.
   - Add `ExportGraphRequest`/`Response` + `ExportGraph` RPC to `graph_v1`.
   - Add `MaintenanceSweepRequest`/`Response` + RPC to `wiki_v1`.

2. **Source / Maintenance split** (proto rename + add):
   - Rename `WikiSourceLog` → `WikiSynthesisSourceLog`; add `WikiIngestSourceLog`.
   - Rename `WikiMaintenanceLog` → `WikiMaintenanceIssue`; add `WikiMaintenanceAudit`.
   - Add CRUD RPCs for the new messages.

3. **Model widening** (schema migrations):
   - `graph_entities.created_at` + `graph_entities.metadata` (then surface).
   - Same for `graph_relationships` and `graph_claims`.

4. **Extend `GraphStore::get_graph_expansion`** to also return claims + communities for the seed set; the `community.rs` module already builds communities at extraction time.

5. **documents-api-go gRPC**: standalone PR following the wiki/graph pattern.

6. **knowledge_v2 audit**: identify the backing service or mark the proto for deletion.

Each numbered group is its own PR so the cascade (proto regen → mp-contracts → Model Plane gateway → velion gen) stays reviewable.

## What landed in this round

- proto/graph_v1.proto and proto/wiki_v1.proto moved into per-package subdirs
  (`proto/graph/v1/graph.proto`, `proto/wiki/v1/wiki.proto`) so the shared
  `gen/go/` no longer has `v1`+`v2` package collisions.
- `gen-clients.sh` now generates all five Data Plane v2 protos (Go + Rust).
- **graph-index-rs**: tonic gRPC server on `:50053` alongside axum HTTP.
- **wiki-store-go**: grpc-go server on `:50054` alongside chi HTTP; 10/10 RPCs
  implemented against the existing `WikiRepo`. Two proto messages
  (`WikiSourceLog`, `WikiMaintenanceLog`) map a subset of fields pending the
  rename/split decisions above.
- This reconciliation catalog with concrete decisions per gap.
