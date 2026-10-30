# INNH-12: Per-run results: pages a crawl/batch fetched (Apify dataset equivalent), and a run_id filter on artifacts

Status: not started
Lane: B (control/orchestrator). Plan home: E4.1 (W4, on hold; the user asked for it now). Estimate: 4.5 d (core 3 d + optional run_id filter 1.5 d).
Verified by reading quarryv2-next. Nothing was built or run. Paths under `apps/Ingestion Plane/Quarry-v2`.

## What exists
NO results route exists: edge routes.rs:150-295 has crawl/batch status (:160-163, :167-169), errors, ongoing, events tails (/v1/jobs/:id/events, /v1/runs/:id/events, /v1/crawl|batch/:id/events; resource_routes list_job_events/list_run_events) and /v1/artifacts, /v1/artifacts/:id; nothing named results/pages. Raw material:
1. The pipeline writes per page, for non-ZDR runs, three artifacts keyed by run: html (or raw for documents), markdown and meta.json (`quarry-runtime/src/pipeline.rs:1311-1400`; key `org=<org>/run=<run_id>/page=<page_hash>/<kind>.<ext>` via `quarry-core/src/artifact.rs:63-69` object_key), and emits ArtifactWritten events {url, format, artifact_id, bytes} (pipeline.rs:1341-1388) but those go to the runtime EventSink (NATS/local), NOT control's event log (the orchestrator re-emits only page_fetched, branding_extracted and page_extracted, workflows.go:268-290).
2. Control's per-job event log holds page_fetched {url, status, fingerprint, links:count, content_type, title} (workflows.go:253-268), page_extracted {url, title, excerpt, word_count, driver, ...} (pageExtractedPayload workflows.go ~290-318) and final page_failed rows (workflows.go:226-247); readable today only as a raw event stream via GET /v1/jobs/{id}/events with a free-form `payload` (event_wire.go toJobHistoryEvent), not as a paged list of pages, and neither payload carries artifact ids or bytes (RunPageResult activities.go:204-225 and InternalRunPageResult routes.rs:1376-1417 have no artifact fields although NormalizedOutput.formats holds the ids, pipeline.rs:1395-1400).
3. ListFilter (`quarry-core/src/pagination.rs:63-88`) = status, created_before, created_after, limit, cursor, sort; no run_id. ListQuery (resource_routes.rs:49-64) mirrors it and ignores unknown params, so even the documented `kind` filter of openapi.yaml /v1/artifacts (:1314-1370, enum markdown/html/raw/pdf/screenshot/extract) is silently NOT implemented. ArtifactSummary (resources.rs ~:84-97) = artifact_id, org_id, kind, bytes, sha256 (always None), created_at, source_url (always None): no run id, no page url. Backends: InMemoryStore keeps meta rows (artifact_store.rs:223-337), FilesystemStore lists from its .index and parses the org= segment (:345-590, list :537-575), S3Store has NO list (trait default returns an empty page, artifact_store.rs:146-153), so on S3 /v1/artifacts is empty. Deploy default: docker-compose.yml:172 uses ARTIFACT_BACKEND memory (artifacts vanish on edge restart), docker-compose.production.yml:60 filesystem.
4. ZDR: crawl and batch are refused for ZDR callers at handoff (routes.rs ~:1188 refuse_durable_job_under_zdr; docs/ZDR_MATRIX.md durable_jobs row) so no ZDR job has results.

## The gap
Recommended build is a per-job results route (the dataset equivalent), not an artifact filter, because the job-to-page-to-artifact join already exists in control's event log and does not depend on which artifact backend lists.
(A) CORE.
1. Make page_fetched self-describing: add closed, content-free fields markdown_artifact_id, source_artifact_id, bytes, word_count, driver to InternalRunPageResult::from_output (routes.rs:1424), to activities.RunPageResult (activities.go:204) and to the `fetched` map (workflows.go:253): additive, JSON-tolerant, no workflow command change (still verify Temporal replay safety: payload map only; run workflow replay tests). Note INNH-4 adds markdown_artifact_id to the same structs; coordinate to build once.
2. Control: GET /v1/jobs/{id}/results built like jobErrors (jobs_lifecycle.go:286-306): org-scoped GetByOrg, db.Events().ForJobTypes(id, [page_fetched], after_seq, limit) (store.go:300), rows {url,status,content_type,title,fetched_at,word_count,driver,markdown_ref,source_ref,bytes,seq}, `next_seq` cursor, default 100, max 500; a URL fetched twice appears twice (retries/escalation); `latest=true` dedupes by latest per URL.
3. Edge: /v1/jobs/:id/results, /v1/crawl/:id/results, /v1/batch/:id/results (kind check like errors_for job_lifecycle_routes.rs:180-215; ZDR token -> 403 because it returns URLs, like the errors route; service principals 403 by the default scope rule). Content is fetched by the existing GET /v1/artifacts/{id} (org-bound); `?include=markdown` NOT recommended for v1; ndjson export is a follow-up.
(B) OPTIONAL (+1.5 d): `run_id` on ListFilter/ListQuery and ArtifactSummary.run_id/page_hash, implemented in list_window/InMemory meta/Filesystem (parse `run=`), and implement the documented `kind` filter; S3 remains unlistable until a ListObjectsV2 implementation is written (separate slice).
Gaps the audit did not state: the event stream already exposes page urls so a client could reconstruct results today (poor UX, no artifact refs); artifact refs dangle after an edge restart on the memory backend; page_hash is url+fingerprint so a changed page in a later run is a different artifact.

## Files
Rust: `crates/quarry-edge/src/routes.rs:1376-1440` (InternalRunPageResult fields + from_output), NEW `crates/quarry-edge/src/job_results_routes.rs` (+ lib.rs mod) or add to `job_lifecycle_routes.rs` near errors_for (:180-215) and the route table routes.rs:160-170; `crates/quarry-core/src/resources.rs` (JobResultView, JobResultsView next to JobErrorsView ~:278-300).
Go orchestrator: `services/quarry-orchestrator/internal/activities/activities.go:204-225`, `internal/workflows/workflows.go:253-268` (+ activities_test.go, workflows tests).
Go control: NEW `services/quarry-control/internal/resources/jobs_results.go` (+ _test.go) mounted next to jobSummary/jobErrors (where MountJobs registers /v1/jobs/{id}/errors); event_wire.go untouched.
ZDR: `quarry-core/src/zdr_matrix.rs` + `docs/ZDR_MATRIX.md` (new row job_results: refused FORBIDDEN, same shape as job_errors), `scripts/tests/zdr-contract-sync.test.sh` (append-only), `tests/zdr_gap_rows.rs`.
Docs: `docs/openapi.yaml` (3 paths + JobResultsView schema), `docs/CONTRACTS.md` (beside "Errors (edge)" ~:745-752), `docs/REST_RESOURCES.md`, openapi route allowlist if needed.
Optional (B): `quarry-core/src/pagination.rs:63-88`, `quarry-edge/src/resource_routes.rs:49-90, :120-131`, `quarry-runtime/src/artifact_store.rs:146-153` (trait), `:223-337` (memory meta + run_id), `:345-590` (fs parse), list_window `:174-215`, ArtifactSummary in resources.rs, `graphql/schema.rs:29` ListFilterInput. No migration.

## Flag
Edge `QUARRY_EDGE__JOB_RESULTS`, default OFF (501, same pattern as job control): recommended even though the route is a read, because it returns page URLs and artifact refs for the first time. The orchestrator/edge payload additions are additive and need no flag. Do not touch the live QUARRY_EDGE__SCRAPE_ACTIONS, SCRAPE_CAPTURE, JOB_CONTROL, AI_FORMATS or QUARRY_SCHEDULE_CAPABILITIES. Needs rebuilt edge AND orchestrator images (an orchestrator rebuild changes the live worker; Temporal replay safety must be checked before any live swap, so prove on a side-by-side stack and ask before touching live).

## Contract change
GET /v1/jobs/{id}/results (also /v1/crawl/{id}/results and /v1/batch/{id}/results; kind mismatch or foreign id is 404). Query: limit (1..500, default 100), after_seq (cursor), latest (bool, default false). 200 `{data:{items:[{seq:int, url:string (final URL), status:int, content_type?, title?, fetched_at:RFC3339, word_count?, driver?:'static'|'tls'|'browser', markdown_artifact_id?:'art_...', source_artifact_id?:'art_...', bytes?:int}], next_seq?:int}, meta:{request_id}}`. Rows come from page_fetched only; failed and blocked pages stay on /errors. Items written before the page_fetched fields existed carry url/status/title only; `next_seq` is set only when the page was full. Content: GET /v1/artifacts/{markdown_artifact_id} (existing, org-bound, 404 if gone). Errors: 404 NOT_FOUND (foreign/unknown/kind mismatch/not a job id), 403 FORBIDDEN under the token's ZDR floor (details.reason 'job_result_urls'), 403 for service principals, 501 UNSUPPORTED when the flag is off, 400 on a bad limit. Optional (B): `GET /v1/artifacts?run_id=run_...&kind=markdown` filters (documented `kind` now real), ArtifactSummary gains run_id and page_hash (additive).

## Tests to add
Go control: `jobs_results_test.go` - rows from page_fetched in seq order, cursor paging with next_seq, limit clamp 500, latest=true dedupes by URL, foreign org -> 404, unknown job -> 404, failed/blocked pages absent, rows from before the new payload fields tolerate missing artifact ids.
Go orchestrator: workflows_test.go/activities_test.go - page_fetched payload carries markdown_artifact_id/bytes/word_count/driver when RunPage returns them, absent otherwise; replay test unchanged.
Rust (Docker recipe): routes.rs unit test InternalRunPageResult::from_output includes artifact ids from NormalizedOutput.formats and omits them under ZDR (formats None); job_results_routes tests with mock control (pattern tests/job_lifecycle.rs: org from claims, kind mismatch 404, ZDR token 403, service token 403, flag off 501, next_seq passthrough); an integration test running a stubbed crawl page through PageRunner with InMemoryStore asserting the id in the result resolves via GET /v1/artifacts/{id} for the same org and 404s for another org. ZDR: new matrix row + zdr_gap_rows test; zdr-contract-sync.test.sh row.
Optional (B): artifacts_fs_list.rs extension - ?run_id= returns only that run's artifacts on the fs backend and across a restart, memory likewise, another org sees none; ListFilter default serializes without run_id (pagination.rs tests ~:197-260); `kind` filter test.
Docs: openapi-route-coverage, openapi-no-bare-null, docs-contract. Done requires a side-stack run: crawl 5 fixture pages, GET /v1/crawl/{id}/results returns 5 rows with working markdown refs; same call with a ZDR token is 403.

## Risks
- ZDR/privacy: results return URLs and titles so ZDR callers are refused; page_fetched payloads already persist url+title in control's event log (existing posture; events are not purged by org erasure, gdpr_purge.go:28 lists events as untouched): this adds no retention but makes it easy to read; document in org-erasure notes. Artifact content stays behind the org-bound GET /v1/artifacts/{id}.
- Security: org scope enforced twice (control GetByOrg + artifact org binding via ensure_org_owns); a foreign job id must be 404 exactly like errors; ids parsed (JobKind) before reaching control as in load_summary; never echo raw error text.
- Cost: reads are cheap (one indexed ForJobTypes page); no inline markdown (a 500-row page of full markdown can reach tens of MB).
- Durability: on the memory artifact backend (dev default) markdown_artifact_id 404s after an edge restart; S3 cannot list (run_id filter returns nothing) though refs work via get; state it in CONTRACTS.md.
- Duplicates/retries: a URL can have several page_fetched rows; clients use latest=true for a dataset view.
- Temporal: adding fields to RunPageResult/page_fetched payload is replay-safe only if no new commands are introduced; run the orchestrator workflow tests.
- Scale: a 5,000-page crawl is 5,000 rows paged at 500; ForJobTypes is by (job_id, type, seq): confirm an index exists (SummaryForJob/CountsForJobs use grouped queries, pg/resources.go:1297, :1323).
- OpenAPI: the documented but unimplemented `kind` artifact filter is a latent contract lie; fix or remove it in the same edit.

## Commits
(none yet)

## Tests (counts)
(none yet)

## Not verified
Code reading only; nothing built or run. Unverified: index on (job_id, type, seq); Temporal replay safety of the payload change.
