# data-orchestrator-go Research Dive

Generated: 2026-06-07
Updated: 2026-07-11 (secure-MVP durability evidence; preserves the superseded 2026-07-10 live audit below)

Scope: `apps/Data Plane v2/services/data-orchestrator-go`

## 2026-07-15 final isolated acceptance delta

The rebuilt disposable service passed its strict HTTP four-shape matrix
(401/401/200/403). The shared deployment is unchanged. Production mutation
routes still fail before persistence because no signed resumable worker/callback
contract exists; this is secure containment, not functional production readiness.
Database-backed job coverage remains below the 80% gate.

## 2026-07-15 isolated runtime delta

The current-source image `f00398f1e980` carries revision
`eeebd0bc98c66434936460020958891066eb05fd`. Its disposable HTTP four-shape
matrix returned 401/401/200/403, proving that header-only tenant impersonation
is closed on this isolated build. Production mutations remain intentionally 503
until the signed resumable worker exists; the shared deployment is unchanged.

## Secure-MVP current state — 2026-07-11

- **Implemented:** sensitive HTTP routes now require cryptographically verified
  RS256/JWKS claims, pin tenant identity from those claims, and reject conflicting
  organization headers. Mutation routes require `data:orchestrate` or
  `data:admin`; downstream callbacks forward a bearer instead of trusted identity
  headers. The additive `20260711160000_quality_orchestrator_durability.sql`
  migration and `PostgresJobStore` persist tenant-scoped job identity, document
  IDs, status, progress, result/error, and timestamps. Organization-local
  idempotency prevents duplicate submission across retries, transitions are
  lifecycle constrained, and `GET /v1/orchestrator/jobs/{jobID}` performs a
  tenant-pinned completion lookup. This is durable lifecycle state; automatic
  process-restart recovery/resumption is not yet proven.
- **Tested:** the disposable-PostgreSQL
  integration passed lifecycle, idempotent replay, and tenant-isolation checks,
  including an idempotent migration application and scoped fixture rollback.
  Current `go test -race ./...`, `go vet ./...`, `go build ./...`, and
  `govulncheck ./...` pass. The changed `internal/authctx` package measures 91.1%
  statement coverage. The source-only `internal/jobs` profile is 31.9%; the
  current expanded PostgreSQL profile has not been rerun, so the >=80% package
  gate remains unproven.
- **Built/reachable/effective in isolation:** the current durability image built
  with revision/build labels and passed the disposable endpoint matrix. It has
  not been deployed to the shared environment.
- **Containment/remaining gates:** the unsigned `dataplane.cost.ledger` consumer
  remains disabled by default behind two explicit insecure-development gates.
  Cost-ledger ingestion is ineffective until signed, scoped events/NATS
  permissions exist. Production mutations return 503 before persistence until a
  signed resumable worker/callback contract exists. Prove expanded database
  coverage, restart recovery, and controlled deployment before claiming
  resumable production operation.

The remainder is a superseded, sanitized pre-fix audit. Preserve its root-cause
analysis, but do not treat its container or reachability statements as current.

## Historical pre-fix live audit (superseded)

This pass re-verified the prior 2026-06-07 static-read doc against the live container (`dpv2-data-orchestrator`, healthy, port 8012) and the current source tree (git-clean, no uncommitted diff under this service directory — the running image, built 2026-07-02T10:34:01Z, matches the checked-out source exactly). Headline results:

- **CRITICAL/P0 CONFIRMED LIVE, UNCHANGED**: `GET /v1/orchestrator/stale-embeddings` accepts any caller-supplied `X-Org-ID` header with zero credential check and returns that org's real stale/stuck/failed embedding report. Reproduced with curl against the live container below. Same root cause and same class as documented for Control Plane's `X-Org-ID`/`X-User-Role` trust gaps and Data Plane v2's graph-index/data-quality findings — this is the fourth confirmed instance of the identical pattern.
- **NATS document-lifecycle ownership hypothesis REFUTED**: this service does NOT publish `dataplane.documents.updated` or `dataplane.source_objects.changed`. It publishes only `dataplane.documents.created` (re-triggered for reindex jobs) and `dataplane.documents.indexed` (re-triggered for graph-build jobs). The `documents.updated` / `source_objects.changed` lifecycle events are owned exclusively by `documents-api-go`'s outbox publisher (`internal/events/publisher.go` + `internal/events/outbox.go`), confirmed by grep and by NATS `connz` showing `dpv2-documents-api` as a distinct, separately-connected client. The "RDI gap: no documents.updated/source_objects.changed publisher" item from prior cross-plane memory is not this service's responsibility and was already noted elsewhere as closed — data-orchestrator-go was never a candidate.
- **NEW finding (not in the 2026-06-07 doc or the 2026-07-02 plane audit)**: submitted jobs (`POST /v1/orchestrator/jobs`, `/reindex`) are entirely ephemeral. There is no jobs table, no persistence of job state beyond a single `job_created` audit-log row, and no `GET /v1/orchestrator/jobs/{id}` route (confirmed 404) or `GET /v1/orchestrator/jobs` route (confirmed 405 — only POST is registered). A caller gets a 202 with an in-memory snapshot of the job and then has no way, ever, to learn whether it completed, failed, or is still running.
- **NEW finding**: zero test files exist anywhere in this service (`find . -name "*_test.go"` → empty). `go vet` is clean and `go build` succeeds, but there is no test coverage at all for a service that is currently shipping a live, unauthenticated cross-org data leak.
- gofmt drift confirmed and narrowed: only 2 files, both purely cosmetic struct-tag/field alignment (`internal/jobs/stale_detector.go`, `internal/model/job.go`). No logic drift.
- ZDR: not applicable to this service. It never touches document content — only document IDs, org IDs, and job/embedding metadata — so it is correctly out of scope for the ZDR propagation findings that apply to documents-api-go / retrieval-engine-rs / Model Gateway.
- Quarry-v2 `DataPlaneIngestRequest` contract drift: not applicable to this service. Grep confirms `DataPlaneIngestRequest` exists only in Ingestion Plane's Quarry-v2 crates (`quarry-runtime`, `quarry-core`) and its cross-plane contract package (`pkg/quarrycontracts`) — it has no presence anywhere in Data Plane v2. This baseline item belongs to documents-api-go's ingest contract, not data-orchestrator-go; re-verify it there, not here.

## Historical snapshot (superseded)

`data-orchestrator-go` is the maintenance and rebuild controller inside Data Plane v2. It owns internal job submission (reindex / graph-build / wiki-refresh), stale-embedding detection, and cost-ledger consumption. It is a Go HTTP operator service (chi router), Postgres- and NATS-backed, not a product-facing user surface.

Container identity (live, 2026-07-10): `dpv2-data-orchestrator`, port `8012`, status `Up 14 hours (healthy)`. Image `data-plane-v2-data-orchestrator`, created `2026-07-02T10:34:01Z`; container created `2026-07-02T10:36:14Z`. `git status --porcelain -- services/data-orchestrator-go` is empty — the running binary is built from exactly the source read in this pass, so all findings below are live-current, not stale.

Non-generated, non-vendored file count: 9 Go files (`cmd/main.go`, `internal/config/config.go`, `internal/cost/consumer.go`, `internal/handler/orchestrator.go`, `internal/jobs/executor.go`, `internal/jobs/stale_detector.go`, `internal/metrics/metrics.go`, `internal/model/job.go`, `internal/otel/tracing.go`).

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go` — wires Postgres (`pgxpool`), NATS (`nats.go`), `jobs.Executor`, `jobs.StaleDetector`, `cost.Consumer`, chi HTTP server. Cost-ledger consumer startup failure is logged as a warning and tolerated (service continues without it) — unchanged from the 2026-06-07 read.
- `internal/jobs/executor.go` — `CreateJob` (writes one `data_plane_audit_log` row, returns an in-memory `*model.Job`, never persists it), `Run` dispatch to `ExecuteReindex` / `ExecuteGraphBuild` / `ExecuteWikiRefresh`.
- `internal/jobs/stale_detector.go` — three Postgres queries per call (stale, stuck-pending, failed), all scoped by `org_id = $1` taken directly from the unauthenticated header.
- `internal/handler/orchestrator.go` — HTTP handlers plus `OrgIDMiddleware`, the entire "auth" layer for this service.
- `internal/cost/consumer.go` — subscribes to `dataplane.cost.ledger`, persists to `cost_events`.

Primary surface (confirmed against `cmd/main.go` routing table, all three under `handler.OrgIDMiddleware`):

- `POST /v1/orchestrator/jobs`
- `POST /v1/orchestrator/reindex`
- `GET  /v1/orchestrator/stale-embeddings`
- `GET /health`, `GET /readyz`, `GET /metrics` (unauthenticated, as expected for ops endpoints)

## CRITICAL/P0 — X-Org-ID accepted with zero credential (confirmed live, 2026-07-10)

`internal/handler/orchestrator.go`:

```go
func OrgIDMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		orgID := r.Header.Get("X-Org-ID")
		if orgID == "" {
			writeError(w, http.StatusBadRequest, "X-Org-ID header required")
			return
		}
		ctx := context.WithValue(r.Context(), orgIDKey, orgID)
		next.ServeHTTP(w, r.WithContext(ctx))
	})
}
```

This is the entire authorization layer for all three `/v1/orchestrator/*` routes. There is no JWT/JWKS verification, no bearer token, no internal service key, no call out to Control Plane — grep for `jwt|bearer|authorization|authctx` across the whole service returns nothing. The header value is trusted as-is and passed straight into `StaleDetector.Detect(ctx, orgID, 0)`, which parameterizes `WHERE ku.org_id = $1` directly.

**Sanitized live reproduction (2026-07-10):** no-auth requests carrying both a
fabricated tenant selector and a selector for a populated tenant returned 200.
Response bodies, tenant IDs, row counts, document/knowledge IDs, and timestamps are
removed. This proved the query executed solely under caller-selected tenant scope.

The missing-header historical control returned 400 (body redacted), confirming the
only gate was header presence rather than tenant entitlement.

`POST /v1/orchestrator/jobs` and `POST /v1/orchestrator/reindex` sit behind the identical `OrgIDMiddleware` (verified by code read — `r.Route("/v1/orchestrator", func(r chi.Router) { r.Use(handler.OrgIDMiddleware) ...`), so the same no-credential trust applies to job creation and reindex triggering, not just the read-only stale-embeddings report. These two were **not** fired live in this pass (unlike the idempotent GET, they write an audit-log row and — for non-empty `document_ids` — publish live NATS events per document, which would pollute the shared live environment); the finding for them rests on code inspection, not live reproduction, and is flagged as such.

## NATS: what this service actually publishes (live-verified, 2026-07-10)

`internal/jobs/executor.go` defines and publishes exactly two subjects:

```go
const (
	SubjectDocCreated  = "dataplane.documents.created"
	SubjectDocsIndexed = "dataplane.documents.indexed"
)
```

- `ExecuteReindex` re-publishes `dataplane.documents.created` once per document ID in the job (re-triggers the ingest-consumer chain for a reindex).
- `ExecuteGraphBuild` re-publishes `dataplane.documents.indexed` once per document ID (re-triggers graph-index's consumer).
- Neither of these is `dataplane.documents.updated` or `dataplane.source_objects.changed`.

Fleet-wide grep confirms those two lifecycle subjects are defined and published only by `documents-api-go`:

```
services/documents-api-go/internal/events/publisher.go:12:  SubjectDocUpdated = "dataplane.documents.updated"
services/documents-api-go/internal/events/publisher.go:14:  SubjectSourceObjectChanged = "dataplane.source_objects.changed"
services/documents-api-go/internal/handler/documents.go:336: h.repo.EnqueueOutbox(r.Context(), result.Document.OrgID, events.SubjectDocUpdated, evtPayload)
```

`documents-api-go` publishes `documents.updated` through an outbox pattern (`EnqueueOutbox` + a separate drain worker in `internal/events/outbox.go`), not a direct `nc.Publish` call — that drain worker is the actual thing to re-verify for "is it publishing now," and it lives in documents-api-go, not here.

Live NATS connectivity check (`docker exec dpv2-nats … /connz`, IPs mapped to containers via `docker inspect`):

| IP | Container | Subscriptions |
|---|---|---|
| 172.19.0.12 | dpv2-data-orchestrator | 1 |
| 172.19.0.14 | dpv2-documents-api | 0 |
| 172.19.0.6 | dpv2-graph-index | 3 |
| 172.19.0.15 | dpv2-retrieval-engine | 4 |
| 172.19.0.2 | dpv2-index-engine | 2 |
| 172.19.0.5 | dpv2-embedding-engine | 4 |
| 172.19.0.7 | dpv2-wiki-store | 0 |
| 172.19.0.16 | dpv2-quickwit-adapter | 7 |

`dpv2-data-orchestrator` is live-connected to NATS with exactly 1 active subscription, consistent with the code: the only `Subscribe` call in this service is the cost-ledger consumer (`dataplane.cost.ledger`) in `internal/cost/consumer.go`. `stale_detector.go` and `executor.go` only publish, never subscribe, which is why the subscription count is 1 and not higher.

**Conclusion for the task's routing hypothesis**: data-orchestrator-go is not, and never was, the owner of `documents.updated` / `source_objects.changed` publishing. That ownership sits with documents-api-go's outbox path. Re-verify the outbox drain worker there, not in this service, if the RDI-gap-closed memory needs re-confirming.

## Ephemeral job state — new finding, 2026-07-10

`CreateJob` (`internal/jobs/executor.go`) does exactly one durable thing: insert a `job_created` row into `data_plane_audit_log`. It does not insert into any `jobs` table — grep for `INSERT INTO` / `CREATE TABLE` across the service returns only the audit-log insert (in `executor.go`) and the `cost_events` insert (in `cost/consumer.go`). The `*model.Job` returned to the HTTP caller and then mutated by the background goroutine (`job.Status = model.StatusCompleted` / `StatusFailed`) is a plain in-memory struct that is discarded the moment the goroutine returns — nothing else in the process holds a reference to it after the initial 202 response.

There is no route to check on a job afterward:

```
$ curl -H "X-Org-ID: [redacted-test-org]" http://localhost:8012/v1/orchestrator/jobs/some-id
HTTP_STATUS: 404
$ curl -H "X-Org-ID: [redacted-test-org]" http://localhost:8012/v1/orchestrator/jobs
HTTP_STATUS: 405
```

Confirmed against `cmd/main.go`'s route table: only `POST /jobs`, `POST /reindex`, `GET /stale-embeddings` are registered under `/v1/orchestrator`. A caller who submits a reindex or graph-build job gets an immediate 202 with a `pending`/`progress:0` snapshot and then has no way — not via this service, not via any persisted table — to ever learn whether the job succeeded, partially completed, or failed. This is a real operability gap on top of the existing "cost consumer degraded-mode is silent" note from the prior pass, not just a nice-to-have: for a "rebuild/maintenance controller," rebuild outcomes are unobservable.

## Zero test coverage — new finding, 2026-07-10

```
$ find . -name "*_test.go"
(empty)
```

No unit tests, no integration tests, anywhere in this service. `go build ./...` succeeds and `go vet ./...` is clean, but there is zero automated coverage for a service that is currently shipping a live cross-org authorization bypass (see CRITICAL/P0 above) — a regression test asserting `OrgIDMiddleware` rejects an unverified header would have caught this class of bug before it reached a running container.

## Not applicable to this service (scope corrections from the baseline)

- **ZDR (Zero Data Retention)**: not applicable. `data-orchestrator-go` never touches document content — grep for `content\b` (excluding `Content-Type`) is empty, and grep for `zdr|zero.?data.?retention|ephemeral` is empty. It only ever handles document IDs, org IDs, and embedding-status metadata. The real ZDR propagation gaps (bulk-ingest bypass, retrieval ephemeral-mode gap, semantic-cache persistence, Model Gateway flag drop) belong to documents-api-go, retrieval-engine-rs, and Model Gateway respectively — re-verify there, not here.
- **Quarry-v2 `DataPlaneIngestRequest` cross-plane drift**: not applicable. Fleet-wide grep for `DataPlaneIngestRequest` returns matches only inside `apps/Ingestion Plane/Quarry-v2/{crates/quarry-runtime,crates/quarry-core,pkg/quarrycontracts}` — zero references anywhere under Data Plane v2. This baseline item is a Quarry-v2 ↔ documents-api-go ingest-contract concern; it does not touch data-orchestrator-go's surface at all.
- **Rust clippy/fmt drift** (embedding/index/retrieval-engine-rs, `orchestrator.rs:58`): not applicable — this service is 100% Go, no Rust code.

## Formatting / build hygiene (re-verified 2026-07-10)

```
$ gofmt -l .
internal/jobs/stale_detector.go
internal/model/job.go

$ go vet ./...
(clean)

$ go build ./...
BUILD OK
```

Both gofmt hits are purely cosmetic struct-tag/field-alignment drift (e.g. `ContentUpdatedAt time.Time` vs `time.Time` column width in `StaleEmbedDetail`, and `JobStatus` const block alignment in `model/job.go`) — no logic differs. This confirms and narrows the baseline's generic "Go fmt drift in the 4 Go services" claim for this specific service: 2 files, cosmetic only, `gofmt -w` is a safe, zero-risk fix.

## Stubs, Placeholders, And Missing Connections

`grep -rniE "TODO|FIXME|mock|stub|fake|placeholder|not.?implement|unimplemented"` across the entire service returns **zero matches**. No explicit code stubs, no backup files, no marked-incomplete code paths. The service's problems are not "unfinished code" — they are a genuine, shipped, unauthenticated authorization gap plus an unobserved job-lifecycle design, both confirmed live above.

Partial relationship (unchanged from 2026-06-07): cost-ledger consumer startup failure is logged as a warning and tolerated — the service continues to serve HTTP traffic without cost accounting if NATS subscription setup fails. This means the cost-ledger path can silently disappear in a degraded environment with no alerting.

## API Design And Performance Notes

- Keeping rebuild/maintenance routes separate from product-facing retrieval/indexing APIs is the correct architectural boundary — unchanged assessment.
- The stale-embedding queries (`internal/jobs/stale_detector.go`) are three sequential Postgres queries per call, each `LIMIT`ed (500/500/200) and indexed on `org_id`/`embedding_status` — reasonably cheap, no pagination needed at current scale.
- The real risk is not API complexity, it's the complete absence of authentication on all three routes, and the complete absence of job-outcome observability.

## Historical bottom line (superseded)

`data-orchestrator-go` is a small, focused, cleanly-built (no stubs, no TODOs, clean `go vet`, near-clean `gofmt`) internal control service — but it is currently running in production-shaped form with (1) a live, reproduced, zero-credential cross-org data disclosure on every one of its three `/v1/orchestrator/*` routes, and (2) job submissions that vanish into the void with no way to ever check their outcome. It does not own `documents.updated`/`source_objects.changed` publishing (that's documents-api-go), it has no ZDR surface (correctly out of scope), and the Quarry-v2 ingest-contract drift item does not touch it at all. The one actionable fix that matters most here: replace `OrgIDMiddleware`'s bare header trust with real bearer/JWT verification against Control Plane before this container sees another org's traffic.

## 2026-07-11 secure-MVP delta (current)

The historical disclosure/vanishing-job conclusion is superseded. Routes now
require verified tenant-pinned JWTs and operation scopes; PostgreSQL job state,
readback, exact idempotency conflict handling, and constrained transitions are
tested. Production mutations return 503 before persistence because the only
publisher is intentionally disabled until a signed resumable worker/callback
contract exists. Full race, vet, build, and govulncheck pass; source-only jobs
coverage is 31.9%. This is secure containment, not restored mutation readiness.
