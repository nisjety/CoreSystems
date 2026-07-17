# index-engine-rs Research Dive

Generated: 2026-06-07
Updated: 2026-07-10 (live re-verification pass — container health, HTTP surface, cargo fmt/clippy/test, source re-read against the 2026-07-02 plane baseline and its 2026-07-10 addendum)

Scope: `apps/Data Plane v2/services/index-engine-rs`

## 2026-07-15 final isolated acceptance delta

The final rebuilt service reached healthy state and participated in the
producer-scoped signed broker delivery/redelivery matrix. The restrictive-ZDR
run left stabilized downstream-store state unchanged. Production subject ACLs,
shared rollout, and database-backed deletion-outbox coverage remain pending.

## 2026-07-15 isolated runtime delta

The current-source image `b642e7485f5a` carries revision
`eeebd0bc98c66434936460020958891066eb05fd` and reached healthy state after the
isolated migrations. No signed broker event, chunk write, or transactional
deletion-outbox flow was invoked, so this is startup evidence only.

## Secure-MVP current state — 2026-07-10

- **Implemented/contained:** the unsigned document-event consumer is disabled by
  default behind two explicit insecure-development gates. The admin HTTP surface
  remains health/readiness only.
- **Tested:** 21/21 tests pass after consumer containment, including the two-gate
  regression; strict combined embedding/index all-target clippy passes.
- **Built/reachable in isolation:** the revised image built locally with
  verified revision/build labels and reached healthy state in the disposable
  stack; no event-flow test or shared deployment has run. With
  the consumer disabled, document chunk/index progression is intentionally
  ineffective until signed producer-scoped events and NATS authorization exist.
- **Coverage/audit:** Rust coverage and Rust dependency-audit tools were unavailable;
  no measured percentage or audit result exists.

The remainder is a superseded, sanitized pre-containment audit. Its old process
health and 20-test result do not prove current Docker effectiveness.

## Historical snapshot (superseded for current state)

`index-engine-rs` is the narrow event-driven indexing core for Data Plane v2. It converts canonical documents into chunked knowledge units and emits downstream progression for embedding and graph work. Container: `dpv2-index-engine`, admin port `:9201`.

Re-verified today (2026-07-10):

- Rust JetStream consumer with a two-route admin HTTP surface (`/health`, `/readyz` only — confirmed by probing, see below)
- no app-facing product API — confirmed, not just asserted
- owns chunking, normalization, extraction, and fingerprinting stages
- structurally clean: zero TODO/FIXME/mock/stub/fake/placeholder/dummy/unimplemented hits anywhere in the tree
- `cargo clippy -p index-engine-rs --all-targets -- -D warnings` is **clean** (0 warnings) — the known clippy doc-comment violation from the plane baseline lives in `retrieval-engine-rs/src/pipeline/orchestrator.rs:58`, not here
- `cargo fmt -p index-engine-rs -- --check` **does** show drift (confirms the plane-wide P2 finding), but it is purely cosmetic line-wrapping in two spots, not a functional issue (see Formatting section)
- `cargo test -p index-engine-rs`: 20/20 unit tests pass
- **New/deepened finding this pass**: this service has zero ZDR (Zero Data Retention) awareness anywhere in its own code or schema — it unconditionally persists whatever content it's handed into a durable table with no ZDR column at all. This sharpens the plane's known ZDR finding with the specific mechanism inside this service (see below).
- **New finding this pass**: container logs show a historical crash-loop (hard process exit, not a caught error) tied to transient Postgres unavailability at boot, with no retry/backoff — recovery depended entirely on Docker's restart policy. Currently stable (healthy 14h+ at time of check), but the fragility is real and reproducible by design (see Operational Resilience section).

Non-generated file count: 10 `.rs` files, ~1,294 lines total (`wc -l` across `src/`).

## Runtime Shape

Key runtime entrypoints (re-read in full this pass):

- `src/main.rs` (57 lines)
  - loads `Config::from_env()` (envy-based, no secrets logged)
  - opens a Postgres pool (`max_connections(10)`), connects to NATS, sets up JetStream stream + durable pull consumer
  - runs the admin HTTP server and the consumer loop concurrently via `tokio::select!`
- `src/api/mod.rs` (15 lines) — `axum::Router` with exactly two routes: `GET /health`, `GET /readyz`. No other route is registered anywhere in the crate.
- `src/stream/mod.rs` (184 lines) — JetStream stream/consumer setup (`DATAPLANE_DOCUMENTS`, work-queue retention, 7-day max age) and the `run_consumer` loop: pulls a batch, dispatches by subject (`dataplane.documents.created|updated|deleted`), acks, and on repeated failure (`>= max_delivery_attempts`, default 5) publishes to `dataplane.dlq.index-engine`.
- `src/builder/mod.rs` (299 lines) — `process_document()`: fetches content (inline or by re-querying Postgres when the lifecycle event carries no body), normalizes, chunks, dedupes against existing `knowledge_units` by content hash, does near-duplicate reuse (Jaccard ≥ 0.95) to skip re-embedding footer/whitespace churn, records `chunk_lineage` on reindex, computes orphaned knowledge IDs for downstream vector purge.
- `src/chunker/mod.rs` (325 lines) — recursive, structure-aware chunker: atomic table/code blocks kept whole, oversized paragraphs sentence-split, greedy packing with token-bounded overlap, char-window split as a last resort for any single oversized segment. Well-commented, has 8 passing unit tests.
- `src/fingerprint/mod.rs` (54 lines) — BLAKE3 content hash + deterministic UUID-shaped chunk ID from `document_id:chunk_index:content_hash`.
- `src/normalizer/mod.rs` (54 lines) — whitespace/blank-line collapsing.
- `src/extract/{mod.rs,markdown.rs}` (11 + 154 lines) — markdown heading/link extraction, used for lightweight structure hints.
- `src/config.rs` (42 lines) — env-driven config (`DATABASE_URL`, `NATS_URL`, `ADMIN_PORT`, `CHUNK_SIZE`, `CHUNK_OVERLAP`, `BATCH_SIZE`, `MAX_DELIVERY_ATTEMPTS`), all with sane defaults.

Surface:

- admin and health HTTP only — no product-facing routes exist to probe
- NATS JetStream consumer for document indexing events

## Live Verification (2026-07-10)

Container:
```
docker ps: dpv2-index-engine  Up 14 hours (healthy)  0.0.0.0:9201->9201/tcp
docker inspect .State.Health.Status: healthy
```

HTTP probes against `localhost:9201`:

| Path | Result |
|---|---|
| `GET /health` | `200 {"service":"index-engine-rs","status":"ok"}` |
| `GET /readyz` | `200 {"service":"index-engine-rs","status":"ready"}` |
| `GET /` | `404` |
| `GET /v1/knowledge-units` (with `X-Org-ID` header) | `404` |
| `GET /metrics` | `404` |

**Implication for the plane-wide "X-Org-ID header trust" finding**: unlike `graph-index-rs` (:9203), `data-quality-go`, and `data-orchestrator-go`, `index-engine-rs` has no org-scoped read endpoint at all to probe — there is nothing here an `X-Org-ID` header alone could exfiltrate. This service is **not** part of that vulnerability class. Confirmed by direct route enumeration, not by assumption.

**Implication for the CONTROL_PLANE_ENFORCEMENT strict-mode 503 finding**: `index-engine-rs` has no `pkg/authctx`-equivalent dependency, no JWT/JWKS crate, and no `CONTROL_PLANE_ENFORCEMENT` env var anywhere in its config or the compose file. That finding is specific to `documents-api-go` and `retrieval-engine-rs`; it does not apply here.

Postgres side-check (via `docker exec dpv2-postgres psql`):
- `knowledge_units` had the expected document/status indexes; the production index
  count is omitted.
- Current row count: 50 (small dev dataset).
- **Schema has no `zdr_classification`, `visibility`, or `owner_id` column** — see ZDR finding below.

## API And Relationship Map

Current relationships (re-confirmed by reading both sides):

- `documents-api-go` -> `index-engine-rs`
  - `dataplane.documents.created` / `dataplane.documents.updated` / `dataplane.documents.deleted` on NATS JetStream drive indexing. Publisher payload (`services/documents-api-go/internal/events/publisher.go`) carries only `document_id, org_id, source, type, title` — no content, no `zdr_classification`, no `visibility`.
- `index-engine-rs` -> Postgres
  - knowledge-unit persistence (`knowledge_units`) and lineage tracking (`chunk_lineage`), and re-fetches canonical `content` from `documents` by `document_id` when the lifecycle event carries no inline body (the normal case).
- `index-engine-rs` -> `embedding-engine-rs`
  - publishes `dataplane.knowledge.units.created` per new/changed knowledge unit for embedding.
- `index-engine-rs` -> `graph-index-rs`
  - downstream progression for graph extraction (via the same knowledge-unit creation signal; graph-index consumes independently).
- `index-engine-rs` -> DLQ
  - `dataplane.dlq.index-engine` after `max_delivery_attempts` (default 5) exhausted on a given message.

## ZDR (Zero Data Retention) Gap — Deepened Finding

The plane's known ZDR finding ("ZDR is NOT release-safe... bulk document ingest bypasses the single-create ZDR guard") is confirmed live in code this pass, and `index-engine-rs` adds a second, independent link in that chain:

1. **Confirmed bypass at the source** (`services/documents-api-go/internal/handler/documents.go`): the single-document `Create` handler has an explicit guard at line 189 — `if input.IngestPolicy.IsZeroRetention() && input.Content != "" { …403… }`. The `BulkIngest` handler (starting line 269) has **no equivalent check** anywhere in its per-document loop — it applies `applyVisibilityPolicy` and `validate.CreateDocument`, but never calls `IsZeroRetention()`. A ZDR-flagged document submitted via the bulk endpoint with non-empty content is accepted, stored, and triggers a `PublishDocumentCreated`/`PublishDocumentUpdated` event exactly like any other document.
2. **`index-engine-rs` has no way to know or care**: `DocumentEvent` (`src/builder/mod.rs:7-15`) has fields `document_id, org_id, content, title, source, doc_type` — no `zdr_classification`, no ephemeral/visibility flag. This is not a bug introduced by dropping a field on deserialize; the wire event itself (`DocumentCreatedEvent`/`DocumentUpdatedEvent` in `documents-api-go`'s publisher) never carries ZDR classification in the first place, so there is nothing for `index-engine-rs` to consult even if it wanted to.
3. **The persistence target has no ZDR column either**: `knowledge_units` (verified via live `\d knowledge_units`) has 16 columns — none of them `zdr_classification`, `visibility`, or `owner_id`. Once content reaches this table it is architecturally indistinguishable from any non-ZDR content; there is no column a future fix could even filter on without a migration.

Net effect: fixing the `BulkIngest` guard in `documents-api-go` alone is necessary but not sufficient. Even with that gate closed, `index-engine-rs` remains structurally ZDR-blind — any future producer of `dataplane.documents.created/updated` (a new ingestion path, a replay tool, a migration script) that emits an event for content that should have been ephemeral will be chunked and durably persisted with zero possibility of the consumer noticing, because the concept doesn't exist anywhere in this service's types or schema. This directly violates CoreSystem's architecture rule that "Zero Data Retention must propagate through any content-persisting boundary" — `index-engine-rs` is itself a content-persisting boundary that the propagation never reaches.

Recommended fix shape (not yet implemented): thread `zdr_classification` through `DocumentCreatedEvent`/`DocumentUpdatedEvent`, add it to `DocumentEvent` in `builder/mod.rs`, and either (a) reject/skip indexing when the classification demands ephemeral handling, or (b) add the column to `knowledge_units` so downstream retrieval can enforce it — plus close the `BulkIngest` gap in `documents-api-go` as the primary fix.

## Operational Resilience — Startup Crash-Loop History

`docker logs dpv2-index-engine` (full history, not just recent) shows:

```
2026-07-04T07:19:38Z  WARN  slow statement: DELETE FROM knowledge_units ... elapsed=2.08s (threshold 1s)
2026-07-04T07:19:43Z  WARN  slow statement: DELETE FROM knowledge_units ... elapsed=4.01s (threshold 1s)
[gap]
Error: error communicating with database: failed to lookup address information: Name or service not known
2026-07-07T14:15:30Z  WARN  acquired connection, but time to acquire exceeded slow threshold (22.3s)
Error: pool timed out while waiting for an open connection
Error: pool timed out while waiting for an open connection
Error: pool timed out while waiting for an open connection
Error: pool timed out while waiting for an open connection
2026-07-09T21:02:55Z  WARN  acquired connection, but time to acquire exceeded slow threshold (22.7s)
```

Reading `src/main.rs`: the initial `sqlx::postgres::PgPoolOptions::new().max_connections(10).connect(&cfg.database_url).await?` has no retry/backoff — a `?` on a fatal connect error propagates straight out of `main()`, and because these are plain `Error: ...` lines (not JSON — `tracing` was already initialized), they come from the `#[tokio::main]` macro's default `Termination` handling of a top-level `Err`, not from a caught-and-logged path. The process hard-exits; recovery depends entirely on the compose file's `restart: unless-stopped` policy re-launching the container until Postgres is reachable. The container is currently healthy and has been up 14h+ (last restart `2026-07-09T21:01:35Z`), so this is not an active incident, but the fragility is real: any transient Postgres DNS hiccup or pool exhaustion at boot takes the whole service down rather than retrying in place.

Same fragility applies mid-run: inside `tokio::select! { admin_server, run_consumer }`, if `run_consumer` ever returns `Err` (any unhandled sqlx error propagated via `?` inside the loop, e.g. from `handle_document_deleted`), the `select!` arm logs it via `tracing::error!` but the block then falls through to `Ok(())` and the whole binary exits normally — again relying on Docker to restart rather than the consumer loop self-healing.

Historical slow-DELETE entries were checked against the schema and both expected
indexes existed, so this did not appear to be a missing-index problem. Production
row counts are omitted; transient load/contention remained the likely cause.

## Duplicates, Redundancies, And Inactive Surfaces

No duplicate surface found this pass either. The service remains intentionally headless — the only two routes are health/readiness, everything product-facing is consumed downstream through retrieval.

## Stubs, Placeholders, And Missing Connections

Re-ran `grep -rniE "TODO|FIXME|mock|stub|fake|placeholder|unimplemented|not.?implemented|dummy"` across the full crate: **zero hits**. This pass did not find explicit runtime stubs, placeholders, or backup residue inside this service tree — matches the 2026-06-07 finding.

The main dependency risks are external, and one is now concrete rather than speculative:

- if upstream document events drift, this service has little independent contract surface to catch that at the API layer (unchanged from prior pass)
- **the NATS event contract carries no ZDR signal**, so this service cannot independently enforce the plane's ZDR rule even if it wanted to (new, see above)

## Formatting And Lint Status (re-verified live, 2026-07-10)

- `cargo fmt -p index-engine-rs -- --check`: **drift confirmed**, two spots, both pure line-wrap reflow with no logic change:
  - `src/builder/mod.rs:110-114` (`near_duplicate`'s two `HashSet` collection lines)
  - `src/chunker/mod.rs:84-87` (the `step` calculation)
- `cargo clippy -p index-engine-rs --all-targets -- -D warnings`: **clean**, 0 warnings. The plane-baseline clippy doc-comment violation is in `retrieval-engine-rs/src/pipeline/orchestrator.rs:58`, a different crate — index-engine-rs is not implicated in that specific finding.
- `cargo test -p index-engine-rs`: **20/20 pass** (builder: 4, chunker: 7, fingerprint: 4, normalizer: 2, extract::markdown: 3 — live run: 20 passed, 0 failed, 0 ignored).
- Git status for `services/index-engine-rs/` is clean — no uncommitted changes, so what's running in the container (built 2026-07-02, last container restart 2026-07-09) matches the current source tree exactly.

## API Design And Performance Notes

API design:

- the service boundary is correct — avoiding a broad human-facing API here keeps indexing logic internal and rebuildable
- confirmed live: nothing beyond `/health` and `/readyz` is reachable

Performance and operational notes:

- the service is fundamentally throughput-bound on event consumption and Postgres writes (unchanged assessment)
- the near-duplicate reuse logic (`near_duplicate`, Jaccard ≥ 0.95) and content-hash dedup are legitimate cost-saving mechanisms, not placeholders — confirmed by reading the full `process_document` flow
- see Operational Resilience above for the concrete crash-loop/no-retry finding that the previous pass did not surface (it only checked static code, not runtime logs)

## Current Doc Cleanup Read

Keep:

- `DATA_PLANE_DEEP_DIVE.md`
- `docs/WIRE_RECONCILIATION.md`

No delete-ready service-local docs were found in this pass.

## Historical bottom line (superseded)

`index-engine-rs` is still the clean, narrow part of Data Plane v2 the 2026-06-07 pass described: no stubs, no dead surface, no product-facing attack surface, clippy-clean, tests green. Two things changed with this live re-verification:

1. It is **not** part of the plane's X-Org-ID-trust or CONTROL_PLANE_ENFORCEMENT-503 findings — confirmed by direct probing, not assumption — so don't lump it in with graph-index/data-quality/data-orchestrator/documents-api/retrieval-engine when scoping fixes for those.
2. It **is** a real, previously-unstated link in the plane's ZDR gap: the wire contract and the `knowledge_units` schema have no ZDR concept at all, so this service will durably persist content regardless of the originating document's retention policy, and a fix at `documents-api-go`'s `BulkIngest` gate alone won't make this service ZDR-aware for any other future producer. Combined with the newly-observed startup crash-loop history (stable now, but no retry/backoff by design), the follow-up work here is no longer purely "verification and rebuild confidence" — there's a concrete compliance-relevant fix (ZDR field threading) and a concrete resilience fix (retry/backoff on Postgres connect) to schedule.

## 2026-07-11 secure-MVP delta (current)

The historical ZDR conclusion is superseded. Index accepts only signed
Documents events, rejects event ZDR, refetches the canonical document, and does
zero chunk/outbox work for missing, deleted, restricted, or unknown-classification
rows. Deletion progression uses a transactional signed outbox and stable broker
message ID. Focused tests pass (26 passed, 2 explicit disposable-PostgreSQL
ignores) with strict clippy; runtime broker/database proof remains pending after
the isolated startup pass.


## 2026-07-17 optimization-program reconciliation

The two per-chunk ingest queries (COUNT skip-if-exists + near-duplicate embedding-reuse) were PROVABLY DEAD (they ran after the per-document DELETE in the same tx → always empty) and are removed; chunk identity is now computed in-memory (`chunk_identity`), commit `e5846a52` — no behavior change. FOLLOW-UP (spawned task): the near-dup embedding-reuse is inert in prod (re-embeds every chunk on re-crawl); making it live needs its own design.
