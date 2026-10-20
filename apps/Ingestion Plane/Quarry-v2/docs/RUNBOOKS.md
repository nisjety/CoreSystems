# Quarry v2 — Runbooks

Operational playbooks for common Quarry v2 incidents and recovery flows.

## Driver failure cascade

**Symptom:** scrape requests returning `502 DriverFailed` or `504 Timeout`.

1. Check `quarry.events.page_failed` aggregate stream for clustering by driver:
   ```bash
   nats sub "quarry.events.page_failed" --count 50
   ```
2. Inspect `DriverInfo.kind` in failed events. If one driver dominates:
   - **Static**: target site likely Cloudflare-walled. Check site's HTTP response.
   - **TLS**: BoringSSL handshake failing — usually transient, otherwise check `quarry-tls` logs for `ja3 mismatch`.
   - **Browserless / Browserbase / Kernel**: cloud provider outage. Check provider status page.
3. **Mitigate**: Pause schedule via Control Plane API, then route via fallback chain:
   ```bash
   curl -X POST http://control:8081/v1/schedules/<id>/pause
   ```
4. **Recover**: When provider is healthy, resume schedule:
   ```bash
   curl -X POST http://control:8081/v1/schedules/<id>/resume
   ```

## Crawl stuck / no progress

**Symptom:** Temporal workflow `CrawlJobWF` running but `quarry.run.<id>.page_fetched` events stop arriving.

1. Check `CrawlSignal` state via orchestrator:
   ```bash
   curl http://control:8081/v1/runs/<run_id>/signal
   # Expect: "running" | "paused" | "cancelled"
   ```
2. If paused unexpectedly, resume:
   ```bash
   curl -X POST http://control:8081/v1/runs/<run_id>/resume
   ```
3. If cancelled but workflow shows running, check for orphaned activities in Temporal UI.
4. **Last resort**: cancel + restart from checkpoint. The Rust frontier serializes state on every page (`FrontierCheckpoint`); the orchestrator persists this to Temporal so you can resume from the last good state without re-fetching.

## NATS event lag

**Symptom:** Convex / Model Plane consumers reporting stale Quarry data.

1. Check stream pending count:
   ```bash
   nats stream info QUARRY_EVENTS
   ```
2. If pending count is high:
   - Check disk space on the JetStream node (events flushed to disk).
   - Check consumer lag: `nats consumer ls QUARRY_EVENTS`.
3. **Mitigate**: Reduce ingest rate or scale consumers.

## Model Plane budget abort

**Symptom:** Agent runs ending with `MaxCostExceeded { spent_usd, limit_usd }`.

This is by design — the AgentLoop enforces `max_cost_usd` and aborts gracefully.

1. Inspect `agent.failed` event to see the spent vs limit ratio.
2. If aborts are dominant, either:
   - Bump `max_cost_usd` in the request constraints.
   - Lower `max_steps` so the loop terminates earlier.
3. Track total spend by querying the Model Plane cost ledger (cost-core).

## Cache thrash / poor hit rate

**Symptom:** Redis hit rate < 30% on previously-warm content.

1. Verify `cache_admission` config: requests with `noCache: true` or ZDR=on are correctly skipping cache.
2. Check fingerprint stability — `quarry-eval` scoreboard should show `fingerprint_stable: true` for static fixtures.
3. Inspect Redis memory: `redis-cli info memory`.

## Browser grant expiry / unauthorized

**Symptom:** `agent.failed` events with code `Forbidden` and message containing `grant inactive`.

1. Verify Model Plane browser-broker is reachable:
   ```bash
   grpcurl -plaintext browser-broker:9095 grpc.health.v1.Health/Check
   ```
2. Check grant TTL — the broker default is 15 minutes (`BROWSER_BROKER_GRANT_TTL_SECONDS`, clamped to 30–3600 s). A run that outlives its grant needs a new grant; there is no renewal yet (Quarry has no endpoint to swap a live run's grant id). The gateway reports an expired or revoked session grant as `409 browser_session_grant_inactive`; the SPA shows end-user copy for it since `8b6a71df`.
3. If the broker is down, Quarry falls back to `NoopGrantValidator` only when no validator URL is configured (dev mode) — production should NEVER leave `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` empty. (Corrected 2026-10-16: the old text named `BROWSER_BROKER_GRPC`, which no code reads.)
4. Since `7c5a564b` (WS1.C) the broker keeps non-ZDR grants in Postgres and refuses to start without `DATABASE_URL`, unless `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT` is exactly `true`; ZDR grants (`z_` ids) stay in memory and do not survive a restart. Since `1c5f000a` `apps/Model Plane/deploy/docker-compose.yml` sets `DATABASE_URL` for `browser-broker` to `postgresql://browser_broker:${BROWSER_BROKER_PG_PASSWORD}@ingestion-postgres:5432/browser_broker`: a dedicated database and role of Quarry's Postgres server, reached over the `browser-broker-db` network that the Ingestion compose owns. `apps/Ingestion Plane/scripts/bootstrap_broker_db.sh` creates the role and database and is safe to repeat (`--check` changes nothing; `--mint-if-missing` writes a password into `apps/Model Plane/deploy/.env` only when the key is empty). `BROWSER_BROKER_PG_PASSWORD` is required (`:?`), so every Model Plane compose command fails until the key is in `deploy/.env`. **None of this has been applied to the live stack**: no role, database or network exists there and nothing has rebuilt or restarted the broker; a rebuilt broker image exits 1 until the deploy steps are done (they are in the side-session note `wf-U1-broker-db-deploy.md`, for the owner to approve). The server is shared by two planes and separated by database and role only: `ingestion_user`, the superuser all Quarry services connect as, can read and write `browser_grants`.

## ZDR violation alerts

**Symptom:** `Forbidden` errors with `ZDR=on rejects ingest` in logs.

This is the safety net firing — investigate where ZDR-on requests are being routed to durable ingest:

1. Find the offending caller in logs (`run_id`, `org_id`).
2. Check that the upstream is correctly threading `zdr: true` through the contract chain.
3. Audit the ZDR field on the request: `AgentActionRequest.zdr`, `DataPlaneIngestRequest.zdr`, `StructuredExtractRequest.zdr`.

## ZDR floor from the token (user principals) — 403 on durable crawl / batch

**Symptom:** `POST /v1/crawl` or `/v1/batch` answers `403` with `ZDR active: durable crawl jobs are not available under zero data retention`, and `quarry-edge` logs `durable job refused: zdr active and the durable path has no zdr support`.

Expected, not a fault. A USER token whose `zdr` claim is `true` (the org has ZDR on) is a floor under every ZDR decision, whatever the request body says (`Claims::zdr_floor` in `auth.rs`), and the durable job path cannot honour ZDR yet (the orchestrator does not forward the posture to `/v1/internal/run_page`), so it is refused rather than run as non-ZDR. The same floor makes search, answer, scrape, extract, agent runs and audio for that user run as ZDR: no paid search providers, no cache read or write, no durable page or search events.

1. Decode the token payload (never log the token): `principal_type` must be `user` and `zdr` `true` for the floor to apply. A SERVICE token (model-gateway, execution-core) carries `zdr: true` from its registry row for every org and is deliberately NOT read: its run's posture is the body `zdr` flag.
2. A user who should not be on ZDR is an org setting (org-core `interactiveRetention`, plan entitlement): fix it there, the claim follows on the next token mint (up to the token lifetime).
3. There is no switch in Quarry to turn the floor off. A token without the claim adds no floor, so rolling Quarry back restores the old behaviour exactly.

## Data Plane ingest 5xx storm

**Symptom:** Many `page_fetched` events but no corresponding `store_record_written`.

1. Check Data Plane `documents-api-go` health:
   ```bash
   curl http://documents-api-go:9001/health
   ```
2. Inspect IngestClient errors — they're logged at `WARN` level with the run_id.
3. Quarry is non-blocking on ingest: scrape succeeds even if ingest fails, so user-facing impact is delayed search/retrieval, not failed scrapes. Backfill ingest from the Data Plane side once the service recovers.

## Profile capture / restore broken

**Symptom:** Authenticated scrapes returning login pages.

1. Verify the profile is captured:
   ```bash
   curl http://edge:8082/v1/profiles/<profile_id>
   ```
2. If 404: re-run the capture flow with the agent loop.
3. If found: check `expires_at` and `viewport.is_mobile` match the original capture context.
4. For S3-backed `S3ProfileStore`, ensure the bucket is reachable and the IAM role allows `GetObject`/`ListObjectsV2`.

## OpenTelemetry traces missing

**Symptom:** Trace IDs propagated but no spans in Tempo/Jaeger.

1. Check `OTEL_EXPORTER_OTLP_ENDPOINT` is set on all three services (edge, orchestrator, control).
2. Verify the collector is reachable: `nc -zv collector 4317`.
3. For Go services, confirm `quarryotel.Init` was called (look for `OTEL init failed; continuing without tracing` in logs).
4. For Rust edge, check `tracing_opentelemetry` is layered into the tracing subscriber.

## Recovery: scrape backlog from a queue freeze

If a queue freeze required pausing all schedules:

1. List paused schedules: `curl http://control:8081/v1/schedules?status=paused`
2. Estimate replay backlog vs SLA. If backlog > 1h, prefer "skip and resume" over full replay:
   ```bash
   curl -X POST http://control:8081/v1/schedules/<id>/resume?backfill=skip
   ```
3. Otherwise, resume with backfill:
   ```bash
   curl -X POST http://control:8081/v1/schedules/<id>/resume?backfill=missed
   ```

## Chromium will not start: sandbox, seccomp or scratch directories

Chromium keeps its own sandbox in every stack; `QUARRY_BROWSER_NO_SANDBOX` now defaults to `0`.

- **Why it works in a container:** Chromium builds its sandbox from unprivileged user and PID namespaces (`clone`, `unshare`), which Docker's default seccomp profile blocks. `quarry-edge` therefore runs with `deploy/compose/seccomp-chromium.json` (allow-by-default plus a deny-list of escape and escalation syscalls: `ptrace`, `bpf`, `keyctl`, module and time calls, and similar), on that service only.
- **Symptom `FATAL ... zygote_host_impl_linux.cc: Operation not permitted`:** the seccomp profile is not applied. Check `docker inspect quarry-edge --format '{{.HostConfig.SecurityOpt}}'`; it must name `seccomp-chromium.json`.
- **Symptom `launch directory setup failed` (production overlay):** the root filesystem is read-only. The overlay mounts `/var/lib/quarry/chromium` as a tmpfs owned by uid 65532 and points `XDG_CONFIG_HOME` and `XDG_DATA_HOME` into it. A tmpfs without `uid=65532` is unwritable for the non-root user, and Chromium aborts at start-up when its config directory (`$HOME`, read-only in the image) cannot be written.
- **Old kernel or WSL2 without user namespaces:** set `QUARRY_BROWSER_NO_SANDBOX=1` in the operator `.env` as an explicit local opt-out. The edge logs a warning at every launch and refuses to launch Chromium at all when `ENVIRONMENT` is `prod` or `production`.
- **Check:** `docker exec quarry-edge sh -c 'ps -eo args | grep -c -e "[-]-no-sandbox"'` prints `0` while a browser-rendered request succeeds.

## Scrape returns 415 UNSUPPORTED_MEDIA, or a PDF scrapes as garbage

`QUARRY_EDGE__DOC_ROUTING` controls what `/v1/scrape` does with the kind of body it fetched (detected from the first bytes, then the Content-Type):

- `off`: no detection, every body takes the HTML path (the pre-routing behaviour).
- `shadow` (default): detect and record `metadata.content_kind` and the `page_fetched` event; change nothing else.
- `skip`: also refuse images, archives, spreadsheets and other bodies with no extractor, with 415 `UNSUPPORTED_MEDIA` (`error.details.kind`).
- `on`: also route PDF and DOCX to their text extractors. A file that cannot be read is a 415 with `details.reason` `pdf_parse_failed`, `docx_parse_failed` or `too_large` (a child stopped by its limits also carries `details.limit` `parser_stopped`, see Document parser isolation); a PDF with no text layer (a scan) returns `warnings: [document_has_no_text_layer]` and is not ingested (OCR is not applied).

An unknown value stops the edge at boot naming the variable. A page that suddenly fails with 415 after a mode change is usually an image or archive link in a crawl: it counts as a failed page, not a failed crawl. Roll back with `QUARRY_EDGE__DOC_ROUTING=shadow` (or `off`); no deploy of a new image is needed.

## Metrics

Prometheus metrics are off unless `QUARRY_EDGE__METRICS_ENABLED=1` (rollback: `0`; the listener is not started and nothing is recorded). When on:

- They are served on `QUARRY_EDGE__METRICS_ADDR` (default `0.0.0.0:9092`), a listener separate from the public port. The public port never serves `/metrics`: 404 for a caller with a valid bearer, 401 without one (the auth layer answers first). The compose files do not publish 9092; scrape it over the compose network.
- `QUARRY_EDGE__METRICS_TOKEN` is a bearer token (constant-time compare). In production an enabled listener without a token of at least 16 characters stops the edge at boot (`metrics: QUARRY_EDGE__METRICS_TOKEN is required in production…`). Without a token (development only) the endpoint is open.
- Series today: `quarry_http_requests_total{route,method,status_class}`, `quarry_http_request_duration_seconds{route,status_class}`, `quarry_scrape_outcomes_total{outcome}`, `quarry_lease_pool_{in_flight,peak_in_flight,capacity}{pool}`, `quarry_lease_pool_acquired_total{pool}`, `quarry_build_info{version}`. `route` is a class from a fixed list (`scrape`, `agent`, `crawl`, …, `v1_other`, `other`), never the path; no URL, host, organization, user or run id is ever a label.
- Not yet: agent-run gauges and budget series (they need the run registry), usage and cache series, `quarry_edge_draining`.

Check it from a container on the same network:

```bash
curl -s -H "Authorization: Bearer $QUARRY_EDGE_METRICS_TOKEN" http://quarry-edge:9092/metrics | head
curl -s -o /dev/null -w "%{http_code}
" http://quarry-edge:8082/metrics   # 401 (404 with a user bearer)
```

To scrape it from the Control Plane Prometheus, add a job `quarry-edge` (target `quarry-edge:9092`, `authorization.credentials_file` pointing at a mounted copy of the token) to `apps/Control Plane/observability/prometheus.yml`, and mount the token file read-only. Do this only after the file exists: Prometheus refuses a configuration whose credentials file is missing.

## Agent runs hit BUDGET_EXHAUSTED (402)

An **agent** run (a service principal, or a start request with `initiated_by: "agent"`) is bounded in four ways; a **human** session is never capped:

| dimension | limit | default / ceiling |
|---|---|---|
| `steps` | completed actions | 60 / 300 |
| `attempts` | actions tried, failed or not (`steps x QUARRY_EDGE__BUDGET_ATTEMPT_FACTOR`, default 2) | 120 |
| `runtime` | seconds under agent control; the clock stops during a human takeover | 900 s / 3600 s |
| `cost` | estimated USD from the flat action table plus a browser-time term, refused before the overshoot | 1.00 / 20.00 |

A request with no limit (or 0) takes the default, never "unlimited"; a request above the ceiling is clamped. `POST /v1/agent/runs` returns the effective `constraints` and what was `clamped`. The numbers are placeholders pending the ratified budget numbers; they come from `QUARRY_EDGE__AGENT_DEFAULT_MAX_{STEPS,RUNTIME_S,COST_USD}`, `QUARRY_EDGE__AGENT_CEILING_MAX_{STEPS,RUNTIME_S,COST_USD}`, `QUARRY_EDGE__BUDGET_ATTEMPT_FACTOR` and `QUARRY_EDGE__BUDGET_BROWSER_SECOND_USD`. A value that is not a positive number, or a default above its ceiling, stops the edge at boot naming the variable.

`QUARRY_EDGE__BUDGET_ENFORCEMENT` is `off` (no accounting), `shadow` (the default: record once per run what would be refused, refuse nothing) or `on` (refuse with 402, `error.details` = `dimension, limit, spent, unit, estimated, resumable:false`). Roll out by reading `quarry_agent_budget_exhausted_total{mode="shadow"}` for at least a day first: a `class="interactive"` series means a person was misclassified and must be fixed before `on`. Roll back with `QUARRY_EDGE__BUDGET_ENFORCEMENT=off` (or `shadow`); no data migration, the ledger is an optional field of the run checkpoint.

An exhausted run stays exhausted: repeating the call returns the same 402, and a resume (`resume_run_id`) restores the spent steps, attempts, cost and active time, so neither refills the budget. A run that must continue needs a new run.

Known gaps: the model-plane agent loop does not yet map a 402 to a resource-limit stop (it reports the step as failed); the `agent.budget_exhausted` event, a `meta.budget` block on step responses and per-organization allowances are not built.

## One organization is holding the browser pool (`org_render_cap`)

The browser pool (2 leases for the local Chromium, 4 for Browserbase) is shared by every tenant, so one organization crawling JavaScript pages could use all of it. `QUARRY_EDGE__POOL_ORG_MAX_PERMITS=N` caps how many leases one organization holds at once (`0`, the default, is no cap). An organization at its cap waits for one of its own leases to come back, up to the fetch's wait budget, and then gets `429 RATE_LIMITED` with `error.details = {reason: "org_render_cap", scope: "org", limit, active, retry_after_seconds}` (retryable). Running out of time on the whole pool is still a plain `TIMEOUT`, which is how to tell the two apart.

Leave it off for a single-tenant deployment: any cap below the pool size only slows that tenant. For a shared edge a value of half the pool (`1` for the local Chromium) keeps a second tenant working while the first is at its cap. The permits are counted per process (one edge container today). Not built yet: plan-aware per-organization limits and the cap on concurrent agent runs.

## Job admission: one organization's backlog

Before admission the orchestrator's dispatcher read the newest 50 jobs across every organization and started all of them. One organization submitting many crawls could fill that window, and nothing bounded how much a tenant could queue.

With `QUARRY_JOB_ADMISSION=on` (control; default `off`):

- **Per-organization cap.** An organization has at most `QUARRY_JOB_CAP_DEFAULT` (3) jobs running or leased-but-not-yet-started. A job may carry its own `params.admission.cap`; a value that is not a positive number falls back to the default. The rest wait in `status: accepted`, oldest first.
- **Claim, not a window.** The dispatcher calls `POST /v1/internal/jobs/claim {limit}` (HMAC like every control route). Control leases the jobs it hands out for `QUARRY_JOB_CLAIM_LEASE` (60 s), round-robin across organizations (every organization's first job, then every second, …), so a tenant with sixty queued jobs does not delay another tenant's one. A job whose lease runs out without being started is claimable again. While admission is off the endpoint is a 404 and the dispatcher lists as it always did, so control and orchestrator can be deployed in either order.
- **Bounded backlog.** A crawl or batch submission is refused with `429 RATE_LIMITED`, `details {reason: "org_backlog_full", limit, queued, retry_after_seconds}` and `Retry-After` once `QUARRY_JOB_BACKLOG_DEFAULT` (200; or `params.admission.backlog_cap`) jobs are already waiting. The refusal is checked before anything is stored, so it leaves no job, and it is not billed. The gateway turns it into `crawl_backlog_full` (nb and en copy) with `retryAfterSeconds`.
- **Backlog TTL.** A job still waiting after `QUARRY_JOB_BACKLOG_TTL` (24 h) is failed with `params.failure_reason = "backlog_expired"` and a terminal `run.failed` event, so a client streaming it learns it ended. A job leased to the dispatcher is never expired.

Roll out: deploy control, then the orchestrator and edge, then set `QUARRY_JOB_ADMISSION=on`; roll back by setting it `off` (no data change; `claimed_until` is only read while admission is on). Migration 019 adds `jobs.claimed_until` and an index.

Limits to know: the caps are per job row, taken from `params.admission` if present and otherwise the control defaults, so a plan-dependent cap needs the edge to stamp it at submit time (not built; no plan data is read yet). The claim statement is one query without row locking, so run a single dispatcher; two concurrent claimers can each lease up to a batch past a cap. A job whose workflow was started but whose "mark running" write was lost comes back after its lease and is marked running then.

## Stopping the edge (drain)

On SIGTERM (`docker stop`) the edge no longer dies with its browsers open:

1. **Out of rotation.** `/ready` answers 503 `{"status": "draining"}` while `/health` stays 200. A request to start an agent run, a crawl or a batch gets `503 RUNTIME_NOT_READY` with `details {reason: "draining", retry_after_seconds: 5}` and `retryable: true`. Steps on runs that are already live keep working.
2. **In-flight work finishes**, for at most `QUARRY_EDGE__DRAIN_DEADLINE_S` (default 30, 1 to 600; anything else stops the edge at boot naming the variable). axum's graceful shutdown waits for every open connection and an SSE stream or WebSocket never ends by itself, so after the deadline the remaining connections are closed (`drain: deadline reached with connections still open` in the log).
3. **Sessions are released**: each live run's browser session is released (8 at a time, 5 s each, a hung one cannot hold up the rest) and `drain: browser sessions released` logs how many succeeded, failed or were still held by a request. A non-ZDR run's checkpoint stays `Active`, so after the restart `POST /v1/agent/runs` with its `resume_run_id` continues it; a ZDR run has no checkpoint and is gone.

The compose file sets `stop_grace_period: 45s`; keep the deadline well under it (deadline + about 10 s of release time). If `docker stop` still ends in exit 137, the deadline is too close to the grace period or Chromium is hung: lower the deadline.

Not done: an `agent.expired` event with `reason: shutdown` on the run's stream (there is no such event type yet), a usage segment for the interrupted run (the per-run usage metering is not built), and the orchestrator's own worker stop timeout.

## Starting the edge on the production overlay (boot requirements)

`docker compose -f docker-compose.yml -f docker-compose.production.yml up quarry-edge` sets `ENVIRONMENT=production`, and in production the edge refuses to start (or to report ready) unless every one of these holds. The overlay supplies them; this is the list to check when it bails:

- **A durable artifact store.** `QUARRY_EDGE__ARTIFACT_BACKEND` is `filesystem` (the overlay default, a named volume `quarry-edge-artifacts` at `/app/data/artifacts`) or `s3` (set `QUARRY_EDGE_ARTIFACT_BACKEND=s3` and `QUARRY_EDGE__S3_BUCKET`). `memory` stops the edge: `durable runtime is required but QUARRY_EDGE__ARTIFACT_BACKEND is in-memory`. The filesystem backend is single-replica storage; use `s3` where more than one edge runs. The image creates `/app/data/artifacts` owned by uid 65532 so the volume is writable; a volume created before that image existed keeps its old root ownership and answers every artifact write with `500 Permission denied` while `/ready` is still 200 (fix: `docker run --rm -v <project>_quarry-edge-artifacts:/v busybox chown 65532:65532 /v`, or remove the empty volume).
- **A database.** `QUARRY_EDGE__DATABASE_URL` set and reachable, with `QUARRY_EDGE__PROFILE_STORE_KIND=postgres` (the base default). The profile, step-receipt and queue stores each log a warning and fall back to memory if the connect fails, which the durability gate then turns into `durable runtime could not be established`.
- **Durable event history.** `QUARRY_EDGE__DURABLE_EVENT_HISTORY=1` (set by the overlay).
- **A browser-grant validator.** `QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL` (required by the overlay), the BrowserBroker gRPC address on `inter-plane-bus`, e.g. `http://browser-broker:9095`. The edge connects to it at boot, so the broker must be up first.
- `QUARRY_EDGE__INTERNAL_SECRET`, and neither `QUARRY_EDGE_AUTH_DEV_BYPASS` nor `QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS` enabled.

Check: `curl -s http://127.0.0.1:8082/ready` is `{"durable":true,"status":"ready"}` (from inside the container; the overlay publishes no port) and `docker logs quarry-edge` has no `durable runtime could not be established`.

## Run caps and the run sweeper

Every agent run and every person's browser session holds a Chromium context. Two things bound them (`crates/quarry-edge/src/run_limits.rs`, read once at boot; a value that cannot be parsed stops the edge naming the variable).

**Caps** (`QUARRY_EDGE__` prefix; `0` disables one). A start over a cap is `429 RATE_LIMITED` with `details.reason` `global_run_cap`, `org_run_cap` or `org_interactive_run_cap` (`docs/ERROR_REASONS.md`); the gateway shows it as `browser_capacity_reached` with `Retry-After`. `MAX_CONCURRENT_RUNS` 32, `MAX_CONCURRENT_RUNS_PER_ORG` 8, `MAX_INTERACTIVE_RUNS_PER_ORG` 6 (a sub-cap, so open tabs cannot crowd out an organization's agent runs), `RUN_CAP_RETRY_AFTER_S` 10. Chromium memory was measured on 2026-10-17 (headless Chromium in a `rust:1` container, one tab per session on a synthetic page of 3000 DOM nodes and a 50 000-item script array; PSS, so shared pages count once): 1 session 506 MiB, 8 sessions 1092 MiB, 16 sessions 1741 MiB, 32 sessions 3042 MiB, about 81 MiB for each added session on top of roughly 430 MiB of base. Real sites cost several times the synthetic page, so size a host at 2 to 4 times these figures (6 to 12 GiB for 32 sessions) and re-run `measure_memory_per_open_session` (an `#[ignore]` test in `quarry-browser`, see its doc comment) on the target machine before raising a cap. The boot line `run limits: global=... org=... interactive=... sweeper=... dry_run=...` shows what is in force.

**The sweeper** releases the browser of a run nobody is using, every `RUN_SWEEP_INTERVAL_S` (15):

- an agent run after `AGENT_RUN_IDLE_TTL_S` (300) with no activity, or past its absolute lifetime (its own `max_runtime_s` plus `AGENT_RUN_TTL_GRACE_S` 30, else `AGENT_RUN_MAX_TTL_S` 1800, never above 3600);
- a person's session after `INTERACTIVE_RUN_IDLE_TTL_S` (900) idle with no viewer, `TAKEOVER_IDLE_TTL_S` (1800) while the person holds the browser, and always at `INTERACTIVE_RUN_MAX_TTL_S` (14400), viewer or not. The 120 s the gateway puts on a UI session is not a lifetime;
- a run with an action in flight is never released.

An expired run emits `agent.expired` (`reason`, `idle_s`, `age_s`; no URL). A non-ZDR run keeps its checkpoint `active` and can be resumed with `resume_run_id`; a ZDR run leaves nothing. `DELETE` and the sweeper end a run through the same path, so the browser is released even if another request still holds the run.

**Rollout.** The compose file sets `QUARRY_EDGE__RUN_SWEEPER_DRY_RUN` from the host variable `QUARRY_EDGE_RUN_SWEEPER_DRY_RUN`, default `1`, so the sweeper starts in dry run and only logs `would_expire` (run id, class, reason, numbers). Read it for at least a day, check that no live person's session is on it, then set `QUARRY_EDGE_RUN_SWEEPER_DRY_RUN=0`. Rollback with no image change: `QUARRY_EDGE__RUN_SWEEPER=0` and `...MAX_CONCURRENT_RUNS*=0`. The browser pool's own reaper, `QUARRY_BROWSER_SESSION_IDLE_S` (default 3600) and `QUARRY_BROWSER_SESSION_MAX_AGE_S` (default 21600) in `crates/quarry-browser/src/slots.rs`, must stay above every run limit. It only logs what it would reap unless `QUARRY_BROWSER_REAPER=1` (default `log`). The edge's boot check compares the run limits with 3600 s and 21600 s, which it takes as constants: it does not read the values in force, so a changed driver setting goes unchecked.

The edge sets `Retry-After` itself on a 429 or 503 whose body carries `retry_after_seconds` (`crates/quarry-edge/src/retry_after.rs`). A step or tab call on a run the sweeper released answers 404 with `details.reason = run_not_live`; when the caller owns a non-ZDR, still-active checkpoint it adds `resumable: true` and `resume_run_id` (anyone else, and every ZDR run, gets the plain 404).

Known gap: the lifecycle timeline item does not name `system` as the initiator.

## Browser viewport and page hygiene

A lease's viewport is applied to each page when it opens, never to the browser: one Chromium serves every session, and a lease that asks for another size no longer closes it under the others (it used to relaunch the browser and drop every other session's tabs). A lease with no viewport gets `QUARRY_BROWSER_DEFAULT_VIEWPORT` (`WxH`, default `1280x800`, device scale 1, desktop). **Rollout note:** Model Plane runs used to render at 800x600, the old browser-wide default; set `QUARRY_BROWSER_DEFAULT_VIEWPORT=800x600` to keep that until the larger default (more content per screenshot, different breakpoints, bigger PNGs) has been accepted. A mobile lease also gets touch emulation; a page of a mobile lease lays out at the device width only if it says `width=device-width`, like any mobile page.

Each page is also focused and marked active (`Emulation.setFocusEmulationEnabled`, `Page.setWebLifecycleState`) so a page in a background context does not stall. `QUARRY_BROWSER_PAGE_HYGIENE=off` skips those two calls if one misbehaves on a Chromium update; the size is always applied, and a failure to apply it fails the tab open with `details.reason = viewport_emulation_failed`. The log line `browser slot launched` (with `generation`) appears once per browser launch, so more than one per process start means the browser died and was replaced.

## Browser operation deadlines

Every call the driver makes to Chromium has an upper bound from one table (`crates/quarry-browser/src/deadlines.rs`), and a call that overruns it returns a typed `504 TIMEOUT` naming the operation (`docs/ERROR_REASONS.md`) instead of holding the run lock until the router's 60 s timeout. Starting values: launch 25 s (the launch itself 20 s), tab attach 15 s, navigate 30 s, content 15 s, accessibility snapshot 10 s, screenshot 10 s, pdf 30 s, evaluate 15 s, input 5 s, native action 10 s, one wait-for poll 2 s and the wait itself 30 s at most, page or context close 5 s each, profile capture 5 s, download start 10 s and finish 30 s. `QUARRY_BROWSER_DEADLINE_SCALE` (a number in (0, 2], default 1) multiplies all of them; a value outside the range is ignored with a warning.

Collect a week of the `browser_op_duration_ms` debug lines (fields `op`, `ms`) before tightening any number. The two read paths (page content, and the accessibility snapshot behind native projection) retry once after 100 ms when Chromium reports that a navigation destroyed their execution context; nothing that changes the page is ever retried by the driver. Calls that are not in the table (the CDP event collectors, the network guard) are bounded only by chromiumoxide's own request timeout, set to 40 s. That is above the largest unscaled deadline (30 s), so a typed timeout wins first while `QUARRY_BROWSER_DEADLINE_SCALE` is 1.33 or less; above that, a 30 s operation can outlast 40 s and chromiumoxide's error arrives first.

A tab attach does not hold the driver's browser lock while it runs (WS3.F; see "Browser capacity, crashes, the idle reaper and `/ready`"); the `tab_attach` deadline bounds the attach itself.

## Document parser isolation

With `QUARRY_EDGE__DOC_ROUTING=on` a PDF or DOCX is parsed for its text. `lopdf` inflates a compressed stream with no output cap, so a few megabytes of PDF can ask for gigabytes; a permit and a deadline cannot stop an allocation. `QUARRY_EDGE__DOC_PARSE_ISOLATION` chooses where the parser runs:

- `process` (the default on Unix when the running binary is `quarry-edge-rs`): each document is parsed in a child process, the edge binary re-executed as `quarry-edge-rs --doc-parse <pdf|docx>`, with `RLIMIT_AS` 768 MiB, `RLIMIT_CPU` 20 s, `RLIMIT_NOFILE` 16 and no core files. The document goes to the child on stdin and one JSON line comes back; a child that is killed by a limit, crashes or answers nothing usable is a 415 `UNSUPPORTED_MEDIA` with `details.reason` `pdf_parse_failed` (or `docx_parse_failed`) and `details.limit` `parser_stopped`; one that outlives the 15 s deadline is killed and answers 504. Each parse starts a process; that cost has not been measured.
- `inprocess`: the previous behaviour, on the blocking pool, bounded only by the permit count (2), the deadline and the input caps. The rollback.

The boot log line `document parser isolation` (field `isolation`) says which is in force. On a build that is not Unix the mode is `inprocess`, and nothing is logged for that choice; when the executable path cannot be found it falls back to `inprocess` with a warning. The boot line names the mode in force either way. `DOC_ROUTING=on` is only advisable in a multi-tenant deployment with `process`.

## Browser capacity, crashes, the idle reaper and `/ready` (WS3.F)

The local Chromium driver keeps a slot book (`crates/quarry-browser/src/slots.rs`): which session lives in which browser process, how many pages each process holds, and when each session was last used. One process today (K = 1). Settings are read once when the driver is built; a value out of range is clamped, text that is not a number falls back to the default with a warning.

| variable | default | range | what it does |
|---|---|---|---|
| `QUARRY_BROWSER_MAX_SESSIONS` | 40 | 0..4096, 0 = no cap | sessions the driver holds; one more is `429 browser_session_cap` |
| `QUARRY_BROWSER_MAX_PAGES_PER_SLOT` | 24 | 0..4096, 0 = no cap | pages per browser process; one more tab is `429 browser_page_cap` |
| `QUARRY_BROWSER_SESSION_IDLE_S` | 3600 | 30 s..7 d | a session unused this long is reaped (never before twice its lease TTL) |
| `QUARRY_BROWSER_SESSION_MAX_AGE_S` | 21600 | 60 s..30 d, never below the idle value | a session older than this is reaped |
| `QUARRY_BROWSER_REAPER_INTERVAL_S` | 30 | 1..3600 | how often the reaper looks |
| `QUARRY_BROWSER_PROBE_INTERVAL_S` | 15 | 1..3600 | how often the health probe runs |
| `QUARRY_BROWSER_REAPER` | `log` | `0`, `log`, `1` | `log`: only log `browser session would be reaped`; `1`: close them; `0`: no reaper (the prober, the caps and crash handling stay) |

Keep both reaper limits above every run limit in "Run caps and the run sweeper": the edge's run sweeper is the one that ends runs, the driver's reaper is the backstop for sessions nobody released. Run `log` for a week and check that no line names a session that was still in use before setting `QUARRY_BROWSER_REAPER=1`. A reaped session's next call is `502 session_reaped` (`docs/ERROR_REASONS.md`). In `log` (and `0`) mode a session that a caller acquired and never released keeps its place under `QUARRY_BROWSER_MAX_SESSIONS` until the edge restarts; `sessions_open` in `/ready` counts them. An `acquire` that is itself dropped (a scrape timing out during the cold launch) gives its place back, unless a second `acquire` of the same session joined it: the place then goes back only when no `acquire` of that admission finished and none is still running.

**Crash.** When Chromium exits (killed, crashed, out of memory) the driver logs `browser process lost` with `slot`, `generation` and `sessions`; those sessions' next calls are `502 browser_process_lost`, and the next `acquire` or the prober (which runs whatever `QUARRY_BROWSER_REAPER` says) launches a new browser, logged as `browser slot launched` with the next `generation`. No edge restart is needed. A retired browser is killed, given 5 s to exit, and its profile and crash-dump directories under `QUARRY_BROWSER_PROFILE_ROOT` are removed; on the first launch the driver also removes `edge-<pid>-*` directories of processes that no longer exist (Linux only; directories of the edge's own pid are left alone, so a container that always runs as pid 1 does not sweep a previous container's leftovers on a shared volume).

**Probe and `/ready`.** Every probe interval the driver opens `about:blank` in a throwaway browser context and closes it again (deadline `health_probe`, 3 s). With a pinned egress proxy the probe context gets a deny-all policy before its proxy endpoint, like any session. Two failed probes in a row mark the browser unhealthy (`browser slot unhealthy`); an unhealthy browser that holds sessions is never killed, an empty one is recycled (`browser slot recycled`). `GET /ready` carries a `browser` object (`state` `idle`, `healthy`, `degraded` or `unhealthy`, `max_sessions`, `sessions_open`, `slots[]` with `status`, `generation`, `sessions`, `pages`, `healthy`, `probe_failures`) and answers 503 `browser_unhealthy` when, for either driver, every launched browser is unhealthy or dead. The edge runs two local Chromium drivers (agent runs, and the scrape path), so `/ready` carries a second object, `scrape_browser`, with the same shape; `browser` is the agent driver's. Each object is present only when that driver runs a local browser (a remote provider leaves the body as `{"durable":true,"status":"ready"}`). `docs/openapi.yaml` documents both bodies (200 and 503), and `tests/ready_browser.rs` checks the real bodies against those schemas. A browser that was never launched is ready. `/health` and the Docker `HEALTHCHECK` stay on the static liveness answer. Check with `curl -s localhost:8082/ready | jq '{browser, scrape_browser}'`.

**Boot check R-26 (warn only).** At boot the edge logs `browser capacity: QUARRY_BROWSER_MAX_SESSIONS (n) must exceed the concurrent-run cap (m) plus the heavy lane (h); at k concurrent contexts the driver cap answers first` when the session cap does not exceed `QUARRY_EDGE__MAX_CONCURRENT_RUNS` plus the heavy-lane budget (`slots::HEAVY_LANE_BUDGET`, 1 until the heavy lane exists), and a different line when the run cap is 0 (unlimited) while the session cap is not. It only warns: nothing stops the edge. The plan's rule is to make it a boot failure once the Chromium memory per context is recorded; the figures in "Run caps and the run sweeper" were measured on a synthetic page and nobody has made that decision. The check runs only in a build with the `browser-agent` feature.

**Gateway mapping.** The Verevon gateway maps `browser_session_cap` to `429 browser_capacity_reached`, `browser_page_cap` to `429 browser_tab_limit_reached` (both with `Retry-After`), and `session_reaped` and `browser_process_lost` to `410 browser_session_reaped` and `410 browser_process_lost` (`62f1b66a`). The SPA has Norwegian and English copy only for `browser_capacity_reached`; the other three show the gateway's message until `src/shared/i18n/errors.ts` gets entries. The gateway does not read `/ready`, so `browser_unhealthy` needs no mapping.

**A tab attach does not block the other sessions.** The driver's browser handle is a read/write lock: every CDP call (create context, set download behaviour, `new_page`, dispose, download listeners) takes the read side, so sessions attach in parallel, and only a launch, a relaunch and a kill take the write side. The prober skips one tick when sessions are attaching. An attach that is cancelled or times out closes the page and context it created. (Until `7420d3a6` an attach held a mutex across `new_page`, which took up to 4.2 s under the parallel live suite and made two live tests flaky.)

**Page and frame ownership.** A page and a live frame belong to the session admission (the slot book entry) that created them. A key that is released and admitted again is a new session: a late close of the old session's page does not give back a page of the new one (`SlotBook::page_closed(key, admission_id)`), and a late capture or navigation of the old session cannot read, fill or reset the new session's live-frame cache, or put a frame back after the release. The driver wiring is covered by live-gated tests (`a_late_goto_of_a_released_session_leaves_the_next_sessions_frame_alone`, `close_tab_gives_the_page_back_to_the_entry_that_opened_it`) that run only in the `quarry-browser-live` CI job, which has no `continue-on-error` since `bf2e7882` (see "Live Chromium CI lane: no `continue-on-error`, not yet a required check" below); adding it to the branch protection rule is a repository setting.

**A cancelled scrape fetch.** A scrape fetch that is cancelled (the HTTP request was dropped, a caller's timeout) releases its browser session before its pool slot comes back: the release runs on its own task, so dropping the fetch cannot abort it, and a release that fails or panics poisons the lease so the slot is not reused (`HeldSession` in `browser_driver.rs`; `0b86c8c6`, `86b546e6`, `e78b562c`).

## Scrape cleaning cost

Measured on 2026-10-18 with `cargo test -p quarry-transform --test clean_html_cost -- --ignored --nocapture` (`crates/quarry-transform/tests/clean_html_cost.rs`). The test builds a 5,243,097-byte page: a navigation bar of 200 links, a share bar, an article of paragraphs with a link in each, a related box every 50 paragraphs, an aside, a footer, and an inline script and style. Each option ran three times, and the test prints each run. The table gives the median and the minimum of `to_markdown` alone, in milliseconds.

| `to_markdown` options | median ms | min ms | markdown bytes |
| --- | --- | --- | --- |
| defaults (readability) | 4963.5 | 4626.8 | 4396474 |
| `exclude_tags` `.related`, `aside` | 6321.1 | 6257.8 | 4384768 |
| `include_tags` `article` | 4865.5 | 4775.2 | 4396474 |
| `only_main_content` false (`clean_full`) | 4930.8 | 4810.8 | 4401924 |

**Machine caveat.** These are debug (test-profile) builds, not a release build, so they are not a production figure, and no release run was made. The run was one pass on one Windows host (Intel i7-13700H, 20 logical processors, Docker Desktop, one `quarry-rust-env:1` container) that was shared with other work. The spread between the three runs is the only noise estimate, so read the numbers as an order of magnitude and compare the options with each other. The `exclude_tags` row costs more than the defaults because the exclusion path does one more scraper parse and a serialization before the readability pass, while the default path runs readability on the input string. That is read from the code path; it was not profiled.

## Page cache bypass and the Redis-gated cache tests (WS5.D)

The page cache (`quarry:page:{org}:{fingerprint}`) holds only the result of a default scrape. A request with options other than the defaults (`formats`, `include_tags`, `exclude_tags`, an `extractor` other than `readability`, `only_main_content` false, `remove_base64_images`), or whose `signals` select the browser driver (any `actions`, `screenshot`, `pdf`, `prior_block_signals` of 2 or more, or `profile_required`), neither reads nor writes it. ZDR never does. The response's `effective_options.cache_state` says which happened: `hit`, `miss` or `bypass`.

`crates/quarry-edge/tests/cache_policy.rs` (9 cases) needs a Redis at `REDIS_URL` (default `redis://127.0.0.1:6379`) and skips each case without one. The `cargo-check` job of `quarry-v2-ci.yml` runs it, and the Redis case of `zdr_claim_floor.rs`, in the step "Page-cache and ZDR cache gates (Redis)" against a `redis:7-alpine` service published on host port 6380, with `REQUIRE_CONTRACT=1`, so there an unreachable Redis is a failure instead of a skip. The step "Test workspace" of the same job keeps the default address, where nothing listens, so its cache cases still skip. The workflow has not run on GitHub Actions (nothing was pushed); the gate was checked locally against a Redis service (`7fffb2ac`). Any other job that provides a Redis should set `REQUIRE_CONTRACT=1` for the same reason.

## History: browser lines that were ahead of the code (closed)

Three lines of "Browser capacity, crashes, the idle reaper and `/ready` (WS3.F)" (the `QUARRY_BROWSER_REAPER` row, the "Keep both reaper limits" paragraph and the "Crash" paragraph) were committed in `c2b3d2e9`, before the browser code they describe (`c8c31a26`). Between those two commits `QUARRY_BROWSER_REAPER=0` started neither the reaper nor the prober and a dropped `acquire` kept its place, so the lines were ahead of the code. Since `c8c31a26` they describe it, and nothing here needs action. The commit messages involved are listed in the build ledger's errata in `docs/plans/quarry-browser-build-plan-2026-10-14.md`.

## Evidence: a rebuilt edge image at 89c67974 (F7, 2026-10-18)

An image built from 89c67974 (`docker build -f deploy/compose/Dockerfile.edge`, standalone container on a private network, dev auth bypass, `QUARRY_EDGE__DOC_ROUTING=on`, filesystem artifact backend) was exercised with curl. What was seen:

- WS5.D: `maxPages` gives 200 with top-level `markdown`, `warnings[unknown_field]` and a `Deprecation` header; `skipTlsVerification` is 400 `reserved_field`; a ZDR scrape with `markdown` and `html` returns both inline, has no `scrape_id`, and adds no artifact (in-memory list 4 -> 4; filesystem directory 9 -> 9 files, against +4 for a non-ZDR scrape); `max_inline_bytes:50` gives `omitted:["markdown"]`; with `QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION=strict` the `maxPages` body is a 400 `unknown_field`.
- WS5.E: `include_tags:["#nope"]` gives empty markdown and the `include_tags_matched_nothing` warning on example.com and on a large article; `include_tags:["h1"]` on the article gives only the title; `exclude_tags:["p"]` halves it. Today's example.com has no `<h1>`, so `["h1"]` warns there.
- WS5.F: `GET /v1/scrape/<scrape_id>` returns the scrape's fingerprint, including from a restarted container and from a new container on the same volume; a ZDR scrape has no id. A filesystem-backed edge lists no artifacts through `GET /v1/artifacts` (that store keeps the default empty `list`), so count files, or use the in-memory backend, to check that a ZDR scrape stores nothing. (Fixed after this run, `f39d338c`: the filesystem store now lists what it stored.)
- WS5.C: the boot log says `isolation: process`; a public PDF and a local one parse; a 1 MB PDF that inflates to 1 GiB gave 504 `TIMEOUT` after 15 s (not the typed 415 of the unit test, which uses a 256 MiB limit), peak container memory 538 MiB of a 1 GiB limit, no OOM kill, no leftover child, and the next PDF parsed. (After this run `14eb0367` added an inflate budget, so that bomb is a typed 415 `too_large` in the unit and sandbox tests; the image was not rebuilt, so the 415 has not been seen on an image.) The SSRF guard has no switch for private hosts; the test server was given an address on a private docker bridge (`--subnet 11.250.0.0/24`) that the guard classes as public.
- WS3.C: three concurrent runs (1280x800, default, 390x844 mobile at scale 2) share one browser (`browser slot launched` once); a navigate on the first run after the third started returns 200; live frames are 1280, 1280 and 390 px wide, screenshot artifacts 1280, 1280 and 780 px.
- WS3.D: with `QUARRY_BROWSER_DEADLINE_SCALE=0.02`, navigating to `https://httpbin.org/delay/10` is 504 `TIMEOUT` with `details {deadline_ms:600, effect:"unknown", op:"navigate"}`; at `quarry_browser=debug` the log carries `browser_op_duration_ms` lines.
- WS2.D: with `QUARRY_EDGE__MAX_CONCURRENT_RUNS=1` the second `POST /v1/agent/runs` is 429 `RATE_LIMITED` (`reason: global_run_cap`) with `Retry-After: 10`.
- WS3.F: `/ready` carries a `browser` object (`state`, `slots`, `sessions_open`, `max_sessions`); `idle` before the first run, `healthy` with sessions open.

Traps met while doing it: a docker named volume is root-owned and the edge runs as uid 65532, so the filesystem backend answers 500 `Permission denied` until the volume is chowned; Git Bash rewrites absolute container paths unless `MSYS_NO_PATHCONV=1`; full-page markdown (`only_main_content:false`) of a large Wikipedia article is 30.9 MB from a 1.05 MB page because table lines are padded to over a million characters, so even an 8 MB `max_inline_bytes` omits it (fixed after this run, `217fc548`: tables render compact). Not run: the typed 415 of the flate bomb on the image, the Sunset header, the S3 backend.

## Challenge classifier and the one-recovery rule: `QUARRY_CHALLENGE_*` (WS3.H, WS3.J)

Two switches, both read from the edge's environment when a run (an agent run, or one scrape) starts; a change needs an edge restart. Anything but the listed values is the default. Quarry classifies what a page says about access; it never tries to get past a challenge, and neither switch changes that.

| Variable | Values | Default | Effect | Roll back |
| --- | --- | --- | --- | --- |
| `QUARRY_CHALLENGE_CLASSIFIER` | `v2`, `legacy` | `v2` | `v2`: marker-class classifier (`crates/quarry-runtime/src/challenge.rs`). Agent observations carry `challenge {strength, block_reason, recovery}`; a login page, a consent banner, a paywall, a rate limit and a bare status are `non_signal` with `requires_escalation: false`; a host that keeps challenging after its one recovery (a wait, or a reload of the same URL) is refused when `QUARRY_CHALLENGE_ENFORCE=enforce` (below). With `enforce`, a scrape that fetches a bot wall fails with `403 CHALLENGE_DETECTED` and `details.block_reason`; with the default `log` it is let through and only logged. `legacy`: the substring classifier of before WS3.H, kept for one release. It flags login pages and prose as strong escalations, keeps no episodes, refuses nothing, and a scrape is not classified at all. | Set `legacy`, restart. This also removes the scrape-path refusal. |
| `QUARRY_CHALLENGE_ENFORCE` | `log`, `enforce` | `log` | `log` (the default, the plan's setting while the corpus validates): classification and the signals stay, nothing is refused; the agent path logs `quarry.challenge.would_reject` (field `reason`) and the scrape path logs `quarry.challenge.would_refuse_scrape` (fields `reason`, `status`), and the action or the page goes through. `enforce`: an agent action on an exhausted host is refused with `403 CHALLENGE_DETECTED` (`details.recovery: "exhausted"`), and a scrape of a bot wall is refused. A typo is `log`, never `enforce`. | Unset it or set `log`, restart. This keeps the observability and removes every refusal. |

**What is refused (only with `QUARRY_CHALLENGE_ENFORCE=enforce`).** Agent path, once a host is exhausted: navigation to it from any page, and click, type, press, select, upload, download and `evaluate` on its page (the snapshot-ref, frame and semantic forms included). Still allowed: screenshots, content, pdf, every wait, scroll and mouse wheel, answering a dialog that is already open, back and forward, and navigation to another host. Back and forward are allowed because Quarry cannot tell where a history step lands; the page it lands on is judged by its own host, so a step back onto the exhausted host leaves every interaction refused. Scrape path: only a strong signal (a CAPTCHA widget on a page that is itself the challenge, a WAF interstitial, two independent block markers) in an HTML, XML or text body; a status alone, a login page, a paywall and a CAPTCHA on an ordinary form scrape as before.

**Where to look.** `quarry.challenge.strong` (info; `reason`, `strong_seen`) per strong agent observation; `quarry.challenge.scrape_refused` (info; `reason`, `status`) per refused scrape; the `page_blocked` event with `reason: "challenge_detected"` and `block_reason`. No line carries a host, a URL or page text, and a Zero Data Retention run keeps only reason counts (its episode keys are hashes, and its error leaves `details.host` out).

**A real page is refused.** Read the `reason` in the log line. If the page is not a bot wall, set `QUARRY_CHALLENGE_ENFORCE=log` (or unset it) and restart, so nothing is refused while the markers are fixed; use `legacy` only when the v2 classifier itself is wrong across the board. Marker tables and the case table are in `challenge.rs`; a missed or false marker is a new row there before it is a code change.

**What a person sees (gateway and SPA).** The gateway turns Quarry's 403 `CHALLENGE_DETECTED` into something the SPA can act on and forwards no host, URL, page text or Quarry message. Scrape preview: a 403 whose `details.block_reason` is one of `captcha_widget`, `waf_interstitial`, `unexpected_bot_error` or `challenge_loop` is `site_blocked` with `error.blockReason`; any other 403 (a `CHALLENGE_DETECTED` without a known reason, `FORBIDDEN`, `SECURITY_BLOCKED`) is `scrape_refused`; a 429 is `scrape_rate_limited`, with a `Retry-After` header and `error.retryAfterSeconds` only when `error.details.retry_after_seconds` is 1 to 3600. quarry-edge's own scrape 429 puts `retry_after_seconds` at the top level of a flat body, which the gateway does not read at HEAD, so a person gets no countdown there. The earlier proxy suggestion is gone. Browser calls: a 403 `CHALLENGE_DETECTED` is answered as `403 browser_site_blocked` with only `blockReason` (from `details.block_reason` on the scrape path, `details.reason` on the agent path) and `recovery`, each from its closed list (`docs/ERROR_REASONS.md`, "403 `CHALLENGE_DETECTED`"). The SPA chooses Norwegian or English copy by code, one sentence per block reason, and never shows the response message. `BrowserBlockedNotice` appears above the page when the current observation's `challenge.strength` is `strong` (a login page, consent banner, paywall, rate limit or weak signal shows nothing), with "Take over" and "Try another source". "Try another source" runs the action `browser.blocked.try_other_source`: it stops the AI run when the composer knows a run id for that session, then closes the session. If the stop is refused (401, 403, 5xx, or a 412 that is not the ZDR text) nothing is closed and the person sees an error. For a ZDR organization the owner answers run controls with 412 "ZDR browser-run controls are disabled...": the session is closed and the SPA says the run could not be stopped from here and may still report an error (whether closing the session ends that run is not verified live). An AI-run start from the SPA carries an `Idempotency-Key` that is kept across an ambiguous retry of the same start. The gateway also forwards `X-Agent-Run-Id` and `X-Agent-Label` as advisory `aiRunId` and `agentLabel`; Quarry ignores them at HEAD (the half that records which agent opened a tab is uncommitted work). Not done: a model-driven run's own Quarry session shows no notice (the Model Plane half), and no live bot wall has been run through any of this.

**Limits.** The classifier has been checked against hand-written fixtures (about 60 rows), not against captured pages, and no live Turnstile, hCaptcha or login page has been run through it. Treat the confidence values as unvalidated: that is why the shipped default is `log` (the plan says `log` while the corpus validates). Moving an environment to `enforce` is a deliberate step by whoever owns the rollout, after a captured corpus has passed; the deployment evidence of WS3.H and WS3.J (a 403 `CHALLENGE_DETECTED` on the second same-host navigation, a refused scrape of a wall) needs `QUARRY_CHALLENGE_ENFORCE=enforce` on the evidence stack, and the run notes name it.

### Evidence follow-up: table padding, artifact listing, flate bomb (R7)

Three defects of the rebuilt-edge evidence run, fixed in the tree (commits `217fc548`, `f39d338c`, `14eb0367`, and the R7 review-fix commits after them):

- **Markdown size.** A page with one very wide table cell no longer grows every row: tables render compact (cells trimmed, not padded; one line per row), and nested tables are no longer exponential. Check: `cargo test -p quarry-transform --test markdown_blowup -- --nocapture` prints `MEASURE` lines (html bytes, markdown bytes, ratio). **This changes the default markdown output for every caller, not only for pathological pages:** every table now renders as `| a | b |` with a `| --- | ---: |` separator and short rows are not filled (html2md padded each cell to its column's widest cell). A monitor or diff consumer that stored the markdown of a page with a table will see that page as changed once, on its next scrape, and the content of the cells is the same. Nested lists are the one known case that still grows: html2md indents every line by the list depth, so the markdown grows with the square of the depth (x12 of the input at depth 200, x24 at depth 400; a page with thousands of nested lists would be very large). It is not capped; `markdown_blowup.rs` pins the quadratic shape. `pre` blocks are copied 1:1.
- **`GET /v1/artifacts` on the filesystem backend** now lists what the store wrote (it listed nothing before). The listing reads the store's `.index`; org scoping, ordering, `limit` and `cursor` are the same code as the memory store (`list_window`). A restarted edge on the same directory lists the earlier artifacts; ZDR scrapes add nothing. Check: `cargo test -p quarry-edge --test artifacts_fs_list`.
- **Flate-bomb PDF** is answered 415 `pdf_parse_failed` with `details.reason` `too_large` (not 504). The parser child pre-scans the PDF in one forward pass (linear in the file size, whatever the number of `stream` keywords), reads each top-level stream dictionary, and inflates the `/FlateDecode` streams into nothing, refusing the document when they pass 256 MiB together before lopdf inflates anything. A stream that calls itself an image (`/Subtype /Image` as a direct entry of its dictionary) is counted against a separate 384 MiB budget, not exempted, because lopdf reads any stream as page content whatever its dictionary claims. Strings and nested dictionaries are not read for keys, so text such as `(obj)` or `(/Subtype /Image)` in a dictionary neither hides nor changes anything. A truncated stream counts the bytes it inflated before it ended. Not seen: a stream behind a filter chain (ASCII85 then Flate), an encrypted one, or one whose dictionary the pass cannot read; for those the 768 MiB address-space limit and the 15 s deadline are unchanged and still back it up (`a_bomb_behind_a_filter_chain...` pins that the limit, `limit: parser_stopped`, answers it). Check: `cargo test -p quarry-transform --lib pdf` and `cargo test -p quarry-runtime --test doc_parse_sandbox` (the default-limits bomb test runs in a few seconds). The pass reads the dictionary the way lopdf does: `%` comments are white space (a `(`, `<<` or `<` inside one, inside the dictionary or between `>>` and `stream`, changes nothing), spaces and tabs may follow `stream` before the line end, and `#xx` escapes in names are decoded (`/Flate#44ecode` is `/FlateDecode`). A review found the first version blind to all four; `the_reader_accepts_every_spelling_the_scan_must_see_through` (lopdf reads each spelling) and `a_bomb_written_in_any_readable_spelling_is_still_counted` (the pre-scan counts it) in `pdf.rs`, and `a_bomb_in_a_spelling_the_first_pre_scan_missed_is_still_a_typed_415` through the child, pin them. The test to write for any new gap is a bomb lopdf reads but the pre-scan does not count. The 384 MiB image budget is also the most a single content stream can be made to inflate by wearing `/Subtype /Image`: measured at about 5 s in a release build, so inside the 15 s deadline, but the `doc_parse_sandbox` tests run a debug child and do not exercise it. Not re-run against the real edge image yet: rebuild the image and repeat the evidence run's bomb request to close this.

## Evidence, round 2: the rebuilt edge at 1f4edbca (R9, 2026-10-19)

The edge image was built from HEAD `1f4edbca` (the Quarry-v2 tree had no uncommitted changes) with `docker build -f deploy/compose/Dockerfile.edge` and run alone in a private container (`--memory 1g`, dev bypass, `QUARRY_EDGE__ARTIFACT_BACKEND=fs` on a docker volume, process isolation for the document parser, every other knob at its default unless a row says so). A second private container served the fixtures. Everything was removed afterwards. Seven checks, each with what was seen:

| # | Check | Result | What was seen |
|---|---|---|---|
| 1 | `GET /ready` | PASS | 200, `{"browser":{...},"durable":true,"scrape_browser":{...},"status":"ready"}`; each driver object has `state` (`idle`), `max_sessions` (40), `sessions_open` (0) and one slot with `slot`, `status`, `generation`, `sessions`, `pages`, `healthy`, `probe_failures`: the shape of `docs/openapi.yaml`. With one agent run open: `browser.state` `healthy`, `sessions_open` 1. |
| 2 | Markdown size, `only_main_content:false` | PASS | List of countries by GDP (nominal): page 681,541 bytes, markdown 266,488 bytes (0.39x), longest line 10,202. Rust (programming language): page 1,055,399 bytes, markdown 389,915 bytes (0.37x), longest line 8,929; the F7 run gave 30.9 MB from 1.05 MB for the same article. |
| 3 | Filesystem artifact list | PASS | One non-ZDR scrape with `formats:[markdown,html]` took `GET /v1/artifacts` from 8 to 12 items (html, markdown, links, scrape record); a ZDR scrape of a second URL left it at 12 and returned the text inline with no `scrape_id`; after `docker restart` it listed the same 12 ids. |
| 4 | Flate-bomb PDF (1,044,218 bytes, inflates to 1 GiB) | PASS | 415 `UNSUPPORTED_MEDIA`, `details {kind: "pdf", reason: "too_large"}`, message "PDF decompressed size exceeds the 256 MiB budget", in 0.25 s (two runs). `docker stats` over 40 s: peak 18.8 MiB of the 1 GiB limit; `OOMKilled=false`, `RestartCount=0`; no leftover child in `docker top`; a normal PDF scraped 200 before and after. The reason is `too_large`, not `pdf_parse_failed` (see `docs/ERROR_REASONS.md`, 415). |
| 5 | Agent tab calls during a take-over | PASS, with a finding | With `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=on` and the dev bypass (one person identity; the agent is expressed with the `initiated_by` hint, which can only tighten): after `POST .../browser-session/control {mode: human_takeover}`, agent tab select, tab close and `step` were each 409 `USER_TOOK_OVER` with `details {control_mode: "human_takeover", control_epoch: 1}`; the person's own select was 200; agent select after the hand-back was 200. With enforcement `off` the same calls were 200 and the log had four `would_reject: agent request during human takeover` lines. A real service-principal token was not used. Finding: on `POST .../tabs` the hint is read from `initiatedBy` only; `initiated_by` in the body is silently ignored, so that agent call opened a tab during a take-over with enforcement on (200, and no `would_reject` line with enforcement off). Select and close take both spellings in the query, and `/step` takes `initiated_by`. |
| 6 | Run cap | PASS | `QUARRY_EDGE__MAX_CONCURRENT_RUNS=1`: log `run limits: global=1`; run 1 200; run 2 `429 Too Many Requests`, header `retry-after: 10`, `RATE_LIMITED`, `details {reason: "global_run_cap", scope: "global", active: 1, limit: 1, retry_after_seconds: 10}`; `/ready` stayed `ready`. |
| 7 | Challenge classification (R6) | PASS | A local page with the Cloudflare interstitial markup and one with a bare Turnstile widget, scraped with `QUARRY_CHALLENGE_ENFORCE=enforce`: 403 `CHALLENGE_DETECTED`, `details.block_reason` `waf_interstitial` and `captcha_widget`, `next: "other_source"`, and no page text in the body; a normal PDF on the same edge was 200. At the default (`log`) the interstitial scraped 200 with its text and the log carried `quarry.challenge.would_refuse_scrape` (`reason waf_interstitial`, `status 200`). Fixtures only, no live challenge page. |

Not covered: a real service-principal token (the dev bypass has one identity), the S3 artifact backend, a live challenge page. The same table with the commands is in the side-session notes, `wf-R9-evidence.md`.

**Live Chromium CI lane: no `continue-on-error`, not yet a required status check (replaces the earlier "continue-on-error" remark under "Page and frame ownership"; this heading used to say "is a required check", which no file in the repository can make true).** The `quarry-browser-live` job in `.github/workflows/quarry-v2-ci.yml` ("Live browser tests (Chromium)") no longer has `continue-on-error`; whether it is a required status check is a repository setting that nothing here verified (a repository admin has to add that check name to the branch protection rule for `main`), and the workflow only runs on PRs that touch `apps/Ingestion Plane/Quarry-v2/**` or the workflow file. It installs Chrome for Testing `155.0.8059.84` through `browser-actions/setup-chrome@v2` (bump it on purpose), exports its path as `CHROME`, sets `QUARRY_BROWSER_NO_SANDBOX=1` (hosted runners restrict unprivileged user namespaces; the edge refuses that flag when `ENVIRONMENT` is prod) and runs three sets with `--test-threads=1`: `cargo test -p quarry-browser --features chromiumoxide`, `cargo test -p quarry-runtime --features chromiumoxide --test browser_egress_boundary` (compiled out without the feature, so `cargo test --workspace` never ran it) and `cargo test -p quarry-edge --test zdr_behavior` (needs no Chromium; it already ran in the workspace job). A live test no longer skips when `CHROMIUMOXIDE_TEST=1` and there is no browser: `quarry_browser::live_gate` fails it, also when `CHROME` names a file that does not exist (chromiumoxide alone would fall back to any `chromium` on the `PATH`). To run the lane locally, set `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/path/to/chromium` and run the three commands above. Evidence: in the `quarry-rust-env:1` container (Chromium 155.0.8059.79) the three commands passed three runs in a row (135 passed + 4 ignored, 2 passed, 4 passed each time; the first run took 25 minutes including the cold build); with `CHROME=/nonexistent`, and again with no Chromium on the `PATH`, the quarry-browser and browser_egress_boundary commands failed (exit 101) and zdr_behavior still passed. Not verified: the job itself on a GitHub-hosted runner (it cannot be run here), that `setup-chrome@v2` accepts the pinned version and `install-dependencies` on `ubuntu-24.04`, and the runner's wall time and disk use with the quarry-edge build added. `actionlint` (the `rhysd/actionlint` image) reported nothing on the workflow file. Re-run on 2026-10-20 (the doc-reconcile unit U8) on an export of HEAD `137ba2ac`, same image, Chromium 155.0.8059.79: the quarry-browser set 135 passed, 0 failed, 4 ignored; `browser_egress_boundary` 2 passed; `zdr_behavior` 4 passed; `quarry_browser::live_gate` 6 passed; with `CHROME=/nonexistent` the quarry-browser set exits 101; without the feature `cargo test -p quarry-browser` is 85 passed. The five tests of `quarry-browser/tests/ssrf.rs` are all `#[ignore]` at HEAD (0 passed, 5 ignored) and belong to none of the three sets; un-ignoring four of them behind the gate is uncommitted work.
