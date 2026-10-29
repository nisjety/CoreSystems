# Plan — Fix Quarry-v2 → Data Plane v2 Write Path

**Status:** ✅ IMPLEMENTED (Phases 1–5 + static verification). Phase 6 live E2E needs the running stack; Phase 7 deferred (separate PR).
**Date:** 2026-05-30
**Scope:** Ingestion Plane/Quarry-v2 · Data Plane v2 · Model Plane (v1)
**Source:** audit of 25 cited claims (all substantively verified).

---

## Implementation status — 2026-05-30

| Phase | State | Evidence |
|-------|-------|----------|
| P1 Lock contract | ✅ Done | DP-v2 `CreateDocumentInput` mapped; `X-Internal-Api-Key` + `X-Org-ID` required; `content` mandatory; chunks derived downstream |
| P2 Repoint client | ✅ Done | `ingest_client.rs`: `/v1/ingest`→`/v1/documents`, `CreateDocumentBody` mapping, ZDR→skip durable write, headers added. **11/11 unit tests green** |
| P3 Idempotency/dedup | ✅ Done | Partial unique indexes already exist (`uq_documents_idempotency`, `idx_documents_crawl_url_dedup`). Client sends **URL-stable** `idempotency_key` = `quarry-url:<blake3(url)>` + `metadata.url`; repo `updateContent` re-indexes on content change |
| P4 Async readiness | ✅ Done (producer) | `embedding-engine` already publishes **`dataplane.documents.indexed`** + sets `documents.status='indexed'` when all units embedded. Agents use inline scrape now; durable retrieval after. Model Plane consumer = optional follow-up |
| P5 NATS cleanup | ✅ Done | Removed dead `quarry.documents.crawled` subscriber (`subscriber.go` deleted, `main.go` wiring removed). `go build` + `vet` + `test ./...` green |
| P6 Verify | ◑ Static done | Rust 11/11, Go build/vet/test green both sides. **Live E2E (scrape→store→index→embed→retrieve) requires Postgres+NATS+Qdrant** — run via Ingestion Plane `END_TO_END_TESTS.md` |
| P7 Browser agent loop | ⏸ Deferred | `browser_agent.rs:310` — separate PR, not blocking |

**Files changed:**
- `Quarry-v2/crates/quarry-runtime/src/ingest_client.rs` — repoint + payload mapping + ZDR skip + URL-stable idempotency key + tests
- `Data Plane v2/services/documents-api-go/cmd/main.go` — removed dead subscriber wiring
- `Data Plane v2/services/documents-api-go/internal/events/subscriber.go` — **deleted** (dead path)

**Already correct, no change:** dedup indexes (P3), `dataplane.documents.indexed` readiness event (P4), Model Plane retrieval relay (`dataplane.rs:492`) + session-core local fallback.

---

## Goal

Quarry scrape (`ingest:true`) → Data Plane v2 stores doc → indexes knowledge units → embeds to Qdrant → Model Plane agent retrieves durable web evidence. One canonical, production-correct write path. Today: **all three write paths misaligned, zero data lands.**

---

## Root causes (verified)

| ID | Layer | Problem | Evidence |
|----|-------|---------|----------|
| R1 | HTTP | Quarry POSTs `/v1/ingest`; DP-v2 has **no such route** → 404. Canonical = `POST /v1/documents` (single) + `POST /v1/documents/bulk` (batch) | `ingest_client.rs:62`; DP-v2 `main.go:125,130,131` |
| R2 | NATS | Quarry publishes `quarry.run.<id>.<evt>` + `quarry.events.<evt>` (incl. `store_record_written`). DP-v2 subscribes only `quarry.documents.crawled` → never fires | `nats_event_bus.rs:226,229`; DP-v2 `subscriber.go:15` |
| R3 | gRPC | `CreateDocument` refused unless `DPV2_ALLOW_GRPC_DOCUMENT_WRITES=1` | `document_svc.rs:169` |
| R4 | Payload | Contract field is `source_url`, not `url`. Must map to DP-v2 Create schema | `contracts.rs:119` |
| R5 | Async | Index/embed/Quickwit run async after doc write → agent may retrieve before data ready | `index-engine-rs/builder/mod.rs:~135`; `embedding-engine-rs/stream/mod.rs:17` |
| R6 | Agent | Browser agent loop (click/type/observe) not wired to Quarry. Separate track | `browser_agent.rs:310` |

---

## Decision (per your narrative — restated, confirm)

- **Canonical write = HTTP `POST /v1/documents`** (+ `/bulk` for chunk batches). Simplest, synchronous, already canonical.
- **gRPC stays deprecated.** Do not depend on `DPV2_ALLOW_GRPC_DOCUMENT_WRITES`.
- **NATS = observability only.** Not the ingestion path.
- **Target = Data Plane v2.** All document/index/embed/retrieval code lives there (v1 dir empty of it).

---

## Phases

### Phase 1 — Lock the contract (read-only, no code)
- Read DP-v2 Documents Create handler + `document_repo.go` Create + bulk handler.
- Capture exact request JSON: field names, required vs optional, dedup key, bulk shape.
- Produce field-map table: Quarry `DataPlaneIngestRequest` → DP-v2 `/v1/documents` body.
- **Exit:** signed-off mapping table (esp. `source_url`→DP field). No silent drops.

### Phase 2 — Repoint Quarry ingest client
- `ingest_client.rs:62`: `/v1/ingest` → `/v1/documents`. Route to `/v1/documents/bulk` when `chunks.len() > N`.
- Apply Phase 1 mapping (org_id, source_url, title, markdown, chunks, fingerprint, source_trace, run_id).
- Update wiremock tests expecting `/v1/ingest` (`ingest_client.rs:254–423`).
- Keep auth header + timeout config.
- **Exit:** unit + wiremock green against `/v1/documents`.

### Phase 3 — Idempotency / dedup
- Confirm DP-v2 `documents` table unique on fingerprint (or url+org_id). Add migration if missing.
- `knowledge_units` already `ON CONFLICT DO NOTHING` (`builder/mod.rs:138`) — mirror for documents.
- **Exit:** re-scrape same URL = no duplicate doc/units.

### Phase 4 — Async retrieval-readiness contract
- Two-track readiness:
  - **Now:** Quarry returns scrape result to Model Plane as short-lived context (agent unblocked immediately).
  - **Durable:** after embed, DP-v2 emits `dataplane.documents.ready` (keyed by document_id/fingerprint). Consumer already on `dataplane.knowledge.units.created` (`stream/mod.rs:17`).
- Model Plane: use scrape result inline; retrieve durable on next turn.
- **Decide:** event signal vs poll. Recommend event.
- **Exit:** documented readiness contract; agent never retrieves empty.

### Phase 5 — NATS cleanup
- DP-v2 `quarry.documents.crawled` subscriber (`subscriber.go:15`): **remove** (dead, wrong subject) OR repurpose to subscribe `quarry.events.store_record_written` + pull artifact by ref.
- Recommend remove — HTTP is canonical write. Keep Quarry `quarry.run.*`/`quarry.events.*` for metrics only.
- **Exit:** no dead subscriber; subject map documented.

### Phase 6 — Verify end-to-end
- Integration: scrape `ingest:true` → assert DP-v2 doc row + knowledge_units + Qdrant vectors + Model Plane `/v1/retrieval` returns chunk. Use Ingestion Plane `END_TO_END_TESTS.md` harness.
- Negative: `ingest:false` → no DP write. Re-scrape → no dup.
- **Exit:** green E2E; gRPC flag unused.

### Phase 7 — Browser agent loop (SEPARATE, deferred)
- Wire `browser_agent.rs:310` real Quarry dispatch. Own plan, own PR. Not blocking.

---

## Risks

| Sev | Risk | Mitigation |
|-----|------|------------|
| HIGH | Field mismatch (`source_url` vs `url`) → silent data loss | Phase 1 mapping table + Phase 6 assert |
| HIGH | Async race → agent retrieves empty | Phase 4 readiness signal + inline scrape context |
| MED | No dedup → duplicate docs on re-scrape | Phase 3 unique constraint |
| MED | DP-v2 is target while roadmap deprecates other v2 planes | Confirm DP-v2 is keeper for docs (assumption below) |
| LOW | bulk vs single endpoint for chunked payloads | Threshold `N` in Phase 2 |

---

## Open assumption (confirm)

Building on **Data Plane v2** because canonical document/ingest/retrieval code lives there. Memory note says "v2 deprecated once v1 reaches parity" for other planes. Confirm DP-v2 is the keeper for the document pipeline, not throwaway. If v1 is target → prepend phase: port Documents API to DP-v1.

---

## Sequencing & complexity

- Order: P1 → P2 → P3 → P4 → P5 → P6. P7 independent.
- Complexity: **MEDIUM.** Client repoint small; payload map + async readiness + E2E tests are the bulk.
- Parallelizable: P1 (DP read) ∥ P3 prep ∥ P5 audit.
