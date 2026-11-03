# INNH-12 progress

Lane b run of 2026-10-31. Continue from the tree: every unit below leaves a checkable change.

Design decisions:
- Flag `QUARRY_EDGE__JOB_RESULTS`, default off (unset in the composes). Off: the three edge routes answer 501 before control is called, and 501 wins over the ZDR 403. The orchestrator and edge payload additions are additive and need no flag.
- Rows are `page_fetched` events only (failed and blocked pages stay on `/errors`). The event now carries `markdown_artifact_id`, `source_artifact_id`, `bytes` (size of the stored markdown), `word_count`, `driver`, each only when the runtime reported it.
- Control owns the join: `GET /v1/jobs/{id}/results` (org-scoped GetByOrg, `ForJobTypes` page, `next_seq` cursor, default 100, max 500). `latest=true` dedupes by URL, scanning at most 20 000 fetches (`scan_capped`).
- Edge aliases for crawl, batch and jobs; kind mismatch is 404; ZDR floor is 403 with `details.reason: job_result_urls`; a bad limit, after_seq or latest is 400 naming the field.
- Part B (artifact list) is not behind the flag: `run_id` and `kind` filters, `ArtifactSummary.run_id` and `page_hash`. `kind` is the documented filter, now real; a value that cannot match is a 400.

Units:
- [x] U1 run_page result: `source_artifact_id` and `bytes` on `InternalRunPageResult` (Rust), `RunPageResult` (Go activities), and `addFetchedResultFields` on the `page_fetched` payload (workflows.go); payload map only, no workflow command change
- [x] U2 control: `jobs_results.go` + `jobs_results_test.go`, route mounted next to `/errors`
- [x] U3 edge: `job_results_routes.rs` (flag, gate, params, aliases), `JobResultView`/`JobResultsView` in quarry-core, routes in `routes.rs`, `tests/job_results.rs`
- [x] U4 artifact filters: `ListFilter.run_id/kind`, `ListQuery`, `check_artifact_filters`, memory and filesystem stores, `ArtifactSummary.run_id/page_hash`, store unit tests, `artifacts_fs_list.rs` integration test
- [x] U5 docs and pins: openapi (3 paths, 3 schemas, artifact params), CONTRACTS, RUNBOOKS, REST_RESOURCES, ERROR_REASONS, ZDR matrix row `job_results` (+ golden, .sha256, ZDR_MATRIX.md), `PENDING_FEATURES`, route-count pin
- [x] U6 tests green (counts in INNH-12-status.md)
- [x] U7 status, depth findings, commits
