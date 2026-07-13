# data-quality-go Research Dive

Generated: 2026-06-07
Updated: 2026-07-11 (secure-MVP durability evidence; preserves the superseded 2026-07-10 live audit below)

Scope: `apps/Data Plane v2/services/data-quality-go`

## Secure-MVP current state — 2026-07-11

- **Implemented:** all eval, quality, and cost routes now require verified
  RS256/JWKS claims, pin organization from claims, reject conflicting headers,
  and require the `data:quality:admin` scope. The additive
  `20260711160000_quality_orchestrator_durability.sql` migration and
  `PostgresEvalStore` persist tenant-scoped eval runs, enforce organization-local
  idempotency, constrain lifecycle state, and make eval results retrievable
  after the request that created them.
- **Tested:** the disposable-PostgreSQL
  integration passed lifecycle, idempotent replay, and tenant-isolation checks,
  including an idempotent migration application and scoped fixture rollback.
  Current `go test -race ./...`, `go vet ./...`, `go build ./...`, and
  `govulncheck ./...` pass. The changed `internal/authctx` package measures 82.8%
  statement coverage. The source-only `internal/eval` profile is 26.7%, with
  recovery at 69.2%; Docker failure prevents the current PostgreSQL recovery
  profile, so the >=80% package gate remains unproven.
- **Built/deployed/reachable/effective:** an earlier auth-hardened image was
  built locally with revision/build labels, but the current durability source
  and migration have not been proven deployed, reachable, or effective in a
  rebuilt runtime. No current real-bearer endpoint matrix has run. Historical
  header-only live findings below are superseded for source, not disproved in a
  deployed environment.
- **Remaining gates:** rerun PostgreSQL recovery, rebuild the service/migrator,
  safely prove the current migration/runtime, and include a valid scoped quality
  bearer in the isolated matrix. Process health alone is insufficient.

The remainder is a superseded, sanitized pre-fix audit retained for root-cause
history, not a statement about the current dirty worktree or a rebuilt runtime.

Container: `dpv2-data-quality`, port `8013`. Confirmed `Up 14 hours (healthy)` via `docker ps` on 2026-07-10, image `sha256:a7647cf5808...` built 2026-07-02T10:36:14Z (no git revision label in image metadata — health proves process liveness only, not code-revision or tenant-safety readiness).

## Historical pre-fix live audit (superseded)

This pass re-verified the 2026-06-07 snapshot's claims against the running container, the live Postgres schema, and current source. Two claims held up as-is, two were found to understate real problems, and one (Quarry-v2 contract drift, a related cross-plane item) has since been fixed.

| Claim | Verdict | Evidence |
|---|---|---|
| "retrieval quality surface is live here" | Confirmed | `/health`, `/v1/quality/gates`, `/v1/cost/summary`, `/v1/evals/retrieval` all respond live. |
| "data-quality-go is the active eval surface, retrieval-eval-py is empty" | Confirmed | `services/retrieval-eval-py/` contains 0 files; `docker-compose.yml` defines no container for it; `data-quality-go` is the only quality/eval container running. |
| "No explicit code stubs were found in the active service tree" | **Contradicted** | `internal/handler/quality.go:131` (`GetEval`) ignores its path parameter and returns a static placeholder. |
| Cost-summary org-scope trust gap (from the 2026-07-10 plane baseline) | **Confirmed, live-reproduced** | An arbitrary organization header with zero credentials returned 200. Identifiers and body are redacted below. |
| Quarry-v2 `DataPlaneIngestRequest` missing `initiator_user_id`/`visibility` (cross-plane contract drift item in the shared baseline) | **Fixed since last pass** | `crates/quarry-core/src/contracts.rs` now declares both fields; `cargo test -p quarry-core --test contracts` passes 21/21 including `data_plane_ingest_request_serde_roundtrip` and `zdr_on_serializes_correctly`. See "Cross-Plane Contract Check" below. |

Two problems not present in the prior write-up were found this pass:

1. **`GET /v1/quality/lint` is fully broken (HTTP 500).** The same `deleted_at`-column drift previously attributed only to `wiki-store-go` also breaks `data-quality-go`'s own lint checks, because `internal/lint/lint.go`'s wiki-related queries (`findOrphanWikiPages`, `findStaleWikiPages`, `findWeakCitations`) select `p.deleted_at` against `wiki_pages`, and the live `wiki_pages` table has no `deleted_at` column. `Linter.Run` aborts the whole report on the first error, so document-only lint checks (which would work — `documents.deleted_at` does exist) never get returned either. Live-reproduced below.
2. **Eval results are computed then discarded — there is no way to retrieve a scorecard once produced.** `RunEval` is launched via `go func() { h.runner.RunEval(...) }()` against a bare in-memory `*model.EvalRun` pointer with no persistence layer; the caller's HTTP response is already serialized (status `pending`/`running`) before the goroutine finishes, and `GetEval` (the only other handler touching an eval by ID) never reads the URL parameter or any store — it just returns the static stub message from finding 3 above. Net effect: `POST /v1/evals/retrieval` and `POST /v1/evals/compare` can compute scorecards, but nothing durable is ever written and nothing can read them back via the documented `GET /v1/evals/retrieval/{evalID}` route. `RunCompare` (used by `/v1/evals/compare`) happens to work end-to-end because it runs both sub-evals synchronously in the request goroutine and returns the diff directly in the response body — so `/v1/evals/compare` is the only eval route that actually round-trips scores to a caller today.

A third, lower-severity note: the eval scoring itself (`internal/eval/runner.go`) is a synthetic proxy, not an IR-quality metric grounded in judged relevance. `recall_at_10 = min(candidate_count_reranked/10, 1.0)`, `ndcg_at_10 = recall*0.9` (a fixed ratio, not computed from any relevance/position signal), and `mrr = 1.0` if any candidates came back else `0.0`. There is no ground-truth judgment set anywhere in this service or its schema — the "quality" score is really just a shape-of-response heuristic. This isn't caught by grep for TODO/mock/fake because none of those markers are present in the code; it only shows up on reading the formula.

## Historical 2026-06-07 snapshot (superseded)

`data-quality-go` owns retrieval eval runs, trust scoring, quality gates, lint, and cost summary views for Data Plane v2.

- Go HTTP service (chi router) with Postgres backing (`pgxpool`), Prometheus metrics, OTel tracing.
- Non-generated, non-vendored file count: 11 Go source files across `cmd/` + 8 `internal/` packages (848 total lines across the 7 largest non-main files, measured 2026-07-10).

## Runtime Shape

Key runtime entrypoints:

- `cmd/main.go` — wires Postgres pool, eval runner, trust scorer, gate checker, linter, cost query, and the chi HTTP server. No Control Plane / auth client is constructed anywhere in this file or anywhere in the service (`grep -rn "CONTROL_PLANE\|authctx\|Authorization\|Bearer\|jwt\|JWT"` across all `.go` files returns zero matches). `internal/config/config.go` only reads `DATABASE_URL` and `HTTP_PORT` — there is no `CONTROL_PLANE_ENFORCEMENT` or equivalent knob to even opt into strict mode, unlike `documents-api-go` and `retrieval-engine-rs`.
- `internal/handler/quality.go` — all HTTP handlers; `OrgIDMiddleware` (lines 23-33) is the **entire** access-control surface for `/v1/evals/*`, `/v1/quality/*`, and `/v1/cost/*`: it requires the `X-Org-ID` header be non-empty and otherwise does nothing — no signature check, no lookup against Control Plane, no correlation with any bearer token.
- `internal/eval/*` — eval run creation/execution/comparison (synthetic scoring, see above; results are not persisted).
- `internal/trust/*` — per-document trust scoring: `authority` from a hardcoded
  source map blended 60/40 with an age-bucketed freshness score. It reads
  `documents` scoped by tenant and soft-delete state; the historical empty-input
  check returned 200, with body omitted.
- `internal/gates/*` — 6 release gates (`retrieval_traces_exist`, `no_failed_embeddings`, `indexed_docs_have_chunks`, `retrieval_p95_under_2000ms`, `no_zero_result_queries`, `retrieval_p95_under_800ms_spec`), all real SQL against `retrieval_runs`, `knowledge_units`, `documents` — schema-correct and live-tested (see below).
- `internal/lint/*` — 5 lint checks (`orphan_docs`, `stale_docs` against `documents`; `orphan_wiki`, `stale_wiki`, `weak_citations` against `wiki_pages`/`wiki_page_versions`). The wiki-side checks are broken live (see finding above).
- `internal/cost/*` — aggregates `cost_events` (written by `data-orchestrator-go` from NATS) into per-org token/event buckets.
- `internal/model/*` — shared DTOs (`EvalRun`, `Scorecard`, `TrustScore`, `GateResult`, etc.)
- `internal/metrics/*`, `internal/otel/*` — Prometheus middleware (`dpv2_data_quality_go_*` metrics) and OTel init; unremarkable, no findings.

Primary surface (all confirmed live 2026-07-10 except where noted):

- `POST /v1/evals/retrieval` — 202 Accepted, kicks off an eval in a goroutine; result unrecoverable (see finding).
- `GET /v1/evals/retrieval/{evalID}` — always 200 with the same hardcoded stub message, ignores `{evalID}`.
- `POST /v1/evals/compare` — 200, computes and returns both scorecards + diff synchronously (the one eval route that actually works end-to-end).
- `POST /v1/quality/trust` — 200, real per-document trust scores.
- `GET /v1/quality/gates` — 200/412 (412 when a gate fails), real SQL-backed release gates.
- `GET /v1/quality/lint` — **500** live, broken by wiki `deleted_at` schema drift.
- `GET /v1/cost/summary` — 200, real cost aggregation, **zero-credential org-scope trust gap**.
- `GET /health`, `GET /readyz`, `GET /metrics` — unauthenticated by design (standard for these), all 200.

## Live Evidence

### 1. Cost-summary org-scope trust gap (P0 — reproduces the plane-wide X-Org-ID finding exactly)

The historical no-auth request with a caller-selected organization header returned
HTTP 200. Tenant identifier, time range, counts, cost fields, and response body are
redacted.

No `Authorization` header, no API key, no session cookie — any caller who can reach port 8013 gets a 200 with cost data scoped to whatever string they put in `X-Org-ID`. Omitting the header entirely does 400 ("X-Org-ID header required"), confirming the header's presence is the *only* thing checked (`internal/handler/quality.go:23-33`, `OrgIDMiddleware`). This is the exact same shape as the already-known Control Plane `X-Org-ID`/`X-User-Role` trust gaps and the sibling `graph-index`/`data-orchestrator` findings from the 2026-07-10 plane baseline — `/v1/quality/gates`, `/v1/quality/trust`, `/v1/evals/*` are all behind the identical `OrgIDMiddleware` and equally exposed (confirmed live for `/v1/quality/gates` with a second arbitrary org string, which also returned 200/412 with no credential).

### 2. `GET /v1/quality/lint` — HTTP 500, previously undocumented for this service

The historical lint request returned HTTP 500 with SQLSTATE 42703. Tenant and
response body are redacted; the schema root cause is preserved below.

Confirmed via `docker exec dpv2-postgres psql ... \d wiki_pages` that the live table has no `deleted_at` column (columns are `page_id, org_id, workspace_id, title, path, current_version_id, page_status, backlinks, metadata, created_at, updated_at`). `documents.deleted_at` *does* exist (confirmed via `\d documents`), so the document-only lint checks would work in isolation, but `Linter.Run` (`internal/lint/lint.go:43-75`) runs all 5 checks in a fixed order and returns the first error immediately, so the whole endpoint 500s. The prior baseline attributed this exact schema drift only to `wiki-store-go`; it independently breaks `data-quality-go`'s lint surface too.

### 3. Eval result retrieval is a dead-end stub

The historical create call returned 202, while a subsequent lookup returned 200
with a static placeholder rather than the computed result. Organization, eval ID,
request payload, timestamps, and response bodies are redacted.

The `eval_id` returned from the POST is never usable — the GET handler doesn't read the path parameter at all (`internal/handler/quality.go:129-133`). Combined with `RunEval`'s in-memory-only scorecard (never written to Postgres or anywhere else), the computed eval result is unconditionally thrown away once the goroutine returns.

### 4. Cross-Plane Contract Check — Quarry-v2 `DataPlaneIngestRequest` (fixed since last pass)

The shared plane baseline flagged Quarry-v2's `cargo test --workspace` failing because `DataPlaneIngestRequest` constructors lacked `initiator_user_id`/`visibility`. Re-checked 2026-07-10:

```
$ grep -n "initiator_user_id\|visibility" crates/quarry-core/src/contracts.rs
    pub initiator_user_id: Option<String>,
    pub visibility: Option<String>,   // "private" | "org" | "shared"

$ cargo test --workspace --no-run   # compiles clean, all 21 test binaries built
$ cargo test -p quarry-core --test contracts
running 21 tests ... test result: ok. 21 passed; 0 failed
```

Both fields are now present and exercised by `data_plane_ingest_request_serde_roundtrip` and `zdr_on_serializes_correctly`. This item is resolved — no further action needed on the Quarry-v2 side of this contract.

## API And Relationship Map

- internal operators and evaluation tooling -> `data-quality-go` (no verified caller identity, per finding above)
- `data-quality-go` -> Postgres (`retrieval_runs`, `retrieval_candidates`, `knowledge_units`, `documents`, `wiki_pages`, `wiki_page_versions`, `cost_events`)
- `data-quality-go` -> broader Data Plane runtime — quality judgments depend on retrieval and corpus behavior elsewhere in the plane; `cost_events` rows are written by `data-orchestrator-go` from NATS, not by this service.

## Duplicates, Redundancies, And Inactive Surfaces

- `services/retrieval-eval-py/` remains a fully empty directory (0 files) with no compose service — `apps/STALE_DOC_DELETION_REGISTER.md`'s claim that `data-quality-go` is the active eval surface is **confirmed still true** 2026-07-10.

## Stubs, Placeholders, And Missing Connections

Corrected from the 2026-06-07 pass, which stated "No explicit code stubs were found in the active service tree" — that is no longer accurate (or was already inaccurate):

- `GetEval` handler (`internal/handler/quality.go:129-133`) is a hardcoded stub, unconditionally returning the same message regardless of the requested eval ID.
- `RunEval`'s scorecard has no persistence path — it is computed in a detached goroutine against a bare struct pointer and discarded.
- Grep for `TODO|FIXME|mock|stub|fake|placeholder` across the service returns **zero hits** — the only human-readable admission of incompleteness is the literal string `"requires persistent storage (future)"` at `quality.go:131`, which a naive TODO/stub grep will miss. Treat the absence of matches from that grep as a gap in the grep, not evidence of completeness.

## API Design And Performance Notes

- Route grouping (quality/trust/lint/cost together, separate from eval) still makes sense as "evaluate the retrieval estate" vs. "run/compare a specific strategy."
- Every mutating/reading route in this service trusts a client-supplied `X-Org-ID` with zero verification — this is a correctness/security concern before it's a performance one.
- Route latency is not the bottleneck; correctness (schema drift, missing persistence) is.

## Toolchain / Static Analysis (re-verified 2026-07-10)

| Command | Result | Notes |
|---|---|---|
| `go build ./...` | Pass | Builds clean. |
| `go vet ./...` | Pass | No findings. |
| `gofmt -l .` | **Fail** | Drift in `internal/cost/query.go`, `internal/eval/runner.go`, `internal/lint/lint.go`, `internal/model/eval.go` — mostly struct-tag column alignment (`gofmt -d` shows only whitespace/alignment diffs, no semantic changes). Matches the plane baseline's "Go fmt drift in the 4 Go services," confirmed still present specifically in these 4 files within this one service. |
| `golangci-lint` | Not available | Binary not installed in this environment; could not run. |
| Test files | None found | `find . -name "*_test.go"` returns nothing — this service has zero automated test coverage. |

## Current Doc Cleanup Read

Keep and treat as current:

- This file (`data-quality-go.md`), refreshed 2026-07-10.

Update or archive, not delete:

- `docs/gap-data.md` — per the existing stale-doc register, still implies `retrieval-eval-py` scaffold as the active quality surface in places; re-confirmed 2026-07-10 that `data-quality-go` is the real one and `retrieval-eval-py` is empty.

## Historical bottom line (superseded)

`data-quality-go` is a real, running service — it is genuinely the only live eval/quality surface in Data Plane v2, and its trust-scoring and gate-checking logic execute real, schema-correct, org-scoped SQL. But three things materially undercut the "active eval surface" framing: (1) it has no authentication of any kind — any caller who can set an `X-Org-ID` header gets full read access to another org's cost and quality data, live-reproduced; (2) its lint endpoint 500s outright due to a `wiki_pages.deleted_at` schema mismatch that the prior audit only attributed to `wiki-store-go`; and (3) the eval-by-strategy workflow computes real numbers and then structurally cannot return them to the caller who asked for them — the only eval route that actually works end-to-end today is `/v1/evals/compare`. Separately, the previously-flagged Quarry-v2 cross-plane contract drift (`initiator_user_id`/`visibility` on `DataPlaneIngestRequest`) has been fixed and is confirmed passing 21/21 tests as of this pass.

## 2026-07-11 secure-MVP delta (current)

The historical auth/stub conclusions are superseded. Every route requires a
verified tenant-pinned JWT plus `data:quality:admin`; eval lifecycle and readback
are PostgreSQL-backed with exact idempotency conflict detection. A recovery loop
atomically resumes pending and expired-running work across replicas. Full race,
vet, build, and govulncheck pass. The source-only eval profile is 26.7%
(recovery 69.2%); PostgreSQL recovery integration/coverage remains Docker-blocked.
