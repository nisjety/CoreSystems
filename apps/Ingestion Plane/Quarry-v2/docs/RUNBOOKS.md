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
2. Check grant TTL — the broker default is 15 minutes (`BROWSER_BROKER_GRANT_TTL_SECONDS`, clamped to 30–3600 s). A run that outlives its grant needs a new grant; there is no renewal yet.
3. If the broker is down, Quarry falls back to `NoopGrantValidator` only when no validator URL is configured (dev mode) — production should NEVER leave `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` empty. (Corrected 2026-10-16: the old text named `BROWSER_BROKER_GRPC`, which no code reads.)

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

Known gap: a tab attach still holds the driver's browser lock for its whole length (bounded by the deadlines above); narrowing that lock is WS3.F's work.

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

Keep both reaper limits above every run limit in "Run caps and the run sweeper": the edge's run sweeper is the one that ends runs, the driver's reaper is the backstop for sessions nobody released. Run `log` for a week and check that no line names a session that was still in use before setting `QUARRY_BROWSER_REAPER=1`. A reaped session's next call is `502 session_reaped` (`docs/ERROR_REASONS.md`). In `log` (and `0`) mode a session that a caller acquired and never released keeps its place under `QUARRY_BROWSER_MAX_SESSIONS` until the edge restarts; `sessions_open` in `/ready` counts them. An `acquire` that is itself dropped (a scrape timing out during the cold launch) gives its place back.

**Crash.** When Chromium exits (killed, crashed, out of memory) the driver logs `browser process lost` with `slot`, `generation` and `sessions`; those sessions' next calls are `502 browser_process_lost`, and the next `acquire` or the prober (which runs whatever `QUARRY_BROWSER_REAPER` says) launches a new browser, logged as `browser slot launched` with the next `generation`. No edge restart is needed. A retired browser is killed, given 5 s to exit, and its profile and crash-dump directories under `QUARRY_BROWSER_PROFILE_ROOT` are removed; on the first launch the driver also removes `edge-<pid>-*` directories of processes that no longer exist (Linux only; directories of the edge's own pid are left alone, so a container that always runs as pid 1 does not sweep a previous container's leftovers on a shared volume).

**Probe and `/ready`.** Every probe interval the driver opens `about:blank` in a throwaway browser context and closes it again (deadline `health_probe`, 3 s). With a pinned egress proxy the probe context gets a deny-all policy before its proxy endpoint, like any session. Two failed probes in a row mark the browser unhealthy (`browser slot unhealthy`); an unhealthy browser that holds sessions is never killed, an empty one is recycled (`browser slot recycled`). `GET /ready` carries a `browser` object (`state` `idle`, `healthy`, `degraded` or `unhealthy`, `max_sessions`, `sessions_open`, `slots[]` with `status`, `generation`, `sessions`, `pages`, `healthy`, `probe_failures`) and answers 503 `browser_unhealthy` only when every launched browser is unhealthy or dead. A browser that was never launched is ready. `/health` and the Docker `HEALTHCHECK` stay on the static liveness answer. Check with `curl -s localhost:8082/ready | jq .browser`.

Known gaps: the boot check that the session cap exceeds `MAX_CONCURRENT_RUNS` plus the heavy lane (R-26) is not wired; `docs/openapi.yaml` still documents the old `/ready` body; the gateway does not yet map `browser_session_cap`, `browser_page_cap`, `session_reaped` or `browser_process_lost`. A tab attach now holds the driver's browser lock only while it creates its context and page, and an attach that is cancelled or times out closes the page and context it created.

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

Correction to the commit that added the cleaning-cost measurement (5f90ae3a). Its message says the section "Scrape cleaning cost" is in the next commit. It is not: the section first reached the branch in 37b7ebb0 (WS3.F), which committed this file as a whole, and no later commit changed it. History is not rewritten. The figures in that section are from the 5f90ae3a run; the correction did not measure them again.

The page cache (`quarry:page:{org}:{fingerprint}`) holds only the result of a default scrape. A request with options other than the defaults (`formats`, `include_tags`, `exclude_tags`, an `extractor` other than `readability`, `only_main_content` false, `remove_base64_images`), or whose `signals` select the browser driver (any `actions`, `screenshot`, `pdf`, `prior_block_signals` of 2 or more, or `profile_required`), neither reads nor writes it. ZDR never does. The response's `effective_options.cache_state` says which happened: `hit`, `miss` or `bypass`.

`crates/quarry-edge/tests/cache_policy.rs` needs a Redis at `REDIS_URL` and skips each case without one. `quarry-v2-ci.yml` provides no Redis and does not set `REQUIRE_CONTRACT`, so in that job these cases skip and the ZDR and non-default guards are not gated. A job that provides a Redis should set `REQUIRE_CONTRACT=1`: an unreachable Redis is then a failure, as in `zdr_claim_floor.rs`.

## Correction to c2b3d2e9 (WS5.D): three browser lines it carries

The message of c2b3d2e9 says this file's diff is 8 added lines and 0 removed. Its diff is 11 insertions and 3 deletions. Besides the page-cache section above this one, it changed three lines of "Browser capacity, crashes, the idle reaper and `/ready` (WS3.F)": the `QUARRY_BROWSER_REAPER` row (`0` keeps the prober), the "Keep both reaper limits" paragraph (two sentences added; the second says that a dropped `acquire` gives its place back), and the "Crash" paragraph (the prober relaunches with whatever `QUARRY_BROWSER_REAPER` says). Those lines were written for WS3.F's uncommitted review-fix round in `crates/quarry-browser/src/chromiumoxide.rs` and `slots.rs`, and they describe that code, not HEAD's. At HEAD, `QUARRY_BROWSER_REAPER=0` starts neither the reaper nor the prober (`start_background` returns early), and an `acquire` whose future is dropped during the cold launch keeps its admitted place, because only its `Err` branch releases it. The other sentence, about `log` and `0` mode (a session that is never released keeps its place until the edge restarts, and `sessions_open` counts it), is what HEAD does. Until WS3.F's browser code is committed, the three lines are ahead of the code, and they belong in the same commit as that code. History is not rewritten, so the correction is recorded here.

## Superseded: the "At HEAD" sentences of the correction above

The correction above was true when it was written (329d2b98) and stopped being true at c8c31a26, which committed WS3.F's review-fix round. Since c8c31a26, `QUARRY_BROWSER_REAPER=0` stops only the reaper: `start_background` still starts the prober, which also relaunches a browser whose process exited. An `acquire` whose future is dropped during the cold launch gives its admitted place back, so the three lines that correction names now describe the code. One refinement, from the WS3.F review-fix commit that adds this section: when a second `acquire` of the same session joined while the first was still launching, dropping the first does not give the place back; the place goes back only when no `acquire` of that admission finished and none is still running. The sentence about `log` and `0` mode (a session that is never released keeps its place until the edge restarts) still holds.

## Evidence: a rebuilt edge image at 89c67974 (F7, 2026-10-18)

An image built from 89c67974 (`docker build -f deploy/compose/Dockerfile.edge`, standalone container on a private network, dev auth bypass, `QUARRY_EDGE__DOC_ROUTING=on`, filesystem artifact backend) was exercised with curl. What was seen:

- WS5.D: `maxPages` gives 200 with top-level `markdown`, `warnings[unknown_field]` and a `Deprecation` header; `skipTlsVerification` is 400 `reserved_field`; a ZDR scrape with `markdown` and `html` returns both inline, has no `scrape_id`, and adds no artifact (in-memory list 4 -> 4; filesystem directory 9 -> 9 files, against +4 for a non-ZDR scrape); `max_inline_bytes:50` gives `omitted:["markdown"]`; with `QUARRY_EDGE__SCRAPE_REQUEST_VALIDATION=strict` the `maxPages` body is a 400 `unknown_field`.
- WS5.E: `include_tags:["#nope"]` gives empty markdown and the `include_tags_matched_nothing` warning on example.com and on a large article; `include_tags:["h1"]` on the article gives only the title; `exclude_tags:["p"]` halves it. Today's example.com has no `<h1>`, so `["h1"]` warns there.
- WS5.F: `GET /v1/scrape/<scrape_id>` returns the scrape's fingerprint, including from a restarted container and from a new container on the same volume; a ZDR scrape has no id. A filesystem-backed edge lists no artifacts through `GET /v1/artifacts` (that store keeps the default empty `list`), so count files, or use the in-memory backend, to check that a ZDR scrape stores nothing.
- WS5.C: the boot log says `isolation: process`; a public PDF and a local one parse; a 1 MB PDF that inflates to 1 GiB gave 504 `TIMEOUT` after 15 s (not the typed 415 of the unit test, which uses a 256 MiB limit), peak container memory 538 MiB of a 1 GiB limit, no OOM kill, no leftover child, and the next PDF parsed. The SSRF guard has no switch for private hosts; the test server was given an address on a private docker bridge (`--subnet 11.250.0.0/24`) that the guard classes as public.
- WS3.C: three concurrent runs (1280x800, default, 390x844 mobile at scale 2) share one browser (`browser slot launched` once); a navigate on the first run after the third started returns 200; live frames are 1280, 1280 and 390 px wide, screenshot artifacts 1280, 1280 and 780 px.
- WS3.D: with `QUARRY_BROWSER_DEADLINE_SCALE=0.02`, navigating to `https://httpbin.org/delay/10` is 504 `TIMEOUT` with `details {deadline_ms:600, effect:"unknown", op:"navigate"}`; at `quarry_browser=debug` the log carries `browser_op_duration_ms` lines.
- WS2.D: with `QUARRY_EDGE__MAX_CONCURRENT_RUNS=1` the second `POST /v1/agent/runs` is 429 `RATE_LIMITED` (`reason: global_run_cap`) with `Retry-After: 10`.
- WS3.F: `/ready` carries a `browser` object (`state`, `slots`, `sessions_open`, `max_sessions`); `idle` before the first run, `healthy` with sessions open.

Traps met while doing it: a docker named volume is root-owned and the edge runs as uid 65532, so the filesystem backend answers 500 `Permission denied` until the volume is chowned; Git Bash rewrites absolute container paths unless `MSYS_NO_PATHCONV=1`; full-page markdown (`only_main_content:false`) of a large Wikipedia article is 30.9 MB from a 1.05 MB page because table lines are padded to over a million characters, so even an 8 MB `max_inline_bytes` omits it. Not run: the typed 415 of the flate bomb on the image, the Sunset header, the S3 backend.

## Correction to the Redis gate paragraph of the WS5.D section (R8a): CI runs the gate

The paragraph of the section "Page cache bypass and the Redis-gated cache tests (WS5.D)" says `quarry-v2-ci.yml` provides no Redis and does not set `REQUIRE_CONTRACT`. That was true before the R8a change to the workflow and is no longer true. The `cargo-check` job runs a `redis:7-alpine` service, published on host port 6380, and its step "Page-cache and ZDR cache gates (Redis)" runs `cargo test -p quarry-edge --test cache_policy --test zdr_claim_floor` with `REDIS_URL=redis://127.0.0.1:6380` and `REQUIRE_CONTRACT=1`. In that step an unreachable Redis is a failure, so the nine `cache_policy` cases and the Redis case of `zdr_claim_floor` assert in CI. The `Test workspace` step of the same job keeps the default address (`127.0.0.1:6379`), where nothing listens, so its cache cases still skip there; that step is unchanged on purpose. The rest of the paragraph (the skip without Redis, and why `CI` is not a trigger) still holds.

## Challenge classifier and the one-recovery rule: `QUARRY_CHALLENGE_*` (WS3.H, WS3.J)

Two switches, both read from the edge's environment when a run (an agent run, or one scrape) starts; a change needs an edge restart. Anything but the listed values is the default. Quarry classifies what a page says about access; it never tries to get past a challenge, and neither switch changes that.

| Variable | Values | Default | Effect | Roll back |
| --- | --- | --- | --- | --- |
| `QUARRY_CHALLENGE_CLASSIFIER` | `v2`, `legacy` | `v2` | `v2`: marker-class classifier (`crates/quarry-runtime/src/challenge.rs`). Agent observations carry `challenge {strength, block_reason, recovery}`; a login page, a consent banner, a paywall, a rate limit and a bare status are `non_signal` with `requires_escalation: false`; a host that keeps challenging after its one recovery (a wait, or a reload of the same URL) is refused (below). A scrape that fetches a bot wall fails with `403 CHALLENGE_DETECTED` and `details.block_reason`. `legacy`: the substring classifier of before WS3.H, kept for one release. It flags login pages and prose as strong escalations, keeps no episodes, refuses nothing, and a scrape is not classified at all. | Set `legacy`, restart. This also removes the scrape-path refusal. |
| `QUARRY_CHALLENGE_ENFORCE` | `enforce`, `log` | `enforce` | `enforce`: an agent action on an exhausted host is refused with `403 CHALLENGE_DETECTED` (`details.recovery: "exhausted"`), and a scrape of a bot wall is refused. `log`: classification and the signals stay, nothing is refused; the agent path logs `quarry.challenge.would_reject` (field `reason`) and the scrape path logs `quarry.challenge.would_refuse_scrape` (fields `reason`, `status`), and the action or the page goes through. | Set `log`, restart. This is the first step back: it keeps the observability and removes every refusal. |

**What is refused.** Agent path, once a host is exhausted: navigation to it from any page, and click, type, press, select, upload, download and `evaluate` on its page (the snapshot-ref, frame and semantic forms included). Still allowed: screenshots, content, pdf, every wait, scroll and mouse wheel, answering a dialog that is already open, back and forward, and navigation to another host. Back and forward are allowed because Quarry cannot tell where a history step lands; the page it lands on is judged by its own host, so a step back onto the exhausted host leaves every interaction refused. Scrape path: only a strong signal (a CAPTCHA widget on a page that is itself the challenge, a WAF interstitial, two independent block markers) in an HTML, XML or text body; a status alone, a login page, a paywall and a CAPTCHA on an ordinary form scrape as before.

**Where to look.** `quarry.challenge.strong` (info; `reason`, `strong_seen`) per strong agent observation; `quarry.challenge.scrape_refused` (info; `reason`, `status`) per refused scrape; the `page_blocked` event with `reason: "challenge_detected"` and `block_reason`. No line carries a host, a URL or page text, and a Zero Data Retention run keeps only reason counts (its episode keys are hashes, and its error leaves `details.host` out).

**A real page is refused.** Read the `reason` in the log line. If the page is not a bot wall, set `QUARRY_CHALLENGE_ENFORCE=log` first and restart, so nothing is refused while the markers are fixed; use `legacy` only when the v2 classifier itself is wrong across the board. Marker tables and the case table are in `challenge.rs`; a missed or false marker is a new row there before it is a code change.

**Limits.** The classifier has been checked against hand-written fixtures (about 60 rows), not against captured pages, and no live Turnstile, hCaptcha or login page has been run through it. Treat the confidence values as unvalidated, and read `log` as the safe setting until a captured corpus exists. The plan says `log` while the corpus validates; the shipped default is `enforce`, and the choice is an open decision recorded outside this file.
