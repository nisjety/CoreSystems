# INNH-13: Assess only: retry/resurrect of a failed run, run-finished webhooks, per-run log/event export as JSON, abort for crawl and batch

Status: not started (assess only, lowest priority; nothing to build now)
Lane: D. Estimate for the assessment: 0.5 d. Build estimates per sub-item below.
Verified by reading quarryv2-next. Nothing was built or run. Docker is off; the 2 failing orchestrator tests and 1 control test were taken from W2-closeout.md, not re-run. Paths under `apps/Ingestion Plane/Quarry-v2`.

## What exists
(a) RETRY/RESURRECT FROM LAST STATE
- Agent runs only: POST /v1/agent/runs with resume_run_id (`agent_routes/lifecycle.rs:149`, :166-250). Loads the durable checkpoint, refuses a foreign owner with 404 (:190-200), a closed run with 409 (:201-207), any ZDR run with 403 (:208-220); persisted constraints authoritative; step keys continue past the highest receipt (tests/agent_resume_step_keys.rs; openapi.yaml:508-509).
- Crawl, batch, scrape, extract: NO retry or rerun route. Searched openapi.yaml, openapi-internal.yaml, quarry-control routes and edge routes.rs for retry, rerun, resurrect, restart: none. Control's POST /v1/webhook-deliveries/{id}/retry (resources/webhook_deliveries.go:16) retries a webhook delivery only. Temporal activity retries exist inside a workflow (`services/quarry-orchestrator/internal/workflows/workflows.go:113-117`, MaximumAttempts 5), and the job-control loop can pause/resume, but a terminal failed or timed_out job cannot be restarted. Per-URL errors are listed (GET /v1/jobs/{id}/errors, /v1/crawl/{id}/errors, /v1/batch/{id}/errors, WS15.L1) with a retryable flag (jobs_lifecycle.go:61, :245-254), but nothing consumes them to re-queue.

(b) RUN-FINISHED WEBHOOKS
- Control stores generic webhooks: `services/quarry-control/internal/resources/webhooks.go:8` mounts /v1/webhooks CRUD (store.Webhook{id,url,secret,events[],active,created_at}, store/store.go:691-698). Event ingestion calls fanoutWebhooks for EVERY appended event (resources/resources.go:162, fanout :621-660), matching wh.Events (empty = all, '*' or exact type) and creating pending WebhookDelivery rows. `internal/dispatcher/dispatcher.go` signs (HMAC) and posts via egress.SafeClient (SSRF guard), 5 attempts with backoff 1s, 5s, 30s, 5m, 1h, then DLQ (RUNBOOKS.md:914).
- Terminal event types exist for all job kinds: run_completed, run_failed, run_cancelled, run_timed_out (and run_paused/resumed, schedule_skipped) via jobStatusForEvent (resources.go:245-264).
- The edge does NOT expose /v1/webhooks or deliveries (routes.rs has no such route), and openapi.yaml has none. The only edge-visible webhook is the change webhook (change_webhook.rs to control /v1/webhooks/change).
- CRITICAL: webhooks have NO organization. Migration `store/pg/migrations/002_webhooks_blocklist.sql:6-13` has no org_id column, store.Webhook has no OrgID, and fanoutWebhooks lists the first 1000 webhooks of the whole system with no tenant filter (resources.go:632-633). Any active webhook would receive every organization's events. Also the blocker for INNH-5.

(c) PER-RUN LOG/EVENT EXPORT AS JSON
- GET /v1/runs/{run_id}/events (openapi.yaml:2256; resource_routes list_run_events): org-scoped, ordered job-history page, after_seq and limit (max 1000), data {items[JobHistoryEvent], next_cursor, total_estimated}.
- GET /v1/jobs/{job_id}/events, /v1/crawl/{id}/events, /v1/batch/{id}/events (openapi.yaml:2315, :2386, :2421; routes.rs:173-176, :258): JSON drain by after_seq or Last-Event-ID, or SSE live tail with follow=true behind QUARRY_EDGE__JOB_STREAM_FOLLOW=1 (job_stream.rs).
- Errors: /v1/jobs/{id}/errors etc. Agent runs: GET /v1/agent/runs/{id}/receipts, /egress-receipts, /devtools, /events/stream (SSE with Last-Event-ID resume), /procedure (openapi.yaml:929-1225).
- ZDR: control_records and job_errors are refused under ZDR (ZDR_MATRIX.md); job-history events carry no page URLs for ZDR.
- Missing for "export as a file": no single-call full export (no NDJSON/JSON download of all events plus artifact references), no content-disposition or size bound documented; pagination already allows a client-side export.

(d) ABORT FOR CRAWL AND BATCH END TO END
- Built, dark: DELETE /v1/crawl/{id}, DELETE /v1/batch/{id} (cancel), POST .../pause, .../resume (routes.rs:166-171; `job_lifecycle_routes.rs:479-520`; edge flag QUARRY_EDGE__JOB_CONTROL, job_lifecycle_routes.rs:338-343, off answers 501). The edge writes desired state to control (POST /v1/jobs/{id}/control); the orchestrator control loop (QUARRY_JOB_CONTROL_LOOP=1) reads GET /v1/internal/jobs/pending-control every 2 s, signals the Temporal workflow, force-terminates after QUARRY_CANCEL_GRACE_S (default 120) with run_cancelled {forced:true}; sweep every 30 s corrects closed workflows (RUNBOOKS.md:1155-1168). Typed timeouts: QUARRY_JOB_TIMEOUT_SCRAPE_S/BATCH_S/CRAWL_S. Answers 202 pending or 200 applied, 404 foreign, 409 not_running/not_paused/not_controllable.
- Status per `.plan-work/quarry-w1w2/W2-closeout.md:46` (U28): "Light pass: 1 control test and 2 orchestrator tests (Pause-twice, timeouts env) fail"; deployment evidence (a 5000-page crawl with pause, resume, cancel) is owed in the wave's stack phase and RUNBOOKS says it "is not claimed". So abort is built but NOT proven end to end, and live has the flags off.
- A scrape cannot be cancelled once running (not_controllable). A batch is cancellable only through the same control loop. Agent runs close via DELETE /v1/agent/runs/{id}. Model Plane registry actions (ingestions.cancel_run etc.) and SPA controls are WS15.L5 (Verevon gateway has jobs/{id}/control in the audit).

## The gap (verdicts and build estimates; do NOT build now)
- (d) Abort crawl/batch: exists, needs PROOF not code. Fix or re-baseline the 3 failing tests, then a side-by-side stack proof (cancel a running crawl, a paused crawl, a batch; forced cancel after grace; timeout). 1 to 1.5 d. Flags stay off until the owner approves QUARRY_EDGE__JOB_CONTROL and QUARRY_JOB_CONTROL_LOOP.
- (c) JSON event export: mostly exists (paged JSON at /v1/runs/{id}/events and /v1/jobs/{id}/events). Gap is a documented, bounded "export" convenience: NDJSON or one JSON document with all events up to a size/page cap, plus openapi examples. 0.5 to 1 d (optional; clients can page today). Trap: /v1/runs/{id}/events takes the Temporal run id, /v1/jobs/{id}/events takes the handoff job id.
- (b) Run-finished webhooks for any job kind: plumbing exists, tenancy is missing. Needs: (1) org_id on webhooks and fanout (migration, store.Webhook.OrgID, ListByOrg, fanoutWebhooks filters by the event's org), (2) a tenant-facing org-scoped create/list/delete API on the edge + openapi (shared with INNH-5, so ONE build), (3) an events whitelist with a canonical public event name for run completion per kind (map run_completed/failed/cancelled/timed_out to e.g. quarry.run.finished with job_id, kind, status, counts, statistics), (4) a payload that never carries page content or URLs under ZDR (ZDR orgs have no durable jobs, likely refused), (5) secret handling (secret stored in clear, returned by List; return once, store hashed or encrypted), (6) delivery inspection with org scope. 4 to 5 d; bundle with INNH-5. Security-sensitive: SSRF is handled by egress.SafeClient, but cross-tenant delivery is the open hole.
- (a) Retry/resurrect: only agent runs can resume. A job-level "retry failed URLs" is the realistic equivalent: POST /v1/jobs/{id}/retry (or /v1/crawl/{id}/retry) starting a new job from the failed or retryable URL set of a terminal job, linked by parent_job_id, idempotent, admission and quota checked. The error list with retryable flag already feeds it. 3 to 4 d. True mid-crawl resurrect from frontier state (Apify-style) additionally needs the durable frontier (W3 E3.x, change/refresh) and is 5+ d; recommend the URL-level retry only.

## Files
Read-only assessment; nothing to edit now. A later build would touch:
- Abort proof: `services/quarry-orchestrator/internal/jobs/` and `workflows/` (the 2 failing tests), `services/quarry-control/internal/resources/jobs_control.go` and `jobs_control_test.go`, `crates/quarry-edge/src/job_lifecycle_routes.rs`, `docs/RUNBOOKS.md:1155-1168`, a .plan-work evidence file.
- JSON export: `crates/quarry-edge/src/resource_routes.rs` (list_run_events/list_job_events), `job_stream.rs`, `docs/openapi.yaml:2256-2430`.
- Webhooks: `services/quarry-control/internal/store/store.go:691`, `store/pg/webhooks_blocklist.go`, `store/pg/migrations` (new migration after 002; check the highest number first; INNH-5 plans 025), `resources/resources.go:621-660`, `resources/webhooks.go`, `resources/webhook_deliveries.go`, `dispatcher/dispatcher.go`; edge new `webhook_routes.rs`, `routes.rs`, `control_client.rs`; `docs/openapi.yaml`, `docs/CONTRACTS.md`.
- Retry: `crates/quarry-edge/src/job_lifecycle_routes.rs`, `handoff.rs` (crawl/batch handoff builders), `services/quarry-control/internal/resources/jobs_lifecycle.go` and `jobs_control.go`, `docs/openapi.yaml`.

## Flag
Existing: QUARRY_EDGE__JOB_CONTROL (edge; off answers 501), QUARRY_JOB_CONTROL_LOOP (orchestrator), QUARRY_CANCEL_GRACE_S, QUARRY_JOB_TIMEOUT_*_S, QUARRY_EDGE__JOB_STREAM_FOLLOW. All off on live; owner must approve each. Proposed, default off: QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS (edge routes), QUARRY_CONTROL_WEBHOOK_ORG_SCOPING (control; fanout must not stay unscoped even with the flag off, see INNH-5), QUARRY_EDGE__JOB_RETRY.

## Contract change
None for the assessment. Candidates if the owner picks items later:
- Webhook subscription (shared with INNH-5): POST /v1/webhooks `{url (https only, SSRF-checked), events[] from a closed list [run.finished, change.detected], secret? (server generates, returned once)}`; GET /v1/webhooks; DELETE /v1/webhooks/{id}; GET /v1/webhooks/{id}/deliveries. Delivery body `{id, type:'run.finished', created_at, data:{job_id, kind, status (completed|failed|cancelled|timed_out), counts, started_at, finished_at, origin, schedule_id?, statistics}}` with header X-Quarry-Signature (HMAC-SHA256 over timestamp.body). Note INNH-5 proposes the path /v1/webhook-subscriptions; reconcile to one.
- Retry: POST /v1/jobs/{id}/retry `{scope: 'failed_urls'|'all'}` -> 202 `{data:{job_id (new), parent_job_id, status, counts_queued}}`; 409 job_not_terminal, 404 foreign id, 429 admission, 402 allowance.
- Export: GET /v1/jobs/{id}/events?format=ndjson&limit_events=N (additive).
- Abort: unchanged (already documented in RUNBOOKS and the verbs in openapi:1664-1886).

## Tests to add
None for the assessment. For later builds:
- Abort proof: orchestrator tests for Pause-twice and the timeouts env (fix), control job-control test, a side-by-side stack script (cancel running crawl, cancel paused crawl, cancel batch, forced cancel, timeout) with evidence written to .plan-work.
- Webhooks: Go store and fanout tests with two orgs (an event of org A never creates a delivery for org B's webhook), secret returned once, SSRF refusal on create and on redirect, dispatcher signature and backoff, edge route tests (auth, org scope, ZDR refusal, closed event list), openapi parity test.
- Retry: control test that a retry job copies only retryable failed URLs, is idempotent on Idempotency-Key, links parent_job_id, refuses a non-terminal job; edge test for 404 foreign, 402, 429; ZDR refusal test.
- Export: NDJSON test for ordering, after_seq paging and the size cap.

## Risks
- Cross-tenant webhook delivery (existing, severe): fanoutWebhooks has no org filter and webhooks have no org column. Today the only way to create a webhook is the HMAC-signed internal control API (not tenant-exposed), so it is a latent bug, not a live leak; it becomes real the moment a tenant-facing route or a second tenant's webhook exists. Fix org scoping BEFORE exposing any webhook API (INNH-5 and INNH-13b).
- Webhook secrets are stored and listed in clear (store.Webhook.Secret, JSON field `secret`).
- ZDR: event payloads and job errors carry URLs; ZDR orgs cannot hold durable jobs (control_records refused), so webhooks for them must be refused, not degraded. Retry must not copy URL lists across a ZDR boundary.
- Cost and abuse: retry of a large failed set is a new billable job and needs admission/quota (job_admission), idempotency keys, and a cap per parent.
- Correctness: a terminal job stays terminal (markJobTerminalIfNeeded refuses to move it back, resources.go:205), so "resurrect" must create a new job, never reopen the old one.
- Abort: forced cancel after the 120 s grace terminates the workflow without cleanup, which can leave partial artifacts and unreleased browser slots; timeout and cancel precedence untested live.

## Commits
(none yet)

## Tests (counts)
(none yet)

## Not verified
Code reading only: Docker is off, nothing was run, and the failing orchestrator/control tests were taken from W2-closeout.md, not re-run.
