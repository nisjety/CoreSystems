# INNH-12: Per-run results: pages a crawl/batch fetched (Apify dataset equivalent), and a run_id filter on artifacts

Status: done in tests (dark behind QUARRY_EDGE__JOB_RESULTS, default off; never run on a stack)
Lane: B (control/orchestrator), branch innh/lane-b. Plan home: E4.1 (W4, on hold; the user asked for it now). Built 2026-10-31.
Paths under `apps/Ingestion Plane/Quarry-v2`.

## What existed (verified before building)
No results route. The `page_fetched` event in control's log held url, status, fingerprint, links count, content_type and title, with no artifact ids; the runtime's ArtifactWritten events went to NATS only. `ListFilter` had no run_id; `ListQuery` ignored unknown params, so the documented `kind` filter of `GET /v1/artifacts` was a latent contract lie; `S3Store` has no `list`. INNH-4 had already added `markdown_artifact_id` to the `run_page` result (reused here).

## What I built
1. `page_fetched` is self-describing. Rust `InternalRunPageResult` gains `source_artifact_id` (html, or raw for a document) and `bytes` (size of the stored markdown) beside INNH-4's `markdown_artifact_id`; Go `RunPageResult` decodes them; `addFetchedResultFields` (workflows.go) copies `markdown_artifact_id`, `source_artifact_id`, `bytes`, `word_count`, `driver` onto the event payload, each only when the runtime reported it. Payload map only: no workflow command added or reordered, so a replayed history stays valid.
2. Control `GET /v1/jobs/{id}/results` (`internal/resources/jobs_results.go`): org-scoped GetByOrg (foreign or unknown id 404), `ForJobTypes(page_fetched)` page, rows `{seq,url,status,content_type?,title?,fetched_at,word_count?,driver?,markdown_artifact_id?,source_artifact_id?,bytes?}`, `next_seq` when the page is full, `after_seq`, `limit` default 100 max 500. `latest=true` keeps the latest fetch of each URL (scans at most 20 000 fetches, then `scan_capped: true`).
3. Edge `GET /v1/jobs/:id/results`, `/v1/crawl/:id/results`, `/v1/batch/:id/results` (`job_results_routes.rs`), flag `QUARRY_EDGE__JOB_RESULTS` (off: 501 before control, and 501 wins over the 403). Order: flag, ZDR floor (403, `details.reason: job_result_urls`), query check (400 names the field for limit outside 1..500, bad after_seq, bad latest), job exists and kind matches (else 404), control read. Service principals get 403 from the existing route scope. Envelope: `{data:{items,next_seq?}, meta:{request_id, page:{next?,limit}}}`.
4. Artifact list (not behind the flag, additive): `GET /v1/artifacts?run_id=run_...&kind=markdown`. The documented `kind` filter is now real; a value that is not one of the six documented kinds, or a run_id that is not `run_...`, is a 400 with `details.field`. `ArtifactSummary` gains `run_id` and `page_hash`. Memory store keeps run and page per row; the filesystem store parses them from the key, so the filter survives a restart. S3 still lists nothing.
5. ZDR matrix row `job_results` (Refused FORBIDDEN `job_result_urls`, Enforced; golden JSON, .sha256 and docs/ZDR_MATRIX.md regenerated with the existing mechanism, then rerun without it); `job_result_urls` added to `PENDING_FEATURES` in zdr_feature_matrix.rs. Route-count pin 109 to 112.
6. Docs: openapi (3 paths, `JobResultsView`, `JobResult`, artifact `run_id` param, `ArtifactSummary.run_id/page_hash`), CONTRACTS ("Job results", "Artifact list filters"), RUNBOOKS ("Job results"), REST_RESOURCES, ERROR_REASONS.

## Commits (branch innh/lane-b)
- cc2596c7a feat(quarry-control): per-job results route and self-describing page_fetched (INNH-12)
- 4f2a1bc3c feat(quarry-edge): job results routes and artifact run_id/kind filters behind QUARRY_EDGE__JOB_RESULTS (INNH-12)
- docs(quarry): job results contract, runbook, openapi and status (INNH-12) (this commit)

## Tests (counts)
- Go control `./...` all packages ok; new: 9 in `jobs_results_test.go` (rows from page_fetched in seq order and only those, absent fields left out of the wire, cursor paging, limit default 100 / clamp 500 / junk, a URL fetched twice vs `latest`, `latest` paging, org scoping + unknown + missing org, no leak from another job, in-process numeric payloads).
- Go orchestrator `./...` all ok; new: 3 in `workflows/fetched_results_test.go` (payload carries the fields when RunPage returns them; no empty keys when not; content-free) + 1 in `activities/markdown_artifact_test.go` (RunPage decodes source id and bytes).
- Rust (lane container, `qtest-lane.sh b`): quarry-core 281 lib tests ok (incl. golden), quarry-runtime `artifact_store` 20 ok (5 new: run/kind filters on both stores, after restart, key parsing), quarry-edge lib 673 ok (new: 6 in `job_results_routes`, 2 `from_output` tests, 1 artifact-filter check), integration: `job_results` 7 ok (real router + control double: pass-through and meta.page, query forwarding with the token's org, 400s before control, 404 for kind mismatch/foreign/malformed id, 501 flag off without a control call, 403 ZDR with reason without a control call, 403 service principal), `artifacts_fs_list` 6 ok (1 new: run_id + kind through the real route, org isolation, 400s, restart), `job_lifecycle` 14, `zdr_claim_floor` 26, `zdr_feature_matrix` 14 ok. `cargo check --workspace --all-targets` clean.
- Node contract tests in Quarry-v2: 49 pass (route-count pin updated, openapi-no-bare-null, docs-contract).

## Deviations from the status file's plan
- Edge envelope has `meta.page {next, limit}` and no `links` object (the shared Envelope has none); `next_seq` is in `data` as planned.
- A bad limit is a 400 (as the planned contract said), unlike the errors route which clamps.
- `bytes` is the size of the stored markdown (not the source).
- `latest=true` is capped at 20 000 fetches per call and says so (`scan_capped`); not in the plan.
- Part B (artifact filters) is not behind the flag: it is additive and fixes the documented `kind`. `kind` accepts only the six documented kinds.

## Not verified
- No stack was run: no Temporal replay of an old history, no real Postgres (the pg `ForJobTypes` is unchanged and was not exercised; the control tests use the in-memory store), no side-by-side crawl of five fixture pages, no ZDR token against a live edge. The status file's "done" condition (crawl 5 pages, GET results returns 5 rows with working refs; ZDR token 403) is NOT met on a stack; it is met by route tests with a control double.
- Replay safety is by construction (payload map only) and by the workflow test environment, not by replaying a recorded history.
- Index: `events_job_id_idx` is on `job_id` only (confirmed in 001_init.sql); type and seq filter over that job's rows. No composite index exists.
- The edge and orchestrator images were not rebuilt; rows written before the orchestrator carries the fields have no artifact ids.

## Contract the Innhenting UI needs
`GET /v1/crawl/{id}/results` (also `/v1/batch/{id}/results`, `/v1/jobs/{id}/results`), bearer of the user (not a service identity). Query: `limit` 1..500 (default 100), `after_seq` (cursor), `latest` (bool). 200:
`{ "data": { "items": [ { "seq": 12, "url": "https://e.test/a", "status": 200, "content_type": "text/html", "title": "A", "fetched_at": "2026-10-23T10:00:01Z", "word_count": 120, "driver": "static", "markdown_artifact_id": "art_...", "source_artifact_id": "art_...", "bytes": 900 } ], "next_seq": 12, "scan_capped": false }, "meta": { "request_id": "...", "page": { "next": "12", "limit": 100 } } }`.
`next_seq` is absent on the last page; fields after `fetched_at` are absent, never null. Content: `GET /v1/artifacts/{markdown_artifact_id}` (org-bound, 404 when gone). Errors: 404 (foreign, unknown, wrong kind), 403 (`details.reason: job_result_urls` for ZDR; also service principals), 501 flag off (`QUARRY_EDGE__JOB_RESULTS`), 400 with `details.field`. Artifacts of a run: `GET /v1/artifacts?run_id=run_...&kind=markdown`.

## Owner decisions needed
- Switch `QUARRY_EDGE__JOB_RESULTS=1` on the live edge (flag is dark; the owner flips it) and rebuild edge plus orchestrator (an orchestrator rebuild changes the live worker: prove on a side-by-side stack first).
- Whether the dev compose should default to the filesystem artifact backend so result ids survive an edge restart (today `ARTIFACT_BACKEND=memory`).
