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
4. Since `7c5a564b` (WS1.C) the broker keeps non-ZDR grants in Postgres and refuses to start without `DATABASE_URL`, unless `BROWSER_BROKER_ALLOW_EPHEMERAL_DEVELOPMENT` is exactly `true`; ZDR grants (`z_` ids) stay in memory and do not survive a restart. Since `1c5f000a` `apps/Model Plane/deploy/docker-compose.yml` sets `DATABASE_URL` for `browser-broker` to `postgresql://browser_broker:${BROWSER_BROKER_PG_PASSWORD}@ingestion-postgres:5432/browser_broker`: a dedicated database and role of Quarry's Postgres server, reached over the `browser-broker-db` network that the Ingestion compose owns. `apps/Ingestion Plane/scripts/bootstrap_broker_db.sh` creates the role and database and is safe to repeat (`--check` changes nothing; `--mint-if-missing` writes a password into `apps/Model Plane/deploy/.env` only when the key is empty). `BROWSER_BROKER_PG_PASSWORD` is required (`:?`), so every Model Plane compose command fails until the key is in `deploy/.env`. A rebuilt broker image exits 1 wherever the deploy steps have not been done. **Applied to the live dev stack on the evening of 2026-10-20:** the network `browser-broker-db` was created by hand (with the labels compose expects) and `ingestion-postgres` was attached to it without a restart, `BROWSER_BROKER_PG_PASSWORD` was minted into `apps/Model Plane/deploy/.env`, `bootstrap_broker_db.sh` created the role and database `browser_broker`, and `model-plane-browser-broker-1` was recreated from an image built from this branch at `e7c321d5`; the broker logs "durable Postgres grant store", answers `/readyz` 200 and came back healthy after one `docker restart` with its table intact (0 grants then). The grant-level restart-survival check and the L2 legs A to C were not run (they need real tokens). **Two traps while the branch is not merged:** the main checkout's Ingestion compose does not define `browser-broker-db`, so a `compose up` of `ingestion-postgres` from the main checkout recreates it without that network and the broker's `/readyz` goes 503 until `docker network connect browser-broker-db ingestion-postgres`; and the main checkout's Model Plane compose still builds the old broker, so a `compose up --build browser-broker` from there replaces the new broker with one that ignores `DATABASE_URL`. Rollback: the previous image is kept as `model-plane-browser-broker:rollback-20261020` (retag it as `latest`, then `compose up -d --no-deps --no-build browser-broker` from main; the old broker ignores `DATABASE_URL`); the steps and the removal of the data are in the side-session note `wf-U1-broker-db-deploy.md`. Nothing else on the live stack changed: `quarry-edge` still runs the image of 2026-10-01 (the ledger row "Live stack change" in `docs/plans/quarry-browser-build-plan-2026-10-14.md` has the record). The server is shared by two planes and separated by database and role only: `ingestion_user`, the superuser all Quarry services connect as, can read and write `browser_grants`. Since `08d07ad2` the script keeps the generated password off every command line (the mint step rewrites the env file with shell builtins, where it first used `sed` with the value in its argv) and, unless `--keep-public-connect` is given, revokes `CONNECT` from PUBLIC on every other database that exists when it runs, so the `browser_broker` role cannot log in to those; a database created later keeps PostgreSQL's default until the script is run again. `scripts/tests/bootstrap-broker-db.test.sh` (34 checks since `a1cb37c4`, which added the one that the probe role `other_role` exists; run against a throwaway Postgres: 34 ok, `ALL PASSED` on 2026-10-21, V9, HEAD `95f94e36`) pins all of that, and pins as a known limitation that the superuser can still open `browser_broker`. CI: `browser-broker-ci.yml` has the jobs `broker`, `integration` (the tagged Postgres grant-store tests), `bootstrap-db` and `contract` (`b40242f7`); none of them has run on GitHub Actions. Before that deploy a read-only check (2026-10-20, U8) had found the broker on an image created on 2026-10-02 with no `DATABASE_URL`, no role, no database and no network. A read-only check after it (2026-10-21, V9: `docker inspect`, `docker logs`, `docker network inspect`): `model-plane-browser-broker-1` is healthy on an image created on 2026-10-20 at 18:44 UTC whose revision label is `e7c321d5`, its log shows two boots that both say "durable Postgres grant store", and `browser-broker-db` holds exactly `ingestion-postgres` and the broker. The broker also has an end-to-end harness (`apps/Model Plane/go/services/browser-broker/e2e/run.sh`, its README): it builds the broker image from the working tree and runs it against a throwaway Postgres and a stub identity provider over real gRPC, with a real restart and a stopped database; it needs Docker and touches no running stack.

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
- Series today: `quarry_http_requests_total{route,method,status_class}`, `quarry_http_request_duration_seconds{route,status_class}`, `quarry_scrape_outcomes_total{outcome}`, `quarry_lease_pool_{in_flight,peak_in_flight,capacity}{pool}`, `quarry_lease_pool_acquired_total{pool}`, `quarry_build_info{version}`. `route` is the router's matched template or `unmatched`, never the raw path; no URL, host, organization, user or run id is ever a label.
- The agent-run, step, budget, cache, pool-refusal and drain series are listed in "WS6.I series" at the end of this file. Not yet: the usage series.

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

**SC4 (2026-10-28), supersedes the numbers and the request-timeout sentence above.** Navigate and pdf are now 25 s. chromiumoxide 0.9.1 ignores the 40 s `request_timeout`: every CDP command and every navigation has its own fixed 30 s timer (`handler::REQUEST_TIMEOUT`), so the driver keeps every single-call operation below 30 s, and the library's `Request timed out.` is turned into the operation's typed `504 TIMEOUT` at any `QUARRY_BROWSER_DEADLINE_SCALE`, with `deadline_ms` the bound that actually expired (at most 30000). After a navigate `TIMEOUT` the driver stops the load before it answers, so the page can be read at once: the tab keeps the slow page if its document had arrived (only an image, a frame or the load event hung) and the page it had before if not. The same holds for a run's first navigate (T4b, 2026-10-29): the tab is opened on `about:blank` before it navigates, so a timeout keeps it (on `about:blank` if the document had not arrived) and the next step reads it instead of answering `no_current_page`. The edge's check of the page a step landed on (REST step, browser socket, delegate step; `check_landed_page`) accepts exactly `about:blank` as a tab with no page yet (T4b-1, 2026-10-29): it was a 403 "outside the run's allowed domains". It runs before the receipt and the checkpoint, so a refused step is a `failed` receipt (`SECURITY_BLOCKED`, no observation, the driver's cost since the action ran) and its page never reaches the checkpoint; its egress receipts go out with the next observation, and its `ObservationReady` event and screenshot artifact are already written. `about:blank` stays refused as a navigation target by the edge only (gate 11, tabs, attach, delegate navigate); the browser's `guard_navigation_target` admits it, as it loads nothing. A run resumed from a checkpoint with no URL has, on chromium, no tab, as a run that never navigated; its next observing step opens a blank one (SC4 D1, below). A checkpoint keeps `""` for a blank tab, and a resume treats a stored `about:blank` the same (no recovery navigation). This covers `navigate` only: a new tab opened with a URL (the new-tab and attach routes) still closes when its navigation fails, so a run whose first action is such a tab and times out still has no tab (open follow-up; an observing step after it now gets a blank one, SC4 D1). A `goto` dropped mid-navigation (client gone, outer deadline) marks its tab the same way, and the next navigate stops and clears it. chromiumoxide still holds the stopped navigation and would queue the next one behind it for up to 30 s, so the tab's next navigate first moves it to `about:blank` to clear that (about 0.1 s). Both steps run under op `navigate_recovery` (3 s, scaled); a failure is only logged as `chromiumoxide navigation recovery failed`. An agent step whose observation read (page state, content, accessibility tree) times out now fails at that first read; after an action that can change the page its `effect` is `unknown` and `retryable` false, after a wait or a read-only action `none` and true.

**SC4 D3 (2026-10-29): a navigation the browser's network guard refuses is refused at once.** The tab's Fetch guard refuses a request with `Fetch.failRequest(Aborted)`; for a main-frame document that commits nothing and raises no error, so a navigate to a host outside the run's list, or one redirected there (live R3-SC4: `slow.crawl.test/redir` 302 to fixture-b), waited out the whole navigate deadline (25 s) and answered 504 `TIMEOUT`, `effect: "unknown"`; the block showed only in the next observation's egress receipts. The driver now watches the navigation each `goto` waits on (its own first main-frame document request, for the `goto`'s URL less its fragment or the `https` form of an `http` one, and every redirect hop of it, matched by `redirectedRequestId` or, when Chromium leaves that out, the chain's network id) and ends it as soon as the guard refuses one: 403 `SECURITY_BLOCKED`, `details {reason: domain_grant_blocked | url_or_dns_blocked, op: navigate}`, no URL or host, the message for `domain_grant_blocked` being the edge's own "browser navigation is outside the run's allowed domains" (`docs/ERROR_REASONS.md`). The guard records the egress receipt before it ends the `goto`. The refused navigation still holds chromiumoxide's queue, so it gets the TIMEOUT bookkeeping: the tab is marked, the load stopped and its metadata refreshed, and the next navigate drains it first; the tab keeps the page it had (`about:blank` on a run's first step). Every navigation through the chromium `goto` has it (agent navigate on every step path, a run's first step, the resume's recovery navigation, a browser-rendered scrape), and so does the attach navigation of a new tab opened with a URL (that tab is then closed, as on any failure there). `reload`, `back` and `forward` do not wait for a navigation, so they never had the wait. A document request the page makes itself (a script or meta redirect after the document committed, a navigation the old page starts for another URL while the `goto` waits, a frame) and any subresource never end a `goto`: a refused image is only a receipt. A `goto` whose own request goes unrecognised (a URL Chromium rewrites beyond that) only loses the early end and runs to its deadline as before (review of `3bc6823ca`). For a scrape the browser is the last driver of its plan, so a refused redirect ends the fetch at once with the same code instead of after 25 s (a static shell already held is still returned).

**SC4 D1 (2026-10-29): an observing step on a session with no page answers the blank tab.** A chromium session has no tab until something opens one: a run that never navigated, a run resumed from a checkpoint with no URL (the resume makes no recovery navigation; live R3-SC4 RS), a run whose first new-tab-with-URL failed. Its first wait answered 502 `DRIVER_FAILED` `no_current_page` (`retryable: false`), while a navigate worked. An agent step whose action only reads the page (`wait`, `wait_for`, `screenshot`, `pdf`, `get_content`) now first asks the driver for a page (`BrowserDriver::ensure_page`, called by the shared observation runner, so the REST step, the browser socket and the delegate step all have it): a session with no tab gets a placeholder tab on `about:blank`, active, navigated nowhere (opened, like any page, only with the pinned egress proxy and the session's installed egress policy), and the step answers 200 with the blank observation (`url` `about:blank`, a `completed` receipt, the checkpoint keeps `""`), exactly as after a first-step navigate TIMEOUT (T4b-1). This is also the answer to a fresh run whose first step is a wait: no navigation first is needed. A `wait_for` waits on that blank tab as on any page (`body` is there at once; a selector a blank page lacks times out as it would anywhere); its snapshot forms need an observed snapshot, so a page, first. `about:blank` stays refused as a navigation target at the edge. An acting step (click, type, select, ...) on a session with no page still answers `no_current_page`; a lost or released session and a pinned handle whose tab is gone keep their errors, and a session that has a tab keeps it. The session's next `goto` navigates the placeholder tab (no second tab). WS17.C2 profiles: the placeholder has no URL to plan the restore for, so it neither loads nor restores the saved profile; the session's first `goto` does it on that tab before its navigation (load, locale/timezone emulation, cookies and web storage planned for the `goto`'s target, once per context), the same as a first `goto` that opens its own tab, including the 503 `profile_key_unavailable` refusal. A session that only observed never restored its profile and its release does not save over it (`profile_save_skipped_not_restored`).

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

The page cache (`quarry:page:{org}:{fingerprint}`) holds only the result of a default scrape. (INNH-7 added a second, separate cache for `max_age`: see "Scrape `max_age`" at the end of this file; the rules in this section are about the page cache and are unchanged.) A request with options other than the defaults (`formats`, `include_tags`, `exclude_tags`, an `extractor` other than `readability`, `only_main_content` false, `remove_base64_images`), or whose `signals` select the browser driver (any `actions`, `screenshot`, `pdf`, `prior_block_signals` of 2 or more under `QUARRY_BLOCK_ESCALATION=legacy` (ignored under `honest`), or `profile_required`), neither reads nor writes it. ZDR never does. The response's `effective_options.cache_state` says which happened: `hit`, `miss` or `bypass`.

`crates/quarry-edge/tests/cache_policy.rs` (9 cases) needs a Redis at `REDIS_URL` (default `redis://127.0.0.1:6379`) and skips each case without one. The `cargo-check` job of `quarry-v2-ci.yml` runs it, and the Redis case of `zdr_claim_floor.rs`, in the step "Page-cache and ZDR cache gates (Redis)" against a `redis:7-alpine` service published on host port 6380, with `REQUIRE_CONTRACT=1`, so there an unreachable Redis is a failure instead of a skip. The step "Test workspace" of the same job keeps the default address, where nothing listens, so its cache cases still skip. The workflow has not run on GitHub Actions (nothing was pushed); the gate was checked locally against a Redis service (`7fffb2ac`). Any other job that provides a Redis should set `REQUIRE_CONTRACT=1` for the same reason.

The Postgres store tests of `quarry-runtime` (`postgres_*`: queue, profile, baseline, event history, step receipts with migration `0011`) return early without `DATABASE_URL`, and libtest hides the skip line of a passing test, so a default run asserts nothing there. Since `f27a3327` the `cargo-check` job has a `postgres:16-alpine` service on host port 5433 and a step "Postgres store gates" that runs `cargo test -p quarry-runtime --features postgres-queue --lib postgres_` (at the default thread count since `c9d61e31`; it had `-- --test-threads=1` before) with `DATABASE_URL` and `REQUIRE_CONTRACT=1`; `postgres_step_receipts.rs` then panics instead of skipping when the database is missing or unreachable (at `f27a3327` the other `postgres_*` stores still skipped silently; since `c9d61e31` all five stores fail instead when `REQUIRE_CONTRACT=1` is set and the database is missing). The set is no longer serialised: each test has its own schema (`postgres_test_support.rs`), so the tests share no table, and "Test hygiene, round 6" below has the before and after measurement (`dec93d1e` had recorded four failing tests of the other stores when they ran in parallel on shared tables). Run on 2026-10-20 (U8, with `--test-threads=1`, as the step then was) against a throwaway `postgres:16-alpine` with `REQUIRE_CONTRACT=1`: 23 passed, 0 failed; with no `DATABASE_URL` the 5 `postgres_step_receipts` tests fail (rc 101). Re-run on 2026-10-21 (V9) at HEAD `95f94e36`, at the default thread count with `REQUIRE_CONTRACT=1`, one run in `quarry-rust-env:1` against a throwaway `postgres:16-alpine`: 23 passed, 0 failed (17.7 s). The workflow has not run on GitHub Actions.

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

**What a person sees (gateway and SPA).** The gateway turns Quarry's 403 `CHALLENGE_DETECTED` into something the SPA can act on and forwards no host, URL, page text or Quarry message. Scrape preview: a 403 whose `details.block_reason` is one of `captcha_widget`, `waf_interstitial`, `unexpected_bot_error` or `challenge_loop` is `site_blocked` with `error.blockReason`; any other 403 (a `CHALLENGE_DETECTED` without a known reason, `FORBIDDEN`, `SECURITY_BLOCKED`) is `scrape_refused`; a 429 is `scrape_rate_limited`, with a `Retry-After` header and `error.retryAfterSeconds` only when the hint is 1 to 3600. The gateway reads `error.details.retry_after_seconds` first and, since `f3f6a367`, the top-level `retry_after_seconds` of quarry-edge's own flat scrape 429 body (before that commit it read only the envelope, so against the real upstream a person never got the countdown). The earlier proxy suggestion is gone. Browser calls: a 403 `CHALLENGE_DETECTED` is answered as `403 browser_site_blocked` with only `blockReason` (from `details.block_reason` on the scrape path, `details.reason` on the agent path) and `recovery`, each from its closed list (`docs/ERROR_REASONS.md`, "403 `CHALLENGE_DETECTED`"). The SPA chooses Norwegian or English copy by code, one sentence per block reason, and never shows the response message. Every code the gateway's browser and scrape handlers emit has copy (`d58b7183` added `invalid_url`, `invalid_crawl_request`, `invalid_browser_action`, `zdr_persistent_profile_forbidden` and `model_gateway_unreachable`; `eb5a3f9e` made the gateway answer `browser_artifact_failed`, which has copy, where it said `browser_artifact_unavailable`, which had none), and a test scans the gateway's browser and scrape sources in five spellings of a code and fails on one without copy (`browser-errors.test.ts`). `BrowserBlockedNotice` appears above the page when the current observation's `challenge.strength` is `strong` (a login page, consent banner, paywall, rate limit or weak signal shows nothing), with "Take over" and "Try another source". "Try another source" runs the action `browser.blocked.try_other_source`: it stops the AI run when the composer knows a run id for that session, then closes the session. If the stop is refused (401, 403, 5xx, or a 412 that is not the ZDR text) nothing is closed and the person sees an error. For a ZDR organization the owner answers run controls with 412 "ZDR browser-run controls are disabled...": the session is closed and the SPA says the run could not be stopped from here and may still report an error (whether closing the session ends that run is not verified live). An AI-run start from the SPA carries an `Idempotency-Key` that is kept across an ambiguous retry of the same start. The gateway also forwards `X-Agent-Run-Id` and `X-Agent-Label` as advisory `aiRunId` and `agentLabel`; since `dec93d1e` Quarry reads them (kept only when identifier-shaped) and records them with the initiator on the tab timeline, the step receipt and the owner projection's `openedBy` (`docs/CONTRACTS.md`, "Which agent opened a tab"). The SPA does not read `openedBy` or `agent`. Not done: a model-driven run's own Quarry session shows no notice (the Model Plane half), and no live bot wall has been run through any of this.

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
| 5 | Agent tab calls during a take-over | PASS, with a finding | With `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=on` and the dev bypass (one person identity; the agent is expressed with the `initiated_by` hint, which can only tighten): after `POST .../browser-session/control {mode: human_takeover}`, agent tab select, tab close and `step` were each 409 `USER_TOOK_OVER` with `details {control_mode: "human_takeover", control_epoch: 1}`; the person's own select was 200; agent select after the hand-back was 200. With enforcement `off` the same calls were 200 and the log had four `would_reject: agent request during human takeover` lines. A real service-principal token was not used. Finding: on `POST .../tabs` the hint is read from `initiatedBy` only; `initiated_by` in the body is silently ignored, so that agent call opened a tab during a take-over with enforcement on (200, and no `would_reject` line with enforcement off). Select and close take both spellings in the query, and `/step` takes `initiated_by`. Fixed since, in `dec93d1e`: every body and query takes both spellings and both in one body are a 422 (`docs/CONTRACTS.md`); the image of this run predates it. |
| 6 | Run cap | PASS | `QUARRY_EDGE__MAX_CONCURRENT_RUNS=1`: log `run limits: global=1`; run 1 200; run 2 `429 Too Many Requests`, header `retry-after: 10`, `RATE_LIMITED`, `details {reason: "global_run_cap", scope: "global", active: 1, limit: 1, retry_after_seconds: 10}`; `/ready` stayed `ready`. |
| 7 | Challenge classification (R6) | PASS | A local page with the Cloudflare interstitial markup and one with a bare Turnstile widget, scraped with `QUARRY_CHALLENGE_ENFORCE=enforce`: 403 `CHALLENGE_DETECTED`, `details.block_reason` `waf_interstitial` and `captcha_widget`, `next: "other_source"`, and no page text in the body; a normal PDF on the same edge was 200. At the default (`log`) the interstitial scraped 200 with its text and the log carried `quarry.challenge.would_refuse_scrape` (`reason waf_interstitial`, `status 200`). Fixtures only, no live challenge page. |

Not covered: a real service-principal token (the dev bypass has one identity), the S3 artifact backend, a live challenge page. The same table with the commands is in the side-session notes, `wf-R9-evidence.md`.

**Live Chromium CI lane: no `continue-on-error`, not yet a required status check (replaces the earlier "continue-on-error" remark under "Page and frame ownership"; this heading used to say "is a required check", which no file in the repository can make true).** The `quarry-browser-live` job in `.github/workflows/quarry-v2-ci.yml` ("Live browser tests (Chromium)") no longer has `continue-on-error`; whether it is a required status check is a repository setting that nothing here verified (a repository admin has to add that check name to the branch protection rule for `main`), and the workflow only runs on PRs that touch `apps/Ingestion Plane/Quarry-v2/**` or the workflow file. It installs Chrome for Testing `155.0.8059.84` through `browser-actions/setup-chrome@v2` (bump it on purpose), exports its path as `CHROME`, sets `QUARRY_BROWSER_NO_SANDBOX=1` (hosted runners restrict unprivileged user namespaces; the edge refuses that flag when `ENVIRONMENT` is prod) and runs three sets with `--test-threads=1`: `cargo test -p quarry-browser --features chromiumoxide`, `cargo test -p quarry-runtime --features chromiumoxide --test browser_egress_boundary` (compiled out without the feature, so `cargo test --workspace` never ran it) and `cargo test -p quarry-edge --test zdr_behavior` (needs no Chromium; it already ran in the workspace job). A live test no longer skips when `CHROMIUMOXIDE_TEST=1` and there is no browser: `quarry_browser::live_gate` fails it, also when `CHROME` names a file that does not exist (chromiumoxide alone would fall back to any `chromium` on the `PATH`). To run the lane locally, set `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/path/to/chromium` and run the three commands above. Evidence: in the `quarry-rust-env:1` container (Chromium 155.0.8059.79) the three commands passed three runs in a row (135 passed + 4 ignored, 2 passed, 4 passed each time; the first run took 25 minutes including the cold build); with `CHROME=/nonexistent`, and again with no Chromium on the `PATH`, the quarry-browser and browser_egress_boundary commands failed (exit 101) and zdr_behavior still passed. Not verified: the job itself on a GitHub-hosted runner (it cannot be run here), that `setup-chrome@v2` accepts the pinned version and `install-dependencies` on `ubuntu-24.04`, and the runner's wall time and disk use with the quarry-edge build added. `actionlint` (the `rhysd/actionlint` image) reported nothing on the workflow file. Re-run on 2026-10-20 (the doc-reconcile unit U8) on an export of HEAD `137ba2ac`, same image, Chromium 155.0.8059.79: the quarry-browser set 135 passed, 0 failed, 4 ignored; `browser_egress_boundary` 2 passed; `zdr_behavior` 4 passed; `quarry_browser::live_gate` 6 passed; with `CHROME=/nonexistent` the quarry-browser set exits 101; without the feature `cargo test -p quarry-browser` is 85 passed. At `137ba2ac` the five tests of `quarry-browser/tests/ssrf.rs` were all `#[ignore]` (0 passed, 5 ignored) and belonged to none of the three sets; `3496dd07` un-ignored four of them behind the gate, so since then they run in the first set (see the next paragraph). **Re-run on 2026-10-20 at HEAD `c7429f2e`** (the doc-reconcile unit U8 again; `quarry-rust-env:1`, Chromium 155.0.8059.79, `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium`, `--test-threads=1`, the three commands once, in one container): set 1 lib 138 passed, 0 failed, 4 ignored and `tests/ssrf.rs` 4 passed, 1 ignored; set 2 (`browser_egress_boundary`) 5 passed; set 3 (`zdr_behavior`) 4 passed; all rc 0; `quarry_browser::live_gate` 9 passed; `-- --ignored --list` lists 5 tests (the 4 ignored in the lib and `test_valid_public_url`). With `CHROME=/nonexistent`: set 1 exits 101 (119 passed, 19 failed, 4 ignored) and set 2 exits 101 (2 passed, 3 failed). With `CHROMIUMOXIDE_TEST` unset and the feature on, set 1 is 138 passed, 4 ignored, 4 passed and 1 ignored (the live tests skip); with the feature off `cargo test -p quarry-browser` is 85 passed. One run, not a loop; not run on GitHub Actions. **Re-run on 2026-10-21 at HEAD `95f94e36`** (V9, same image and flags, `--test-threads=1`, the three commands once): set 1 lib 172 passed, 0 failed, 4 ignored (`quarry-browser` gained the WS3.K tests, see "Trusted input for ref actions") and `tests/ssrf.rs` 4 passed, 1 ignored; set 2 (`browser_egress_boundary`) 5 passed; set 3 (`zdr_behavior`) 4 passed; all rc 0.

**Live Chromium CI lane: what each live set proves (corrects the paragraph above).** `quarry-browser/tests/ssrf.rs` now runs in the lane (four tests; they go through `live_gate` and fail when Chromium cannot launch) but it proves only that `ChromiumoxideDriver::goto` refuses loopback, metadata and RFC1918 URLs in Rust before any CDP command; the fifth test there, `test_valid_public_url`, stays `#[ignore]`d because it needs DNS for a public host. `browser_egress_boundary` is what shows a running Chromium being refused: its page-initiated tests make Chromium itself request a loopback listener and read the CDP boundary's refusal from the driver's receipts, once by the domain grant (`domain_grant_blocked`) and once by the SSRF guard (`url_or_dns_blocked`, with a grant that names `127.0.0.1` so the grant layer cannot be what refuses); the proxy tests read `domain_grant_blocked` and `dns_or_ssrf_blocked` from the pinned proxy's own receipts. The fail-instead-of-skip mapping is pinned by `live_gate::enforce` unit tests (`#[should_panic]`; turning the panic into a skip fails two of them). The lane itself can never reach that branch, because its install step must yield an executable.

## A grant carries a budget claim (`QUARRY_EDGE__GRANT_BUDGET_CLAIM`, `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM`; WS6.E)

A browser-broker grant can carry an upper bound on what a run under it may spend: `budget_micro_usd`, `budget_max_steps`, `budget_max_runtime_s`, a `budget_source` (`request`, `org_default` or `entitlement`) and a `budget_minted_at_unix` the broker stamps itself (proto fields 40-44 on `AcquireGrant*`, `ValidateGrantResponse` and `DeriveRunGrant*`; stored in `browser_grants` by broker migration `0003_browser_grants_budget`). **The claim can only tighten.** Quarry takes, per dimension, the smaller of what the request, the default and the ceiling gave and what the claim says; a claim that is not smaller changes nothing, so it cannot raise a bound. The broker refuses a derived run grant that is looser than its parent's claim (`derive_widens_budget`, PermissionDenied) and keeps a parent's bound when the derive names none.

- **Minter.** model-gateway `POST /v1/browser/runs`, when `MODEL_GATEWAY_BROWSER_BUDGET_CLAIM=on` (default off; anything but `on` is off): cost is `min(request.max_cost_usd, org-core per-run ceiling)`, steps and runtime are the request's; the claim is clipped to the parent session grant's own and rides `DeriveRunGrant`. The same numbers replace the request's in the `browser_agent` tool input. An unreachable or unconfigured org-core is fail-open (Quarry's ceiling still applies). A run grant the caller presents is not re-minted at the broker, but its run parameters are clipped to the tighter of the claim it carries and what this request mints, so the organization's ceiling also reaches a run grant a member derived with no claim. **A retry finds its grant.** The claim is recomputed on every attempt, so a keyed retry (same `Idempotency-Key`) can ask for a different claim than the first attempt did (org-core down, a ceiling that moved, a different `max_cost_usd`). The broker refuses a derive replay that names a different claim, so the route then asks once more for no claim, which the broker answers with the grant it issued and the claim it carries (`sameDeriveBudget`); a replay that names no claim cannot widen or re-stamp anything, and a `409` from that second ask is a real conflict (a different host list). The grant found that way keeps the claim it was issued with, but the run is bounded by the tighter of that claim and the claim this attempt mints (`8410abea`), so a first attempt made with the flag off, or before the organization's ceiling was lowered, does not let the retried run escape the ceiling. The lookup needs the rebuilt broker (`86827c60`); an old broker answers the replay with the 409 as before. Where the minter lives is ADR-1; only `mint_budget` moves.
- **Enforcement.** Quarry `start_run`, when `QUARRY_EDGE__GRANT_BUDGET_CLAIM=enforce` (default `ignore`: the claim is read and carried and changes nothing; a misspelt value stops the edge at boot). It is applied once, at start: `data.clamped[]` then names `by: "grant"` and a refusal's `error.details.limit_source` is `grant` (it is `run_budget` otherwise). With enforcement on, a grant of kind `run` makes the run an agent run (R-8), so a person's token holding one is not an uncapped interactive run. A person's own run on a session grant is never capped by it.
- **One run, not the grant.** The claim caps one Quarry run. Quarry keeps no spend state per grant: a second `start_run` under the same run grant gets the whole claim again, and only a resume of a named run (`resume_run_id`) continues a ledger (pinned by `the_claim_caps_each_run_not_the_grant`). The broker mints a run grant for one model run and the gateway presents it to one dispatch, so in the intended flow a grant backs one run; a caller who starts several runs under one grant is bounded per run, by the concurrency and per-organization run caps, and by the grant's expiry, not by a shared total. A per-grant total would need a use-once grant at the broker or a spend ledger keyed by grant at the edge; neither exists.
- **Never re-widened.** The claim a run started under is stored in its checkpoint (`budget.claim`, optional). Per-action grant revalidation does not read it; a resume keeps the tighter of the stored claim and the grant the resume names, and what was spent is not refilled.
- **ZDR.** The claim is three numbers and a label, never content. A ZDR grant carries one in the broker's memory only (its `z_` id never reaches the table), and a ZDR run writes no checkpoint, so nothing about the claim is persisted for it.
- **Roll out.** Broker first (done on the live dev stack on 2026-10-20 as part of the broker's redeploy; migration `0003` is idempotent and nullable: an old broker returns zeros, an old Quarry ignores the fields), then model-gateway `on`, read `clamped` on a few runs, then Quarry `enforce`. Roll back by setting either switch back; nothing is migrated down.

Known gaps: of the three, only the broker runs this code in the live stack (the broker image of 2026-10-20, built at `e7c321d5`, contains the claim fields and migration `0003`; neither switch is set, and the running Quarry and model-gateway images are older and do not have them), and Quarry has no broker service credential before WS1.A2 (X-1), so no production run carries a claim; execution-core does not read the claim (the `browser_agent` tool input carries the tightened numbers instead; mapping the 402 to a resource-limit outcome is sibling (b)'s); `source: entitlement` is accepted and unused until WS6.D.

## A ZDR organization and durable resources (`QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES`; WS4.H stage A)

ADR-4 (recommended default, accepted 2026-10-15) says a Zero Data Retention organization cannot create the durable resources the ZDR matrix lists as gaps: schedules, change monitors and snapshots, browser profile writes. This flag is the switch; stage A ships it **off**.

| Variable | Values | Default | What it does | Rollback |
|---|---|---|---|---|
| `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` | `true`, `false` | `false` | `false` (observe): a ZDR caller is served exactly as before, and each request below leaves one `zdr.would_refuse` log event with the fields `row`, `action` and `org_id` (no URL, host, title or query). `true` (enforce): the same requests are refused `403 FORBIDDEN` with `error.details.zdr_refusal {code: "zdr_durable_mutation_forbidden", action}` and a `zdr.refused` event, before Control Plane or a store is reached. | Set `false`, restart. |

Refused (enforce) and observed (observe), by matrix row and `action`: `schedules` (`create`, `unpause`, `trigger`, `backfill`); `change_history` (`check`); `change_snapshot` (`snapshot`, `refresh`); `profiles` (`save` = `POST /v1/profiles`, `create` = `POST /v1/profiles/create`, `update` = `PATCH /v1/profiles/{id}`); `change_record_internal` (`record`, only when the orchestrator's body declares `zdr: true`: no baseline is written). Never refused, so a ZDR organization can wind down: schedule list, pause and delete; profile list, load, delete and `restore_probe` (reads, writes nothing); `/v1/change/latest` and `/v1/change/history` (reads). `/v1/search/suggest` is unassigned and not covered. The internal change record is refused only when the request body says `zdr: true`; the orchestrator's change activity (`services/quarry-orchestrator/internal/activities/activities.go`, `changeRecordRequest`) sends only `org_id`, `url`, `fresh_fingerprint` and `run_id` at HEAD, so that row stays quiet until the Go side sends the flag. The Go side does not send it yet, because the orchestrator does not know the posture (see "What the orchestrator lacks" below); what it does since V4 is end a run whose record call is refused with `zdr_durable_mutation_forbidden` (403) cleanly: the activity returns `status: zdr_refused` instead of an error, the workflow emits one `run_failed` (`kind: change_monitor`, `reason: zdr_refused`, `refusal_code`, no URL) and completes without error, nothing retries, and no `change_*` event is emitted. Any other 403 from that route is still an auth failure there. Tests: `crates/quarry-edge/tests/zdr_refusal_modes.rs` (8: five for the flag-gated refusals in observe and enforce, with the `zdr.refused` event, the empty profile store and the wind-down routes, and three for the refusals that already existed, below) and the unit tests in `zdr_refusal.rs`.

Read the observe week with the log query `message = "zdr.would_refuse"`, grouped by `org_id` and `row`. Stage B (flag on by default in compose) waits for the owner's ADR-4 sign-off and the customer notice. The refusals that already existed (crawl, batch, a persistent profile or a resume of a ZDR checkpoint at run start) are enforced whatever the flag says and, since V4, carry the same `details.zdr_refusal {code, action}` (`docs/ERROR_REASONS.md`); their status, error code and message did not change. Tests: `zdr_refusal_modes` (`durable_crawl_and_batch_refusals_carry_zdr_refusal_details_in_both_modes`, `the_persistent_profile_refusal_at_run_start_carries_zdr_refusal_details_in_both_modes`, `resuming_a_zdr_checkpoint_is_refused_with_zdr_refusal_details_in_both_modes`). Not done in stage A: flipping the rows to `Enforced` in `quarry_core::zdr_matrix` (the golden and the repo-level sync script move with it).

**What the orchestrator lacks to send `zdr: true` on the change record (V4).** A change-monitor run is fired by a Temporal schedule that quarry-control materialises from its `schedules` row (`services/quarry-control/internal/resources/schedule_wire.go`: the args are `{org_id, url, created_by}`). The row (`store.Schedule`) has no ZDR column, the edge's schedule-create forward (`ControlScheduleCreate` in `schedule_routes.rs`) sends none, and a cron fire has no caller token whose floor could be read, so at fire time the orchestrator has nothing that says the organization is under ZDR. The organization's posture lives in the token claims (Auth Core), not in Ingestion. Sending the flag needs one of: (a) a `zdr` column on the quarry-control schedule row, stamped by the edge from `Claims::effective_zdr` at create time, carried in `changeMonitorArgs` and `ChangeMonitorInput`, then `CheckChangeInput` and `changeRecordRequest` (a schema change in quarry-control, and a schedule created before an organization turned ZDR on would still not carry it); or (b) a posture lookup against Control Plane by the orchestrator at fire time (a new cross-plane client and a Control Plane contract). Neither was invented here.

**Known gap: the URL of a refused change-monitor run is still durable (V4, round 6).** `ChangeMonitorWF` emits `run_started` with `{kind, url}` BEFORE the record call, because the orchestrator learns that the edge refuses only from that call's answer. So for a run the edge then refuses under ZDR, the `run_started` event still carries the monitored URL; only the `run_failed` event that follows (`reason: zdr_refused`, `refusal_code`) is URL-free. The earlier wording "no URL" in the V4 commit message and in this section meant that `run_failed` payload only. The test `TestChangeMonitorWF_ZDRRefusalEndsTheRunCleanly` now pins both facts (the URL in `run_started`, none in `run_failed`). Closing it needs the posture before the first event (option (a) or (b) above); dropping the URL from `run_started` for every change-monitor run would change an event contract other consumers read, and was not done. It matters only for a schedule that exists for an organization that later turned ZDR on, because the edge refuses a ZDR organization's schedule create when the flag is on.

**Corrections to the V4 record.** The commit `e32a7cf8` says `cargo test -p quarry-edge --tests --no-fail-fast` ran 27 test binaries. The right count is 26 binaries (lib and bin plus 24 integration files; 975 tests, 0 failed when re-counted in round 8 at 57607501: lib 381, bin 375, the rest in the integration files. An earlier draft of this paragraph said 972, which was a stale sum; the total moves whenever a test is added, so re-count before quoting it); the 27th `test result` line appears only without `--tests`, as the `Doc-tests quarry_edge` target with 0 tests. The plan's WS4.H detail says the persistent-profile refusal gains `legacy_code: "zdr_persistent_profile_forbidden"`; the first V4 commit left it out and it was added in round 6 (the resume refusal has no legacy token and gets none). Round 7: the commit `bf5c7efd` says the refused run's payload "has no URL"; that holds for the `run_failed` event only (see the known gap above). The workflow test line that pins the URL in `run_started` is a record of that retention gap, not desired behaviour, and must be flipped in the change that closes it. The persistent-profile test now asserts the full refusal message, not only its token.

## Evidence, round 4 (rebuilt edge, 2026-10-20, U9)

Two edge images were built with `docker build -f deploy/compose/Dockerfile.edge` (rc 0) and run alone on a private network, one at a time, with the dev bypass and `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=on`: `u9head`, from an export of HEAD `31441f12`, and `u9wt`, from the working tree at that HEAD. The second one exists because the tab-initiator code (U3) and the WS6.E code (U5) were not committed when this was run, so a HEAD image alone could only show the old behaviour. They are committed now (`dec93d1e`; `3dea6da5` and `df6460aa`), but no image has been built from a HEAD that contains them: what `u9wt` showed is the behaviour of the working tree that became those commits, and the committed blobs were checked by `cargo test` in a clean `HEAD` plus those files (`agent_run_control` 28 passed), not on an image. Both images were removed afterwards. The table with the commands and the raw output is in the side-session notes, `wf-U9-evidence.md`.

- **Tab initiator, image of HEAD `31441f12` (before the fix was committed): the bug was there; fixed in `dec93d1e`.** During a take-over an agent tab open with the body field `initiatedBy` is `409 USER_TOOK_OVER`, but with the snake_case `initiated_by` it is `200` and opens a tab; outside a take-over it is recorded as `human`. Both spellings in one body are `200`, not `422`. The owner projection has no `openedBy`, and the timeline items and receipts have no `agent` object.
- **Tab initiator, image of the working tree that held U3's code (committed afterwards as `dec93d1e`): as documented.** The same calls: agent open with either spelling, select and close (`?initiated_by=agent` or `?initiatedBy=agent`) and step are `409 USER_TOOK_OVER` with `details {control_mode: "human_takeover", control_epoch: 1}`; the person's own open (no hint, or `initiated_by: human`) and select are `200`; both spellings in one body are a plain-text `422`. `GET .../browser-session` shows `tabs[].openedBy` (`{initiatedBy: human}`, or `{initiatedBy: agent, agent: {basis: user_session, aiRunId, label}}`), and the timeline items and the step receipts carry the same `agent`. With a person's token there is never a `principalId`. Not run: a real service-principal token (`basis: service_principal`, `principalId`) and the Postgres column `initiated_agent`.
- **`GET /ready`** returned `200` with `browser`, `scrape_browser`, `durable` and `status` on both images, with the slot keys of `docs/openapi.yaml`.
- **Live Chromium lane** (before the SSRF and egress test changes of `3496dd07` and `f1e3a19e`; re-run at HEAD `c7429f2e` in the lane paragraph above), the three commands of the `quarry-browser-live` job, once, in `quarry-rust-env:1` (Chromium 155.0.8059.79, not the CI pin `155.0.8059.84`) on an export of HEAD `31441f12`: `quarry-browser` 135 passed, 4 ignored (`tests/ssrf.rs`: 0 passed, 5 ignored at `31441f12`); `browser_egress_boundary` 2 passed; `zdr_behavior` 4 passed; all rc 0. Not run on GitHub Actions.
- **WS6.E (grant budget claim):** not run over HTTP. A claim exists only on a grant the browser-broker issues and the edge has no broker standalone. The working tree with its hunks compiled into the release edge. The code is committed since (`561b6d48`, `3dea6da5`, `df6460aa`, `6e8d2a60`, and the review rounds up to `8410abea`); its route-level tests (`grant_budget_claim` 8, `grant_budget_boot` 2, which starts the real binary) pass in the workspace run of 2026-10-20 recorded in `PROGRESS.md`, and there is still no run with a rebuilt broker and a model-gateway minter.

## Evidence, round 4, repeated on the committed HEAD `ddbd4b2c` (U9, 2026-10-20)

The section above ran on images of `31441f12` and of a working tree, before U3 and U5 were committed. This run builds the edge from the committed HEAD `ddbd4b2c` (the Quarry-v2 tree was clean): `docker build --build-arg SOURCE_REVISION=u9r4-ddbd4b2c -f deploy/compose/Dockerfile.edge .`, rc 0, `/version` answers `u9r4-ddbd4b2c`. It ran alone on a private network (`qv2e-net`, containers `qv2e-edge` and `qv2e-www`), with the dev bypass and `QUARRY_EDGE__TAKEOVER_ENFORCEMENT=on` (log `takeover_enforced=true`). Everything was removed afterwards (`docker ps -a`, `network ls`, `volume ls` and `images` show nothing named qv2e or u9).

- **Tab initiator: as documented, on a HEAD image.** During a take-over (control epoch 1) an agent tab open with the body field `initiated_by` and with `initiatedBy`, an agent select (`?initiated_by=agent`), an agent close (`?initiatedBy=agent`) and an agent step are all `409 USER_TOOK_OVER` with `details {control_epoch: 1, control_mode: "human_takeover"}` (the bug of `31441f12`, a `200` for the snake_case spelling, is gone). The person's own open (no hint, and `initiated_by: human`) and select are `200`. Outside a take-over the agent opens are `200`. Both spellings in one body are a plain-text `422 duplicate field initiatedBy`. `GET .../tabs` lists tabs with no opener, as `docs/openapi.yaml` says. `GET .../browser-session`: `tabs[].openedBy` is `{initiatedBy: human}` for the person's tab and `{initiatedBy: agent, agent: {basis: user_session, aiRunId, label}}` for an agent's (snake_case and camelCase advisory fields both read). The timeline `tab` and `action` items and the step receipts carry the same `initiatedBy` and `agent`; the step during the take-over has `agent {basis: user_session}` and epoch 1. No `principalId` anywhere (a person's token), and zero `would_reject` log lines with enforcement on. Not run: a real service-principal token (`basis: service_principal`, `principalId`) and the Postgres column `initiated_agent`: the edge ran without a database.
- **`GET /ready`:** `200` `{"browser":{max_sessions 40, sessions_open, slots[{generation, healthy, pages, probe_failures, sessions, slot, status}], state},"durable":true,"scrape_browser":{same},"status":"ready"}`. Before any run both drivers are `state: idle`, slot `status: starting`; with one agent run open `browser.state` is `healthy`, `sessions_open` 1, slot `ready`, `healthy: true`, `pages` 5.
- **Live Chromium lane**, the three commands of `quarry-browser-live` plus the `--ignored --list` step, once, in `quarry-rust-env:1` (Chromium 155.0.8059.79; the CI pin is 155.0.8059.84) on the clean tree at `ddbd4b2c`, with `CARGO_PROFILE_TEST_DEBUG=0 CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1` and `--test-threads=1`: `quarry-browser --features chromiumoxide` 138 passed, 4 ignored (lib) and `tests/ssrf.rs` 4 passed, 1 ignored; `browser_egress_boundary` 5 passed; `zdr_behavior` 4 passed; the ignored list names 5 tests; all rc 0. Not run on GitHub Actions.
- **WS6.E (grant budget claim), the part that can be driven without a broker:** `QUARRY_EDGE__GRANT_BUDGET_CLAIM=bogus` stops the real release binary at boot (`Error: budget: QUARRY_EDGE__GRANT_BUDGET_CLAIM must be ignore or enforce, got "bogus"`, exit 1); `enforce` boots and logs `grant budget claim mode=enforce`. Not run: a claim carried on a grant (`by: grant`, `limit_source: grant`): the edge has no broker standalone, so that still needs a rebuilt broker and a model-gateway minter.

## Test hygiene, round 6: Postgres store tests in parallel, tab opener cap, mutation checks (V2, 2026-10-20)

- **Postgres store tests (`postgres_*` in `quarry-runtime`) no longer need `--test-threads=1`** (this replaced the "Serialised on purpose" sentence of the Postgres store gates paragraph under "Page cache bypass and the Redis-gated cache tests"; `36977011` said so but left that sentence as written, and the V9 documentation pass rewrote it, together with the `--test-threads=1` in the PROGRESS row). Cause: the stores' tables are unqualified, so every test used the same tables in the database's default schema and began with a `TRUNCATE`, and tests of one store and of different stores wiped each other's rows. Now each test gets its own schema (`crates/quarry-runtime/src/postgres_test_support.rs`: `qt_<unix secs>_<uuid>`, `SET search_path` on every connection, migrations applied into it) and the four stores that used to skip silently on an unreachable database also honour `REQUIRE_CONTRACT=1`. Evidence (a throwaway `postgres:16-alpine`, `quarry-rust-env:1`, `REQUIRE_CONTRACT=1`, default thread count, 5 runs each, one container at a time): the changed tests, 23 passed and 0 failed in all 5 runs (17 s a run); the same tests on the tree before the change (a copy of the working tree with `lib.rs` and the five store files restored from HEAD), 5 of 5 runs failed, 5 or 6 of the 23 tests each time (profile `save_is_upsert_on_repeated_id` and `list_orders_by_recent_update`, event history `replay_window_filters_by_timestamp`, baseline `compare_snapshot_returns_correct_status`, `load_history_orders_newest_first` in three runs `save_then_load_latest_returns_most_recent`, and in two runs one queue test). Cost: a test run leaves one `qt_` schema per test (115 after 5 runs); each call drops the `qt_` schemas older than six hours. The CI step "Postgres store gates" runs without `--test-threads=1` since `c9d61e31`. Not run on GitHub Actions (its Postgres service has `max_connections` 100; each test's pool is at most 5 connections).
- **Opener record of a run's tabs (`openedBy`).** The record of who opened each tab (tab routes only) keeps up to 256 full records (class and the agent's ids); a tab opened after that has `openedBy: {initiatedBy, unattributed: true}` (class only), so it is not shown as a tab the browser opened itself, and for a ZDR run (no timeline item, no receipt) the class of every tab opener is still readable. Records are pruned against the driver's tab list by every route that lists the tabs (the tab list, open, select, close, the control change and `GET .../browser-session`), so a tab that goes away with no close call (a page that closes itself, or a tab the user closes in the browser) loses its record at the next of those calls, and the map never holds more entries than the browser had tabs at the last one. Tests: `agent_routes::tests::the_tab_opener_record_is_bounded_and_prunes_gone_tabs`, `agent_routes::tests::prune_tab_openers_drops_the_openers_of_tabs_that_are_gone`, `agent_run_control::tabs_opened_past_the_opener_cap_are_marked_unattributed_in_the_projection` and `agent_run_control::a_tab_that_disappears_without_a_close_call_loses_its_opener`.
- **Mutation checks, done.** `browser_egress_boundary.rs` (copy of the workspace, own cargo target volume, Chromium, `--test-threads=1`): baseline 5 passed; `guard_page_request_target` returning `Ok` turns the `..._ssrf_guard` test red; the CDP interception not installed turns both page tests red, the domain-grant one at the `6cffdc2e` assertion (the pinned proxy holds a receipt for the listener); the grant layer of the CDP guard forced open turns the domain-grant test red; restored, 5 passed. `scripts/tests/bootstrap-broker-db.test.sh`: with the `CREATE ROLE other_role` line deleted the old test still passed, the new one fails (2 FAILED). Gateway: `unwrap_or(10)` of `browser_capacity_response` changed to 11 turns the new absent-retry-key test red.
- **SDK docs drift check and Python SDK tests in CI.** The job `spa-verify` of `verevonv3-ci.yml` runs `sdks/generate.sh` (step "Generate the Quarry SDKs") and then two steps that run inside `apps/Ingestion Plane/Quarry-v2/sdks`. "Check the committed SDK docs match the spec" fails when `git status --porcelain -- .` is not empty (committed `sdks/typescript/docs` pages that differ from a regeneration of `docs/openapi.yaml`, or pages the spec added and nobody committed), and also when fewer than 100 pages are tracked, so an empty comparison cannot pass. "Test the generated Quarry Python SDK" installs the generated `requirements.txt` plus pytest into a throwaway venv and runs `sdks/python/tests` (28 tests, about 9 s) against the freshly generated `quarry_client`. **Trap, fixed in round 4:** the first version of the drift step had no `working-directory`, so it inherited the job default `apps/Frontend Plane/verevonv3`; the repo-root pathspec then named a directory that does not exist, `git status` printed only a warning and exited 0, and the step could never fail. The step now runs in the sdks directory and compares `.`. Negative controls on the committed tree, run with the step's script from that directory: clean tree rc 0 (281 pages counted); an untracked page under `sdks/typescript/docs` rc 1; a tracked page with an added line rc 1; the old command from the SPA directory with the same untracked page printed only the warning (no status line). Two regenerations of the working-tree spec were byte-identical; compared with the committed pages, exactly one differed (`TabOpener.md`, by the `unattributed` field above), now committed. The 28 Python tests passed against that regeneration. Not run on a GitHub runner (no push): the venv and pip steps are verified only by the equivalent local run (pip `--target` plus `PYTHONPATH`).


## Trusted input for ref actions (WS3.K)

`QUARRY_BROWSER_TRUSTED_INPUT=off|on`, default `off`, read when the Chromium driver is built. With `on`, a click, a type and a select on a ref in the page's own (top-level) frame first pass an actionability check, and a click and a type then reach the page as real browser input (`event.isTrusted` is `true`). `driver_capabilities.trusted_input` in the run start response says which: `true` only with the flag on. It is separate from `atomic_target_actions`, which still gates every ref action whatever this flag says. `TARGET_REPAIR_REQUIRED` keeps its 409 and its wire code; with the flag on it carries a closed `details.reason` (`docs/ERROR_REASONS.md`, "409 `TARGET_REPAIR_REQUIRED` from trusted input"). The Model Plane planner sentence that tells the model to dismiss an overlay or scroll and observe again, and the `browser.act` schema, are owned by Model Plane and not part of this change.

What it does, per operation:

| operation | before | with the flag on |
|---|---|---|
| click on a ref | `element.click()` in the page, whatever covers the element | probe (attached, visible, enabled) and scroll into view, the box holds still (two samples within 0.5 px, 25 ms apart, 500 ms window), click point = centre of the part of the box inside the viewport, hit test at that point (open shadow roots and slots are walked, `a_slotted_child_of_the_target_passes_through_the_assigned_slot` reaches the slot step of the climb; the target, a descendant or its `<label>` passes; a cover inside a closed shadow root cannot be seen, because the root hides it, and the root's host counts as the target's own component; a cover inside the same closed root as its target is therefore not detected and the click lands on it, pinned by `a_cover_in_the_same_closed_root_as_the_target_is_not_detected`), then `Input.dispatchMouseEvent` move, press, release |
| type on a ref | value setter plus synthetic `input` and `change` | the same checks as a click (visible, enabled, not read-only, scrolled into view, holds still, the field is what receives the pointer at the middle of its visible box), then `DOM.focus`, select all, `Input.insertText` (an empty text deletes the selection). Trusted `beforeinput` and `input`. No synthetic `change`: the browser fires it on blur, as for a person. Also text-like: `contenteditable`. A control that takes a value but not typed text (`date`, `datetime-local`, `month`, `week`, `time`, `color`, `range`) keeps the in-page setter after the same checks (its events are not trusted). Any other input (checkbox, radio, hidden, submit, file, ...) and anything that is not a field is refused with `target_disabled` |
| select on a ref | in-page `change` | the same checks as a click (visible, enabled, holds still, not covered) first, then the same in-page setter and `input` and `change` events (not trusted, as with the option setters of the common automation libraries) |
| wait for a ref | connected check | unchanged |

The whole check polls every 100 ms for up to 2 s (`Op::Actionability`); the reason in the error is the last one seen. A node that left the document fails at once with `target_not_attached`: a removed node still resolves from its backend node id, and the first probe finds it detached (`display_none_fails_target_not_visible` asserts this). A node that leaves after it was resolved and before the probe, and a handle whose realm a navigation destroyed, both answer `target_not_attached` at once (`a_node_that_leaves_between_resolve_and_probe_is_not_attached`, `a_handle_whose_realm_was_destroyed_by_a_navigation_is_not_attached`); a ref from the previous document fails earlier, in `DOM.resolveNode`, with the older reason-less error (same test, second half).

Stays on the old in-page path whatever the flag says: a target in a child frame (its coordinates and, for a cross-process iframe, its process are not reachable from here; WS3.L), governed downloads (`download_from_verified_target`), uploads (they never used the click helper: `DOM.setFileInputFiles` on the staged file), `wait_for`, and the legacy selector script path. The selector `click` and `press` actions already used trusted `Element::click` and `press_key` before this change.

A click that opens a JavaScript `alert`, `confirm` or `prompt` blocks the page's main thread, and with it `Input.dispatchMouseEvent`, until the dialog is answered. Measured with Chromium 155 in `quarry-rust-env:1`: the dialog collector holds the dialog within 300 ms, a raw click did not return in 4 s, and with the watch the click returns in about 0.6 s with the dialog pending for `respond_dialog`. The release of the element handle is bounded to 500 ms for the same reason (it queues behind the dialog and runs when it is answered). The in-page click of the old path runs inside `Runtime.callFunctionOn`, which the same dialog would hold; that was not measured here. A ref action that comes while the dialog is still pending is bounded by `Op::NativeAction` (10 s) as before: the lookup that decides the input mode (a frame lookup, for a target bound to a frame) runs inside that deadline, not before it, because `Page.getFrameTree`, `Runtime.evaluate` and `DOM.getDocument` do not answer while a dialog is open and would otherwise wait for the 40 s request timeout (`a_second_ref_action_with_a_dialog_pending_returns_within_the_bound`; with the old ordering the same test does not return within 15 s). An ordinary top-level target carries no frame id in this Chromium, so the lookup is only on the path for a target bound to a frame.

**Soak rule.** Do not set `on` anywhere until the `quarry-browser-live` lane has been green for two weeks with this change in it. Then turn it on for one edge, watch the log line `quarry.browser.actionability_rejected` (fields `reason`, one of the seven in the closed list, and `intent`, `click`, `type` or `select`; nothing else) and compare the rate per reason with the rate of `TARGET_REPAIR_REQUIRED` before. Split the rate by `intent` and by the `covered_by_tag` of the error. A rise in `target_covered` on `click` is the feature working (a banner the old click clicked through). On `type` and `select` it can also be a styled field or select with a transparent overlay or a decorative icon over the middle of its visible box, which the check now refuses (the old setter ignored what was on top), and that looks the same as a modal in the count: a `type` or `select` rate that is high on one site, with the same tag each time (`span`, `svg`, `i`), is that page's overlay and not a modal. a rise in `target_not_visible`, `target_has_no_box` or `target_unreachable_after_scroll` on a page that used to work is a page where the old click reached something the new check will not, and is the thing to look at before the next edge. Turning it off needs only the variable and a restart; nothing is stored. The variable is removed one release after the flag is on by default.

**ZDR.** Nothing is stored. The error carries a reason and a tag name, the log line a closed reason and intent, so the ZDR log filter and `log_field_discipline` have nothing to drop or flag.

**Evidence in this tree (WS3.K, 2026-10-21).** Run in `quarry-rust-env:1` (Chromium) with `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium` and `--test-threads=1`: `cargo test -p quarry-browser --features chromiumoxide`, lib 172 passed, 0 failed, 4 ignored (the lane was 138 passed, 4 ignored; the 34 more are 9 unit and 25 live-file tests of this change: 160 after the first three commits, 168 after round 6, which added one unit test and seven live tests, and 172 after round 7, which added four live tests; all the live-file tests need Chromium except `driver_capability_follows_the_flag`), `tests/ssrf.rs` 4 passed, 1 ignored. The live tests (`set_content` fixtures, flag on unless named): `trusted_click_has_istrusted_true`, `flag_off_clicks_synthetically_and_ignores_the_overlay` (the control), `covered_button_fails_target_covered_with_tag`, `offscreen_button_is_scrolled_then_clicked`, `disabled_button_fails_target_disabled`, `display_none_fails_target_not_visible`, `moving_button_fails_unstable_then_succeeds_once_settled`, `a_target_that_settles_while_waiting_is_clicked`, `type_replaces_value_and_emits_trusted_input_event`, `select_passes_the_gate_then_sets_the_option`, `click_opening_an_alert_returns_with_the_dialog_pending`, `child_frame_targets_fall_back_to_synthetic`, `download_targets_still_use_the_synthetic_path`, `driver_capability_follows_the_flag`; round 6: `a_click_landing_on_a_descendant_passes_the_hit_test`, `a_control_under_its_own_label_passes_the_hit_test`, `shadow_roots_and_slots_are_hit_tested_through`, `type_and_select_are_refused_under_an_overlay_and_while_moving`, `type_replaces_the_text_of_a_contenteditable_element`, `typed_values_for_date_and_range_use_the_setter_and_other_inputs_are_refused`, `a_second_ref_action_with_a_dialog_pending_returns_within_the_bound`. Also green: `cargo test -p quarry-core --lib error` (3), `cargo test -p quarry-runtime --lib observation` (37, including `effectful_actions_are_unknown_without_a_postcondition` and `changed_target_fingerprint_blocks_snapshot_action_before_click`), `cargo test -p quarry-edge --test zdr_behavior` (4) and `--test log_field_discipline` (22), `cargo check --workspace --all-targets` (only warnings that were there before).

**Round 7 (verifier findings, tests and wording only, no behaviour change).** Four live tests: `a_slotted_child_of_the_target_passes_through_the_assigned_slot` (negative control: with `node.assignedSlot ||` removed from the ancestor climb it fails with `target_covered`, `covered_by_tag: span`; restored), `a_cover_in_the_same_closed_root_as_the_target_is_not_detected` (the limit above, pinned), `a_node_that_leaves_between_resolve_and_probe_is_not_attached` and `a_handle_whose_realm_was_destroyed_by_a_navigation_is_not_attached`. The soak rule now says to split `target_covered` by intent and tag.

**Round 6 (reviewers' findings).** Fixed: the frame lookup of `input_mode_for` ran outside the `Op::NativeAction` deadline (above); type and select did not run the stillness and cover checks that the docs promised, and now do (`type_and_select_are_refused_under_an_overlay_and_while_moving`); a target inside a closed shadow root was reported as covered by its host and is now accepted; the `settable` set was wider than documented (every input but read-only) and is now the text-like and valued types listed above; the rotated-box unit test now pins the exact centroid of the clipped polygon (42.745098..., 100) and checks two more rotated boxes by point-in-polygon (`rotated_boxes_are_cut_to_the_viewport`). Added live tests for the branches that had none: a descendant hit, a label-forwarded click and a label for another control, open and closed shadow roots, a slotted target, a cover inside an open shadow root, a contenteditable field, the value route for date, range and colour, and the refusal of a checkbox and a submit input. Counts for the round-3 commit `d5a160eb`: its message says "+6 unit and +14 live"; the exact split is +8 unit (6 in `actionability.rs`, 1 in `deadlines.rs`, 1 in `lib.rs`) and 14 new live-file tests, of which 13 need Chromium (`driver_capability_follows_the_flag` does not): 138 + 8 + 14 = 160. The commit cannot be amended; this is the correction.

**Not done, and why.** No rebuilt edge was run with the flag on: the plan's evidence (a public page with a modal over a link, 409 `target_covered`, the close ref returns 200, the link then returns 200 with a changed `url`) needs a deployed edge and was not possible in this session; the fixtures above are what backs the claim. The flag has not been flipped anywhere. The Puppeteer behaviour that the plan marks `[UNVERIFIED]` (a click that opens a dialog stalls the input acknowledgement) is verified here only for this Chromium build. Cross-process iframes, modifiers, drag, paste, per-key typing and file-chooser interception are WS3.L. A client that sends `trusted_input` inside `driver_requirements` has it ignored: the field is reported, not requestable.


## Block escalation: `QUARRY_BLOCK_ESCALATION` (WS3.I, ADR-5)

One switch, read once when the process starts (a change needs a restart of the edge, which is also the rollback). The edge logs the mode once, as `quarry.block_escalation.mode` (field `mode`). Quarry classifies a block and refuses; neither mode changes a user agent, a TLS fingerprint or a header.

| Variable | Values | Default | Effect | Roll back |
| --- | --- | --- | --- | --- |
| `QUARRY_BLOCK_ESCALATION` | `legacy`, `honest` | `honest` (the build default in `escalation.rs::BUILD_DEFAULT`; a blank value is the default; any value that is not `legacy` is `honest`, so a typo can only refuse more) | `legacy`: the behaviour before WS3.I. `honest`: a status never changes the identity Quarry presents, table below. | Set `QUARRY_BLOCK_ESCALATION=legacy` and restart. |

| Path | `legacy` | `honest` |
| --- | --- | --- |
| Static fetch through a proxy pool, the origin answers 401, 403, 407, 429 or 503 | the proxy is marked unhealthy for 60 s and the request rotates to the next approved proxy (up to 3) | one attempt: `UPSTREAM_BLOCKED` (or `RATE_LIMITED` for 429) with one `egress_attempts` record (`reason: block_status`); the proxy stays healthy and the next request to the host starts on the same proxy |
| Static fetch, connect error, timeout, reset | the proxy is marked unhealthy and the request rotates | the same |
| `FallbackDriver`, a block status from a driver | the next driver in the chain (Chrome-TLS, browser) is tried; if every one is blocked the last block response is returned | with no shell held, the block response itself is returned as it is and `served_by` is the driver that got it. With a JS shell held (earlier driver), the shell is returned instead of the block response: with the production chain `[static, Chrome-TLS, browser]`, a shell from the static driver followed by a 403 from Chrome-TLS returns the shell (the 200 static response, `served_by` static), the same outcome as a Chrome-TLS error after a shell; the browser is not called and the block body is not read as "thinner than the shell, so rotate". The caller therefore sees a 200 and the block is no longer visible to status-based back-off: the autoscaled pool (`record_ok`, `pipeline.rs`) and the per-host scheduler do not slow down for it, and the `warn` log "block or challenge refused under honest after a JS shell" is the only trace. A host that blocks the Chrome-TLS identity is not slowed down by this path (the same trade-off the Chrome-TLS error arm already makes); a status-carrying signal on the response is not built (not done) |
| `FallbackDriver`, `UpstreamBlocked` or `RateLimited` as an error (what the static driver makes of a proxied 403) | falls through to the next driver | the error is returned; `Timeout` and `DriverFailed` still fall through |
| `FallbackDriver`, a 200 page that is a JS shell, an unfollowed redirect | rescued by the browser driver | the same (content, not a status) |
| `FallbackDriver`, a 200 page that is a strong challenge (an interstitial or a CAPTCHA widget, classified by `challenge.rs`) and also looks like a JS shell | rescued by the browser driver, which can run the challenge script | the page is returned as it is (the scrape path classifies it); a challenge never changes identity. When the challenge comes from the Chrome-TLS driver behind a static shell, the shell already fetched is returned and the browser is not called. Only the strong class counts: a weak hint or a plain login page is still a shell and is rescued |
| `signals.prior_block_signals` on `/v1/scrape` and the other planned routes | 1: TLS driver, 2 or more: browser (and the page cache is bypassed) | accepted and ignored: the plan comes from `actions`, `screenshot`, `pdf` and `profile_required` only |

`is_block_status` stays: it still feeds the slow-down in both modes. The autoscaled pool backs off on a block status (`pipeline.rs`), and the per-host scheduler backs off on a block-class error; a block that comes back as a direct-egress response (no proxy) is still recorded by the scheduler as a good fetch, as it was before WS3.I (not changed here). The attempt label `block_rotation` (in `details.egress_attempts[].decision_reason`) is `health_rotation` in both modes: a later attempt in the plan is a health ordering, not a reaction to a block. `GET /v1/presets` lists `careful` (the preset that was `stealth`, now with Block `{challenge, abort}`) and the deprecated alias `stealth` (`alias_of: careful`, `deprecated: true`); `GET /v1/presets/stealth` answers with `Deprecation: true` and `Link: </v1/presets/careful>; rel="successor-version"`, and `POST /v1/jobs {"preset": "stealth"}` keeps working. The alias is removed one release after the rename (`deprecatedPresetAliases` in `presets.go`).

**Known limit.** The static driver reports a connect failure as `UPSTREAM_BLOCKED` (a public code mapping that predates this switch). Under `honest` the error path treats that code as a block, so a connect failure no longer falls through to the Chrome-TLS driver either; the same failure still rotates between proxies (inside the static driver, before the error exists). The fail-closed side of an ambiguity the code cannot resolve. Pinned by `honest_connect_failure_on_the_static_driver_does_not_fall_through_to_tls` (`fallback_driver.rs`: a single dead proxy, `honest` returns `UPSTREAM_BLOCKED` with the Chrome-TLS driver not called, `legacy` lets it serve); changing the mapping must change that test.

**Where the default stands.** The mechanism shipped first with the default `legacy` (`61906c93`); the default is now `honest`, flipped in the last, separate commit of the slice on ADR-5's approved default (`escalation.rs::BUILD_DEFAULT`, gated by the test `build_default_matches_the_documented_default`; the same commit changed the page-cache test, whose `prior_block_signals 2` bypass case now exists only in legacy mode). The plan asks for a before and after measurement of scrape success on the pages a TLS or browser tier used to rescue before the default changes. No such measurement was possible on this branch (no live origins, no baseline of rescued pages), so the flip rests on the tests listed in `docs/PROGRESS.md`, not on a measurement; do not read it as one. A running edge changes mode only when it is rebuilt and restarted. Before relying on `honest` in production, compare the rate of `UPSTREAM_BLOCKED` and `RATE_LIMITED` per host class with the rate before; pages a TLS or browser tier used to rescue after a 403 now come back as that block. `QUARRY_BLOCK_ESCALATION=legacy` plus a restart is the rollback.

**Page cache.** The cache gate follows the plan: under `honest` a request with `prior_block_signals` of 2 or more has a static plan, so it reads and writes the page cache like any default scrape (under `legacy` it selected the browser and bypassed the cache).

**In-repo callers of `prior_block_signals` (audited).** Two producers used the field as a request for a tier. (1) Model Plane `model-gateway` (`DriverSignals::browser()`, the one retry of `scrape_readable`) asked for a browser render when a plain fetch returned no readable text. That is meant as a content condition, so it now sends `profile_required: true` (browser in both modes, page cache bypassed in both) and `prior_block_signals: 0`; the wire pin tests in `model-gateway/src/quarry.rs` were changed with it. It is a content condition only for a 2xx page: a direct-egress block (a 401, 403, 407, 429 or 503) comes back from the edge as a normal `{data:{status:N}}` envelope, not an error, and the first version of this change re-fetched such a page through the browser, a status-driven change of identity. `scrape_readable` now returns a block-status page as it is (`readable_scrape_does_not_escalate_a_block_status_page`); a 200 interstitial has no marker in the scrape output, so the client cannot tell it from an empty shell (the edge refuses it with `CHALLENGE_DETECTED` when `QUARRY_CHALLENGE_ENFORCE=enforce`, which arrives as an error and is not retried). An edge from before this change still honours `profile_required`. (2) The verevonv3 gateway's `fetch_listing_markdown` retry sends `prior_block_signals: 1` to reach the TLS driver after a failed plain extract. Under `honest` there is no TLS tier to reach: that retry now repeats the same static fetch (one extra origin request, same result). This is a known regression of the flip, not fixed here (the TLS tier is an identity change by design; the gateway is another plane's code). Pages that only the TLS tier rescued after a block come back as `UPSTREAM_BLOCKED` or `RATE_LIMITED`; `QUARRY_BLOCK_ESCALATION=legacy` restores them. SDK consumers that send the field get the same: accepted, ignored.

**`careful` and a challenge.** The preset records Block `{challenge, abort}`, and its description says the policy is advisory: no runtime reader acts on `Block.on` / `Block.action` (`polite` has the same property with `escalate`). What is enforced is narrower and in `challenge.rs`: `QUARRY_CHALLENGE_ENFORCE=enforce` refuses a scrape of a bot wall and an agent action on an exhausted host. Wiring Block.action into the run loop is not done.

**Not covered here, and why.** No rebuilt edge was run against a public 403 origin with a two-entry `QUARRY_PROXY_POOL` (the plan's evidence step): that needs a deployed edge and a stable public 403 origin and was not possible in this session; the wiremock tests (`fetch::tests::honest_mode_never_rotates_on_status` and the others) stand in for it. The evidence command, for whoever has them: `QUARRY_BLOCK_ESCALATION=honest` with two proxies, `POST /v1/scrape` of the 403 origin, then `docker logs quarry-edge | grep -c "rotating"` is 0 and the error carries one `egress_attempts` record. One pre-existing trap, not fixed here: `attempt_record` in `fetch.rs` puts the proxy URI and the raw transport error text into `details.egress_attempts`, so a pool URI with userinfo would be echoed to the caller.

## ZDR posture of a scheduled change monitor: stamped at create (WS4.H, C1, quarry-control migration 023)

**What changed.** A change-monitor schedule now carries the Zero Data Retention posture of whoever created it, and the orchestrator declares it on every `/v1/internal/change/record`. This is option (a) of "What the orchestrator lacks to send `zdr: true` on the change record (V4)" above, built; that paragraph and the sentence in the stage-A section that says the Go side "does not send it yet" are history (the flag default is unchanged and no matrix row moved to `Enforced`).

**The path, one value end to end.**
1. The edge computes `claims.effective_zdr(req.zdr.unwrap_or(false))` once in `create_schedule` (`POST /v1/schedules` with a `preset`) and in `create_source` (`POST /v1/sources` with `monitor: true`; a source without `monitor` creates no schedule and is not touched). That one value is the argument of `zdr_refusal::refuse` and the stamp on the forwarded body (`ControlScheduleCreate.zdr`, always serialised; `CreateSourceRequest.zdr`). A client body may declare `zdr: true`, which raises the stamp; it can never lower the floor of a user token (the one rule of `Claims::effective_zdr`). The declaration is not forwarded as sent: the legacy create path drops it (`skip_serializing`) and a source forwards the effective value only for `monitor: true`.
2. quarry-control stores it in `schedules.zdr` (migration `023_schedules_zdr.sql`: `ADD COLUMN IF NOT EXISTS zdr BOOLEAN`, nullable, no default) and puts it in the `ChangeMonitorWF` args (`changeMonitorArgs.Zdr *bool`, `omitempty`). NULL (a row created before the stamp) and an absent field mean "nothing declared"; `false` is "stamped standard".
3. The orchestrator's `ChangeMonitorInput.Zdr` goes to `CheckChangeInput.Zdr`, to `changeRecordRequest.zdr` and to the `RunPageInput.Zdr` of the page fetch that comes first in `CheckChange` (`zdr` on the `/v1/internal/run_page` body; the edge runs that fetch in ZDR mode, so no page artifact, durable page event or local-index write-back is made for a stamped-ZDR monitor; an unstamped schedule, a job and a crawl send no `zdr` key there). The reconciler passes the args through as generic JSON, so a changed row reaches Temporal on the next reconcile pass (every pass calls `update`).
4. The edge's `record_internal` refuses a declared `zdr: true` (flag on: 403 `zdr_durable_mutation_forbidden`, which the activity already turns into a clean `zdr_refused` run) or logs `zdr.would_refuse` (flag off: the baseline is stored as before).

**Deploy order.** quarry-control (migration and code), then the orchestrator, then the edge. Every new field is optional on every decoder (Go ignores unknown keys, serde has no `deny_unknown_fields` here), so any order is safe. Temporal replay is unaffected (a new optional field in the inputs, no new commands).

**Visible change with the flag off (owner decision).** For a monitor created after this deploys by a ZDR organization, `emit_change_side_effects` now suppresses the edge's NATS change event and webhook (it already did so for any declared `zdr: true`, in both modes). That is the existing ZDR-correct rule and it is not gated on the flag. The same happens for a monitor that a user of a STANDARD organization creates with `zdr: true` in the body (a declaration raises the stamp): its record route is fail-closed, so its edge NATS change event and webhook are suppressed, and with the flag on the create itself is refused. Monitors created before the deploy (NULL) behave exactly as before. The suppression is pinned by `zdr_posture_stamp::a_declared_zdr_change_record_suppresses_the_change_event_and_the_webhook`, which calls the fan-out (`change_webhook::emit_change_side_effects`, moved there from `change_routes.rs` in round 7 so a test can reach it without a Postgres baseline store; same code, same single caller `record_check`) with a `Changed` record: a standard posture gives one durable event and one signed webhook (the control that proves the test can see them), a ZDR posture gives none. The record tests of this file stop at 501 because the harness has no baseline store, so they alone never reached it.

**Known gaps. Each is pinned by a test or stated here.**
- (a) An organization that turns ZDR on later keeps the stamp its schedules were created with, so those schedules keep firing and storing baselines. Pinned by `zdr_posture_stamp::record_trusts_the_stamp_not_the_current_posture`. Operator remedy in quarry-control's database: `UPDATE schedules SET zdr = true WHERE org_id = '<org>';` (the next reconcile pass propagates it). The permanent fix is a fire-time posture lookup (plan X-9, a Control Plane registry row for the orchestrator), which is not approved.
- (b) Not a gap. A service principal cannot create a change monitor at all: `require_service_route_scope` fails `/v1/schedules` and `/v1/sources` closed for every service (pinned by `zdr_posture_stamp::a_service_principal_cannot_create_a_monitor_in_either_mode`), so every stamped schedule comes from a user token. (An earlier version of this section listed service-stamped-false here; that case is unreachable.)
- (c) The legacy non-preset create path is not stamped. `workflowFor` maps no workflow for it, so it never fires.
- (d) With the flag on, a ZDR organization cannot create a monitor, so a `zdr = true` row can only come from an observe-mode create (flag off) or from the operator remedy above.
- (e) The URL of a refused run is still durable: `ChangeMonitorWF` emits `run_started` and `change_detected` with the URL in the payload, and the URL also sits in the Temporal args and history. Unchanged here and pinned by `TestChangeMonitorWF_ZDRRefusalEndsTheRunCleanly`. The page CONTENT is not: the run_page fetch carries the stamp (step 3, pinned by `TestCheckChangeSendsStampedZdrOnTheRunPageFetch`). A schedule stamped before this fix, or one that never got a stamp (NULL), still fetches with ZDR off, and so does the job path (RUNBOOKS line 101, unchanged).

**Check what is stamped.** In quarry-control's database: `SELECT zdr, count(*) FROM schedules GROUP BY zdr;` (NULL are the pre-stamp rows). With the flag off, `message = "zdr.would_refuse"` and `row = "schedules"` or `"change_record_internal"` lists the organizations an enforcement would affect; the stamp makes the second row non-empty for monitors created after the deploy.

**Matrix.** `schedules`, `profiles`, `change_history`, `change_snapshot` and `change_record_internal` stay `Gap`: the default of `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` is still `false` (ADR-4 stage B needs the owner's sign-off and the customer notice), so a ZDR organization is still served. Only their `issue` and `basis` texts changed, and the `control_records` basis now says `POST /v1/sources` with `monitor: true` is classified by the `schedules` row. `zdr_gap_rows.rs` is not inverted.

**Tests.** quarry-control: `TestCreateScheduleStampsZdr`, `TestUnstampedScheduleArgsAreUnchanged`, `TestWorkflowForCarriesZdr`, `TestListScheduleCarriesStampToReconciler`, `TestSourceMonitorCreateStampsZdr`, `TestSourceWithoutMonitorIgnoresZdr`, `TestMigration023Text`, and (with `QUARRY_TEST_DSN`) `TestPgScheduleZdrRoundTrip` and `TestMigration023IsIdempotent`. The Postgres tests run in CI in the gating job `quarry-control Postgres tests` (`quarry-v2-ci.yml`, a postgres:16 service, `QUARRY_REQUIRE_DB=1` makes a missing DSN fail instead of skip); locally they skip without `QUARRY_TEST_DSN`. Orchestrator: `TestCheckChangeSendsStampedZdr`, `TestCheckChangeOmitsZdrWhenUnstamped`, `TestChangeMonitorWFPassesZdr`, `TestChangeMonitorInputDecodesLegacyArgs`, `TestCheckChangeSendsStampedZdrOnTheRunPageFetch`, `TestRunPageOmitsZdrWhenUnstamped`, and in `internal/schedules` `TestReconcileCreateCarriesTheZdrStampIntoTheTemporalArgs` and `TestReconcileUpdateCarriesTheZdrStampIntoTheTemporalArgs` (the control list payload to the Temporal action args, create and update). Edge: `crates/quarry-edge/tests/zdr_posture_stamp.rs` (13, both flag modes, including a declared `zdr: true` from a standard user on `/v1/schedules` and `/v1/sources`, a service principal refused on both, a legacy cron create that never forwards a declared `zdr`, the change event and webhook suppression above, and `run_page_with_a_zdr_stamp_persists_no_artifact_event_or_index_write`, which posts to `/v1/internal/run_page` with the runtime token and the HMAC org binding and counts artifact files, durable events and local-index writes: positive for an unstamped and a `zdr: false` body, unchanged for `zdr: true`) and the unit test `control_schedule_create_serializes_store_schedule_shape`. The Go tests of the chain (quarry-control `internal/resources`, orchestrator `internal/activities`, `internal/workflows`, `internal/schedules`) gate a merge in the job `quarry Go ZDR stamp chain` (`quarry-v2-ci.yml`, no `continue-on-error`); before round 7 only the report-only `go-coverage` job ran them. A required check needs branch protection to take effect, which this repository does not show.

### WS4.H history notes (for whoever reads the log of this section)
- `8b3c108b` moved the pinned digest of `cross-plane-token-requests.json` in `scripts/tests/zdr-contract-sync.test.sh` (a hunk of WS1.A2 part 1 that was in the shared working tree), but the golden itself landed in `79330ea1`. So `scripts/tests/zdr-contract-sync.test.sh` is red for that file at `8b3c108b` and `82225bf2` and consistent from `79330ea1` on. The branch is local and unpushed; no CI ran those commits. The script's own summary at round 7 is `65 passed, 0 failed` (the count moves whenever another unit adds a row; re-run before quoting) with seven `pending -` lines (six vendored `zdr-posture-matrix.json` rows and the mp-scrub `secret-shapes.v1.json` row), not one as `8b3c108b`'s message says.
- `efebf3e0` also carried WS3.I's RUNBOOKS hunks (the `QUARRY_BLOCK_ESCALATION` default row and "Where the default stands" now saying `honest`, and a "Page cache" paragraph). The code default flipped in `89097598`, so the document said `honest` while the code still said `legacy` between those two commits; both agree at HEAD.
- Corrections to wording in earlier WS4.H commits. (1) `2a7759f6`, `06959092`, `8b3c108b` and `efebf3e0` say a service principal's schedule is stamped `false` and list it as a known gap. That case cannot occur: every service principal gets 403 on `/v1/schedules` and `/v1/sources` before the handler runs (`require_service_route_scope`). The code was always fail-closed; only the sentences were wrong. (2) `06959092` says a client body can neither lower nor raise the stamp, and `efebf3e0` says a client body has no field that sets it. Since the next commit a body's `zdr: true` raises the stamp for a user of a standard organization (the floor of a ZDR user token still cannot be lowered), on both `/v1/schedules` and `/v1/sources` with `monitor: true`. (3) `8b3c108b` says `52 passed, 0 failed (1 pending row)` for the sync script: that run was on the shared tree; the script has seven pending rows, and the commit's own tree was red on the cross-plane pin (first bullet). (4) `65ccf059` says the run passed with `zdr_posture_stamp (11)`, and the RUNBOOKS line added by `c4510d88` says `edge file: 11`. The file had 7 tests at every commit from `06959092` to `c4510d88` and 11 from `96c81447`: those runs measured the shared working tree, which already held the uncommitted edge change that `96c81447` later landed. The file has 13 tests from round 7. Quote a count only from a run on a clean checkout of the commit.

## The browser grant validator in production (WS1.A2 part 2)

Production (`ENVIRONMENT=prod|production`) never starts a browser without a BrowserBroker grant, and every check of that grant (at the start of a run, on each step, when a tab is opened and on a sensitive approval) is made with a service token minted for the verified run's organization.

**Boot rules** (`crates/quarry-edge/src/grant_boot.rs`, one pure function, unit-tested; the real binary is tested in `tests/browser_grants_prod.rs`). The edge refuses to start, naming the variable, when:
- grants are required (production, or `QUARRY_EDGE__REQUIRE_BROWSER_GRANTS`) and no validator endpoint is set;
- it is production and only `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_URL` (the HTTP shim) is set. The shim posts to `/v1/broker/grants/{id}/validate`, which no Model Plane service serves. Outside production it stays available as a development compatibility path and logs a warning;
- `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` is set without `QUARRY_EDGE__QUARRY_SERVICE_API_KEY`. The only exception is the explicit dev bypass (`QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS=1`, refused in production) with the static `QUARRY_EDGE__MODEL_PLANE_TOKEN`;
- the gRPC address is set in a binary built without the `grpc` feature.
The gRPC connection is made at boot (a placeholder address does not boot: `grpc connect failed`) and the manifest row `browser-grant-validate` is parsed at boot, so a bad row or an unreachable broker stops the edge instead of failing the first run. With no endpoint at all and no requirement (a local stack) grants are not enforced and the edge logs a warning.

**How a check is made** (`GrpcGrantValidator`). The token is the manifest row `browser-grant-validate`: audience `browser-broker`, scope `browser:read` only, minted at `POST {AUTH_CORE_URL}/api/browser-broker/internal-token` for the run's organization and cached per organization (300 s token, refreshed 30 s early). The broker looks the grant up inside the token's organization, so another organization's grant is `NotFound`. The org-blind `validate` is refused (Forbidden) once service tokens are configured; `revoke` is refused too, because no principal holds `browser:write`. Broker answers:
- `NotFound` and `FailedPrecondition` (the broker's answer for a revoked or expired grant) are an inactive grant, so the caller answers 403 `browser grant is inactive or expired` (SECURITY_BLOCKED), never a 5xx (G6);
- `Unauthenticated`: the cached token is dropped, one fresh token is minted and the call is made once more; a second refusal is Forbidden (403);
- `PermissionDenied` is Forbidden (403), not retried;
- `Unavailable`, `Internal`, `DeadlineExceeded`: up to three attempts (50 ms then 200 ms apart), then `DRIVER_FAILED` (502): fail closed;
- Auth Core refusing or failing the mint is returned as it is (403 or an upstream error) and the broker is not called.
The service token is not bound to the grant's owner: owner binding stays Quarry's (`bind_validated_grant`), pinned by `known_gap_a_service_principal_is_not_bound_to_the_grants_owner`.

**Deploy order.** auth-core with the registry row first (`bash "apps/Control Plane/scripts/run-control-plane.sh" up -d --no-deps --build auth-core`: until it serves `quarry-edge` on audience `browser-broker`, every grant check in production is a 403), then broker and model-gateway, then quarry-edge. The base compose carries `QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL: ${QUARRY_EDGE_BROWSER_GRANT_VALIDATOR_GRPC_URL:-}` (empty is a development stack); the production overlay requires it (`http://browser-broker:9095` on `inter-plane-bus`).

**The contract overlay** `apps/Ingestion Plane/docker-compose.grant-contract.yml` is test-only (SC1 L2). It sets two environment keys and nothing else; `scripts/tests/grant-contract-overlay.test.mjs` renders base, production and contract with `docker compose config` and fails when the contract render lost or changed any production hardening key (`read_only`, `cap_drop`, `tmpfs`, `security_opt`, `ports`, `ENVIRONMENT`, both dev bypasses, `QUARRY_BROWSER_NO_SANDBOX`, the durable-history flag, the artifact volume). The live leg is `bash scripts/tests/browser-grant-prod-contract.sh` (preflight, three Playwright phases around a per-hop log scan; `--boot --yes-rebuild-stack` rebuilds the stack); it has not been run on a stack from this branch.

**Tests.** quarry-runtime `grpc::browser_broker` (with `--features grpc`; a scripted gRPC broker and a fake Auth Core), quarry-edge `grant_boot` and `tests/browser_grants_prod.rs` (the `prod_*` tests; the ones that use the real validator need `--features grpc`, the CI job `browser-grant-contract.yml` runs them with it).

## WS6.A, the WS5 residuals and WS2.E: the record of round D3 (2026-10-21)

- **Rust, `quarry-edge`, the whole test set** (`cargo test -p quarry-edge --no-fail-fast`, run through `qtest.sh`: Linux, image `quarry-rust-env:1`, `CARGO_BUILD_JOBS=2`, on the shared worktree). 29 test-result blocks (unit, integration and doc tests): 1016 passed, 0 failed. The new `zdr_claim_floor` test `a_zdr_scrape_of_a_pdf_returns_inline_text_and_persists_nothing` is among them, and that binary passes 23 of 23. Other units' uncommitted test hunks were in the tree during the run.
- **Go, native** (`go test ./... -count=1 -v`, go1.26.5): `services/quarry-orchestrator` 106 passed, 0 failed. `services/quarry-control` 228 passed, 0 failed, 15 skipped: the Postgres integration tests, which need `QUARRY_TEST_DSN` (the required CI lane runs them). `pkg/quarrycontracts` 35 passed, 0 failed.
- **Node contract tests** (`node --test scripts/tests/*.test.mjs`): 23 run, 23 pass, before and after the `openapi.yaml` edit.
- **OpenAPI.** `docs/openapi.yaml` parses with `gopkg.in/yaml.v3` v3.0.1 (50 paths, 147 schemas). `ScrapeRequest` carries `x-quarry-lenient-until`: the lenient period ends at `QUARRY_EDGE__SCRAPE_STRICT_AFTER`, which the Ingestion compose file passes through with an empty default.
- **Unwrap audit of the WS5 scrape path.** `awk -f scripts/unwrap-audit.awk <files>` reports `unwrap()`, `expect(`, `panic!`, `unreachable!`, `todo!` and `unimplemented!` outside `#[cfg(test)]` items. Positive control: 3 planted production calls reported, 2 planted test calls not reported. Over the 13 files of the scrape path (`quarry-edge`: `scrape_routes`, `scrape_contract`, `routes`, `extract_routes`, `limits`, `handoff`; `quarry-runtime`: `content_route`, `pipeline`, `doc_parse_main`; `quarry-transform`: `pdf`, `docx`, `sniff`; `quarry-core`: `scrape`) the audit reports 0 production hits; the 196 matching lines in those files are test code. The same audit on the gateway's `knowledge/quarry.rs` and `search.rs` and on the model-gateway's `quarry.rs` and `grpc.rs` reports 0. No annotation was needed.
- **WS6.A crawl bound.** `docs/CONTRACTS.md` section 9.4 states the defaults, the ceilings, `data.limits`, the job params and the workflow's `GetVersion` fallback. The recipe `scripts/ws6a-crawl-bound-evidence.sh` passes its `--self-test` (no network: the four values match and a short job is refused, as expected). The recipe itself is **NOT RUN**. It needs an exit-gate stack started with `QUARRY_CRAWL_DEFAULT_MAX_PAGES=7`, a documented public seed with more than 7 internal links, and a cookie file for a seeded dev account. It must not be run against the live stack.
- **WS6.A, still open (stack evidence).** The 7-page crawl; the pre-fix capture on an image built before `5995eeca`; the recorded-history replay of `CrawlJobWF`. `TestCrawlJobWF_OldVersionHistoryKeepsLegacyBehaviour` still uses the `OnGetVersion` mock. The pinned SDK, `go.temporal.io/sdk v1.29.1`, has that mock and the worker replayer; the replay needs a history captured from a real run.
- **WS5.B `/v1/extract`.** Explicitly deferred: `docs/CONTRACTS.md` section 9.5 gives the reason and the next step. Not built.
- **WS2.E, the SPA** (a scratch copy under the unit's scratchpad; the worktree got no install). The copy used `pnpm install --frozen-lockfile` with the worktree's lockfile (pnpm 11.3.0). Two workspace packages had to be built in the copy first: `@verevon/remote-core` (`tsc -p tsconfig.build.json`) and the Quarry SDK (its `dist`, copied into the installed `file:` package). Results:
  - `tsc -b` (the typecheck script): 12 errors, all TS2305 in `src/shared/api/quarry-client.ts` (SDK model names). None in `src/app/App.tsx`. The census baseline was 33 errors; in the copy they went 33 to 21 when `remote-core` was built, and 21 to 12 when the SDK was built. The remaining 12 come from the SDK's own tsconfig, which TypeScript 6 rejects (TS5108); the copy built it with `--rootDir src --outDir dist`. Not chased further.
  - `FleetRunConsole.test.tsx`: 1 file, 1 test, passed.
  - `vite build` (the `pnpm build` script stops at `tsc -b`, so the chunk check ran on `vite build` directly): exit 0, 188 JS assets, no `FleetRunConsole` string and no `agents/fleets` string in `dist`.
  - Full `vitest run`: 322 of 326 files pass; 3626 of 3632 tests pass. The 6 failing tests in 4 files need a repository root or a sibling plane's file that the copy does not have (`TRUST_REPO_ROOT`, `gdpr.go`, a Model Plane file). They are not in WS2.E's files.
  - No App route harness exists (no test file under `src/app`), so no route test was added, as the plan's condition requires.
- **Not run in this round.** The WS5.B deployment probes (a) to (f) and the staging re-ingest check; the WS5.C, WS5.D, WS5.E and WS5.F probes; the WS2.E container check (`/agents/fleets/abc` on the restarted SPA container); the WS5.H throwaway-failure demonstration and the `quarry-v2-ci` run (CI).

## ZDR run tombstone: built, OFF, and gated on counsel review (WS4.J, C2, quarry-control migration 015)

**Status in one line.** The tombstone is built and tested to the counsel gate. It is OFF by default, no env file, compose file or default turns it on, and **counsel review gates ENABLING it, not building it**. Nobody should set `QUARRY_EDGE__ZDR_TOMBSTONE` outside a side-by-side test stack until that review is recorded.

**What it is.** When a run ends under Zero Data Retention, the edge may send quarry-control a metadata-only record that the run happened: `POST /v1/internal/zdr-tombstones`, HMAC-signed like the change webhook, stored in `zdr_run_tombstones` (migration `015_zdr_run_tombstones.sql`). It exists so an operator can answer "did this organization run anything under ZDR, when, and how did it end" without the run having left content behind. It is a **new egress path under ZDR and it retains the fact that a ZDR run happened**. That is metadata retention that a customer's ZDR terms may or may not allow, which is why counsel gates enabling it.

**What it may carry (the allowlist).** `crates/quarry-runtime/contracts/zdr-tombstone.schema.json` (pinned in `scripts/tests/zdr-contract-sync.test.sh`, vendored for quarry-control under `internal/resources/testdata/`): `run_id`, `org_id` (`^[A-Za-z0-9_-]{1,64}$`), `kind` (`scrape`, `search`, `answer`, `extract`, `agent_run`), `retention_class` (always `zdr`), `outcome` (`ok`, `error`, `refused`, `cancelled`, `timeout`), `driver` (`static`, `browser`, `tls`), `started_at`, `ended_at`, `step_count`, `cost_units`, `error_class` (an `ErrorCode` wire name or a transport class, never a message). No URL, host, title, query, user id, content, digest, fingerprint or free text, and no key named `zdr` (`change_webhook.go` rejects a truthy `zdr` and that gate stays). Four places repeat the list and a test compares each to the schema: the edge struct (`tests/zdr_tombstone.rs`), the Go wire struct (`TestZdrTombstoneStructMatchesSchema`), the migration's columns and CHECK lists (`TestMigration015MatchesTheAllowlistSchema`), and the Go store's vocabularies.

**How it is closed.**
- **Emission only under ZDR, fail open, bounded.** `emit_run_end` and `Pending::begin` return at once for a standard run. Emission is a spawned task with a 2 s timeout behind a 64-permit semaphore; with the permits gone the tombstone is dropped and counted, never awaited. A lost tombstone loses audit evidence and enforces nothing; it can never fail a request. The log carries a class or a status, nothing else.
- **Where it fires.** `POST /v1/scrape` (at the end of `scrape_with`), `POST /v1/scrape/stream` (inside the runner task), `POST /v1/search`, `/v1/answer` and `/v1/extract` (a wrapper around the handler that reads the response status, so every return path is covered), and agent runs from `finish_run`, which both the owner's DELETE and the sweeper use, after the `closed` check, so only the path that wins the removal sends. `POST /v1/answer/stream` is not covered yet (its end is inside the stream). A request refused before any work (400, 404, 405, 413, 422) is not a run and sends nothing; the class of a search/answer/extract tombstone comes from the status alone (502 is `DRIVER_FAILED` whether the cause was a driver or an upstream block).
- **quarry-control.** Mounted inside the HMAC group and only when the process has an HMAC secret; the handler also refuses a request without a signature header (the verifier lets an unsigned request through in rollout mode). 4 KiB body cap (over it: 400 `BAD_REQUEST`, there is no 413 code in the envelope), typed decode with unknown fields refused, any string containing `://` refused first, ids by pattern and classes by closed list. 204 for an insert and for a duplicate `run_id` alike (first write wins), so the route is not an existence oracle. There is **no HTTP read** of tombstones; `ListByOrg` is store-only.
- **The table.** CHECK constraints refuse a value outside the vocabulary whatever wrote it; `retention_class = 'zdr'` makes a standard row impossible; an `UPDATE` trigger refuses edits (INSERT ... ON CONFLICT DO NOTHING never reaches it, DELETE stays allowed for the sweeper and for erasure); no foreign key to any other table.
- **Retention.** `internal/janitor/tombstones.go` runs hourly and deletes in batches of 1000 by `ended_at`. `QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS` is 30 when unset; `0` deletes everything that ended before now (the evidence step uses it); a value that is not a non-negative integer logs and falls back to 30 and **never disables the sweeper**. `QUARRY_ZDR_TOMBSTONE_DRY_RUN` (true, 1, yes, on) counts and deletes nothing; the default is a real sweep. **This sweeper is deliberately not two-key armed like the artifact janitor** (`janitor.go` says that one is unsafe to arm without a provenance guard): it deletes metadata, nothing references a tombstone, deleting it is the privacy-safe direction, and with the edge flag off there are no rows to delete. That departure from the plan's "two-key-armed" wording is recorded here and in the plan ledger.
- **Org erasure.** `PurgeOrg` deletes the org's tombstones in the same transaction as its other rows (`PurgeResult.ZdrTombstonesDeleted`).

**Flags.** `QUARRY_EDGE__ZDR_TOMBSTONE` (edge, default off; needs `control_base_url` and the internal secret or it stays off and logs why), `QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS` (control, default 30), `QUARRY_ZDR_TOMBSTONE_DRY_RUN` (control, default off). `tests/zdr_tombstone.rs::no_file_in_the_workspace_turns_the_flag_on` fails if a compose, env or Dockerfile in the Quarry workspace sets it (and `git grep ZDR_TOMBSTONE` over tracked files finds no setting anywhere in the repository).

**Deploy order when it is eventually enabled.** quarry-control first (the migration and the receiver), then the edge with the flag. An edge with the flag on and an older control gets 404 from the receiver, counts the delivery as failed and carries on.

**Enabling it (not before counsel).** (1) Record the review (who, what the contract says, which customers' terms were checked) in the plan ledger. (2) On the target stack set `QUARRY_EDGE__ZDR_TOMBSTONE=1` on the edge. (3) Run one ZDR scrape and one ZDR agent run and check `SELECT run_id, org_id, kind, outcome, driver, error_class, ended_at FROM zdr_run_tombstones;` shows two rows, `pg_dump --table zdr_run_tombstones | grep -c "<a canary string used in the scraped URL>"` is 0, a standard scrape adds no row, and with `QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS=0` the sweeper empties the table within an hour. (4) Tell the affected customers: it is a new category of retained metadata.

**Not in this slice.** Enabling it anywhere; new catalogue keys; an HTTP read of tombstones; plan-based retention from Control Plane (a later API read, never its database); `answer/stream`; the legal compatibility of the metadata with each customer's ZDR terms, which is unknown.

## WS6.A, WS5 and WS2.E: round 4 corrections to the round D3 record (2026-10-21)

- **Go, native, re-run at HEAD `7849ff1b`** (`go test ./... -count=1 -v`, go1.26.5). The counts are every `--- PASS` and `--- SKIP` line, subtests included. `services/quarry-control`: 233 passed, 0 failed, 20 skipped. All 20 skips are Postgres integration tests, and each prints `QUARRY_TEST_DSN unset`. `services/quarry-orchestrator`: 106 passed, 0 failed, 0 skipped. `pkg/quarrycontracts`: 35 passed, 0 failed, 0 skipped.
- **This replaces the quarry-control figure of 228 passed and 15 skipped** in the round D3 section above. That run is dated 21:46 on 2026-10-21 and does not contain ten tests of the ZDR tombstone receiver: five passing (`TestSweepDeletesOnlyPastRetention`, `TestSweepDryRunDeletesNothing`, `TestSweepZeroDaysDeletesAll`, `TestTombstoneOptionsFromEnv`, `TestMigration015MatchesTheAllowlistSchema`) and five Postgres-gated skips (`TestPgZdrTombstoneStoreRoundTrip`, `TestPostgresOrgErasureRemovesTombstones`, `TestZdrTombstoneMigrationAppliesTwice`, `TestZdrTombstoneRowCannotBeUpdated`, `TestZdrTombstoneTableRefusesValuesOutsideTheVocabulary`). Each of the ten was first added in commit `90634d87`.
- **Node contract tests, re-run at HEAD `7849ff1b`:** `node --test scripts/tests/*.test.mjs` gives 23 run, 23 pass, 0 fail.
- **Crawl bound recipe, `--self-test`, re-run at HEAD:** `SELF-TEST OK`, exit 0. The recipe itself is still NOT RUN.
- **The `zdr_claim_floor` PDF test has changed since the round D3 run.** Round 4 adds a check that the ZDR markdown contains none of the PDF's own syntax (`%PDF`, `/Type`, `Tj`), and a standard-organisation control on the same bytes (`a_standard_org_scrape_of_the_same_pdf_keeps_its_bytes_as_raw`). **That change is NOT compiled and NOT run.** At 23:37 on 2026-10-21, `qtest.sh` printed `DOCKER IS NOT ANSWERING` (exit 76), and at 23:40 a 45-second `docker info` probe timed out. The figure "23 of 23" above is the run from before that change. The route-off mutation check (the fixture set to `"off"`, where the two PDF tests are expected to fail) is NOT RUN.

## WS5.B PDF test, the HEAD counts and the scrape-path audit: round 5 (2026-10-23)

- **The ZDR PDF test is run and committed as `ff8fb158`.** `cargo test -p quarry-edge --test zdr_claim_floor` through `qtest.sh` (`quarry-rust-env:1`, `CARGO_BUILD_JOBS=2`, in the shared worktree, where quarry-edge carried other units' uncommitted hunks): 24 passed, 0 failed, 0 ignored. The route-off mutant (the fixture's `set_var` changed to `"off"`): 2 failed, 22 filtered out. `a_zdr_scrape_of_a_pdf_returns_inline_text_and_persists_nothing` fails at its markdown syntax check, and `a_standard_org_scrape_of_the_same_pdf_keeps_its_bytes_as_raw` fails at the `formats.raw` byte count. The file was restored and its sha256 checked. This run replaces the "NOT RUN" statements in the round 4 block above.
- **Go, native, at HEAD `b8b7495b`** (the commit before `ff8fb158`). The tree was exported with `git archive`, so no other unit's uncommitted hunk is in these counts. `go test ./... -count=1 -v`, go1.26.5; the counts are every `--- PASS` and `--- SKIP` line, subtests included. `services/quarry-orchestrator`: 106 passed, 0 failed, 0 skipped (7 packages ok). `services/quarry-control`: 233 passed, 0 failed, 20 skipped (15 packages ok; each skip prints `QUARRY_TEST_DSN unset`). `pkg/quarrycontracts`: 35 passed, 0 failed, 0 skipped. `TestCrawlJobWF_OldVersionHistoryKeepsLegacyBehaviour`, which uses the `OnGetVersion` mock, passes.
- **Node, at HEAD `b8b7495b`:** `node --test scripts/tests/*.test.mjs` (Node v24.19.0): 23 run, 23 pass, 0 fail.
- **Crawl recipe, `--self-test` at HEAD `b8b7495b`:** `SELF-TEST OK`, exit 0. The recipe itself is still NOT RUN; it is for an exit-gate stack only.
- **Unwrap audit, re-run at HEAD `b8b7495b`:** `awk -f scripts/unwrap-audit.awk` over the 13 scrape-path files reports 0 production hits, at HEAD and in the worktree (those files have no uncommitted change). Positive control: a planted file with 2 production calls (`unwrap()`, `panic!`) and 1 call inside a `#[cfg(test)]` module reports exactly the 2 production lines.
- **Facts re-checked at HEAD `b8b7495b`:** `docs/openapi.yaml` `ScrapeRequest` carries `x-quarry-lenient-until`. `docs/CONTRACTS.md` section 9.4 (crawl bounds) and section 9.5 (the `/v1/extract` deferral) are present. The edge (`limits.rs`) and the orchestrator (`jobs/limits.go`) read the same four `QUARRY_CRAWL_*_MAX_*` names. The gateway's `start_crawl` refuses `maxPages` of 0 with a 400 and clamps larger values to 5000 before the edge.
- **Trap for a re-run.** A first export of the same tree, at a path of 251 characters for `services/quarry-control/internal/cron`, made `go vet` fail with `fork/exec ... vet.exe: The directory name is invalid`, and no test passed (the modules exited 1 with 0 passed). The export at 217 characters for the same package ran the counts above. Keep the export path short.
- **Not run in this block:** the 7-page crawl and its pre-fix capture, the recorded-history replay of `CrawlJobWF`, the WS5.B, WS5.C, WS5.D, WS5.E and WS5.F stack probes, the WS2.E container check, and the GitHub CI runs.

## quarry-edge, the whole test set: round 5 (2026-10-23)

- `cargo test -p quarry-edge --no-fail-fast` through `qtest.sh` (`quarry-rust-env:1`, `CARGO_BUILD_JOBS=2`): exit 0; 32 test-result blocks (unit, integration and doc tests); 1060 passed, 0 failed, 0 ignored. The run includes `zdr_claim_floor` (24 tests); no block reported a failure.
- The run used the shared worktree. It started at HEAD `c3faf37b`. Commit `fffe3627` (another unit's, in `scripts/tests` and an e2e spec, no quarry-edge Rust file) landed while the run was going. At the check after the run, quarry-edge still had other units' uncommitted hunks (13 modified files and 3 untracked files), so the count is for that tree and not for a committed HEAD.
- This replaces the round D3 figure of 29 blocks and 1016 passed, which was taken before the round 4 PDF test and the later units' commits.

## ZDR run tombstone: round 5 corrections and the erasure marker (WS4.J, C2, 2026-10-23)

**Status is unchanged: built, OFF, counsel review gates ENABLING it.** Nothing in this round sets `QUARRY_EDGE__ZDR_TOMBSTONE`, adds a default that turns it on, or adds an enable path. The section "ZDR run tombstone: built, OFF, and gated on counsel review" above still governs; this one corrects and extends it.

- **Late tombstone after erasure (new, commit `377f7194`).** `PurgeOrg` deleted an org's tombstones, but a run that ended after the purge (an agent run that expires later, a scrape in flight) would send its tombstone and re-create a row for the erased org. Migration `024_zdr_tombstone_erasures.sql` adds one marker row per erased org (an org id and a timestamp, nothing else, org id CHECK-shaped like a tombstone's; idempotent; its own file because `015` may already be recorded on a database). `PurgeOrg` writes the marker in the same transaction as its deletes, only for an org id a tombstone could carry. `Put` inserts only `WHERE NOT EXISTS (marker)` and answers the same silent no-op as a duplicate `run_id`, so the receiver still answers 204 and is **not** an erasure oracle. `Put` (one short transaction) and `PurgeOrg` both take `pg_advisory_xact_lock(hashtext('zdr_tombstone_org:' || org))` first, so a `Put` cannot read "no marker" before the purge commits and insert after its DELETE; a hash collision only serialises two orgs. The sweeper deletes markers at `max(retention, 7 days)` (`MinErasureMarkerDays`); a dry run returns before the marker sweep and deletes nothing. Deploy: the migration ships with quarry-control; an edge that sends nothing (flag off) never touches the table.
- **Retention bound (new, same commit).** `QUARRY_ZDR_TOMBSTONE_RETENTION_DAYS` above 36500 (100 years) is invalid and falls back to 30 like any other invalid value; it never disables the sweeper. The old cutoff multiplied the day count into a `time.Duration`, which overflows an int64 from 106752 days and put the cutoff in the future, so one sweep would have deleted every fresh tombstone. The cutoff is now clamp plus `AddDate`. **`AddDate` alone is not enough:** at `1<<55` and `math.MaxInt` days it wraps into the future too, so the clamp is load-bearing and both values are in `TestSweepWithAHugeRetentionDeletesNothingFresh` and `TestTombstoneCutoffNeverLandsInTheFuture`.
- **The no-read-route test was too weak (corrected, same commit).** The earlier `TestZdrTombstoneHasNoReadRoute` sent unsigned GETs, which the HMAC verifier refuses before the router, so it would have passed with a GET handler mounted. It now sends signed GET, HEAD, PUT, PATCH and DELETE (none may answer 2xx or leak a run id) and walks the chi route table (exactly `POST /v1/internal/zdr-tombstones`).
- **Claims of earlier commits that this round corrects.** `0ef06f3b` and `7849ff1b` were committed with "tests not run" and with the gitleaks scan skipped (Docker was down). Both are now done: `tests/zdr_tombstone.rs` ran (25 passed, 0 failed, including the three added in `e36429a1`), and gitleaks over the full diff of `0ef06f3b` and of `7849ff1b` (`git show` piped to `ghcr.io/gitleaks/gitleaks:latest detect --pipe --redact --config .gitleaks.toml`, same spelling as the hook) reports "no leaks found" for both. The Postgres runs recorded for `90634d87` passed `-run "Zdr|...|PurgeOrg"` with literal quote characters inside the regex, which dropped the `PurgeOrg` alternative; the round 5 run uses the regex unquoted and includes every `PurgeOrg` test.
- **Lane note.** The emission for an agent run lives in `finish_run` in the committed `agent_routes/lifecycle.rs` (`0ef06f3b`). In the working tree another unit has split `finish_run` into a new, still uncommitted `agent_routes/close.rs`, which carries the same `emit_run_end` call; the agent tests of `tests/zdr_tombstone.rs` passed against that tree. Whoever commits `close.rs` must keep the call.

**What was run (2026-10-23, shared worktree; quarry-control had no other unit's hunk, quarry-edge did).**

- quarry-control, native, `go vet ./...` clean; `go test ./... -count=1 -v`: 242 passed, 0 failed, 27 skipped in 15 packages (every skip is Postgres-gated and prints `QUARRY_TEST_DSN unset`). Against the figure of 233 passed and 20 skipped recorded above for HEAD `b8b7495b`, this is nine more passes (three in the memory store, four in the janitor, one in the receiver, one migration text test) and seven more Postgres-gated skips.
- Postgres, throwaway `postgres:16-alpine` under the docker lock, `QUARRY_REQUIRE_DB=1`, `go test ./internal/store/pg/ -run 'Zdr|Tombstone|Erasure|Migration0|PurgeOrg' -v`: 18 passed, 0 failed, 0 skipped. It includes `TestMigration024AppliesTwice`, `TestMigration024HoldsOnlyAnOrgIDAndATimestamp`, `TestPostgresTombstoneAfterOrgErasureIsNotStored`, `TestPostgresPutRacingOrgErasureLeavesNoRow`, `TestPostgresErasureMarkerOnlyForATombstoneShapedOrgID`, `TestPostgresPutBlocksOnTheOrgLockHeldByAnErasure` (an erasure transaction holds the lock; a `Put` must not return until it commits, then reports the no-op), `TestPostgresPurgeOrgBlocksOnTheOrgLock` and `TestPostgresErasureMarkerSweepIsByAge`.
- quarry-edge through `qtest.sh`: `--test zdr_tombstone` 25 passed; `--test zdr_feature_matrix` 13 passed; `--test log_field_discipline` 22 passed (baseline not regenerated); `cargo check -p quarry-edge --bins` finished, so the `main.rs` tombstone wiring of `0ef06f3b` compiles (6 warnings, dead code in the bin); `cargo test -p quarry-edge --no-fail-fast`: 32 test-result blocks, 1060 passed, 0 failed, 0 ignored.
- Mutation checks on a workspace COPY (never the tree). Go, native: the memory store ignoring the marker, writing no marker, writing one for any id, sweeping fresh markers; `MinErasureMarkerDays` 0; the pre-fix cutoff (a `Duration` with no clamp); the clamp removed; the env upper bound removed; a dry run reaching the marker sweep; the marker sweep removed; a GET route mounted: all killed. Postgres: `Put` without the lock, without `NOT EXISTS`; `PurgeOrg` without the marker, without the lock, with a marker for any id; migration 024 not idempotent, without its shape CHECK, with an extra `reason TEXT` column; a sweep that deletes fresh markers: all killed. **One survivor, equivalent:** reverting only `AddDate` to a `Duration` while the clamp stays (the clamp keeps the multiplication inside int64). Rust (emitter and wiring): a serialized key outside the allowlist, emission for a standard run, the posture ignored in `Pending::begin`, the id pattern check removed, drops not counted, a 400 recorded as a run, the agent close path sending nothing, the flag defaulting on, the permit not held, the delivery timeout removed, a delivery not counted as sent, the in-flight cap ignored, a failed delivery not counted: all killed. Not mutated: "await the sink instead of spawning" (the compiler does not allow it in the synchronous `emit` without a signature change, so no mutant was written); that property is covered only by `a_full_in_flight_cap_drops_and_counts_without_waiting` and the hung-delivery test.

**Not done, still the owner's.** Counsel review and any enabling; the evidence run on a stack (two rows, `pg_dump | grep -c` canary 0, the retention-0 sweep, a standard scrape adds no row), which needs the flag on; an HTTP read of tombstones; plan-based retention; `answer/stream`.

## Driver deadlines, profile directories and the run routes: round 4, unit B1 (WS2.A to WS2.D, WS3.D, WS3.F, 2026-10-23)

**Tab routes and the broker (WS2.C, a documented exception).** The plan says every action surface asks the grant broker. The tab routes keep `admit_tab_operation` instead of `admit_action`, on purpose: a tab open revalidates the grant, the domain allow-list and the URL itself, and a tab select or close touches no page content and nothing the grant covers, so they do not ask the broker. The doc on `admit_tab_operation` says so, and `a_revoked_grant_stops_a_tab_open_and_an_action_but_not_a_tab_select_or_close` pins both halves (a revoked grant stops the open and a step, and does not stop a select or a close). If a later slice adds a tab operation that reads or changes page content, it goes through `admit_action`.

**`taken_over_at` in the 409.** `USER_TOOK_OVER` now carries `details.taken_over_at` (RFC 3339, the moment the person took the browser). A run resumed into a takeover has none (the checkpoint does not carry it), and the field is then absent, not zero. Tests: `a_takeover_refusal_says_when_the_person_took_the_browser`, and `a_service_principal_is_admitted_again_after_the_browser_is_handed_back` (the give-back test for a service principal).

**Expiry is the system's act (WS2.D).** The owner timeline's `lifecycle` item for an expiry carries `initiatedBy: "system"`; the stored `Lifecycle` record has an `initiated_by` that is skipped when absent, so rows written before this change read back unchanged. The shutdown drain no longer uses `Arc::try_unwrap`: it waits (bounded) for an action that is running, marks the run closed so nothing new starts, and releases a clone of the session. `DrainReport.in_use` now means "an action was still running when the per-run deadline passed"; those runs are left to the process exit. Tests: `the_shutdown_drain_releases_every_idle_runs_session_and_leaves_a_busy_one`, the two drain unit tests, and `lifecycle_event_round_trip_keeps_the_system_initiator_on_an_expiry` (Postgres).

**Migrations 0010 and 0011 apply twice.** `migrations_0010_and_0011_apply_twice_and_leave_rows_alone` (quarry-runtime, `postgres-queue`) applies both a second time on a database that already holds rows and checks the rows did not change. It only means something against a database: with `DATABASE_URL` unset the Postgres tests skip and report a pass. Run it with `DATABASE_URL=postgres://postgres@<host>/<db> REQUIRE_CONTRACT=1 cargo test -p quarry-runtime --features postgres-queue --lib postgres_step_receipts` (7 tests).

**A step after a 504 (WS3.D).** `tests/agent_step_timeout.rs`: a navigation that hits its deadline is a typed 504 `TIMEOUT` (`details.op` `navigate`), and the next step is accepted at once; a second step queued behind a navigation that is stuck is counted in `in_flight` (2 while it waits), never overtakes it, and runs right after the 504 with `in_flight` back to 0. No clock decides anything in the second test: the double holds the navigation on a gate the test opens.

**Every CDP call has a deadline (WS3.D).** Four new entries in `deadlines.rs`: `TabMetadata` (3 s: reading a tab's URL for the list and the metadata refresh; the title is read under `Content`), `LiveFrame` (5 s), `GuardResolve` (5 s: the answer to a paused request in the network guard) and `Metrics` (3 s: the telemetry sample's `page.metrics()`, which the first version of this change missed). The eight functions the census found bare (`update_tab_metadata`, `install_network_guard`, `hydrate_cookies`, `hydrate_storage`, `install_popup_bridge`, `capture_live_frame`, `list_tabs`, `root_frame_id`) are bounded; a screencast that sends no frame is a typed `Timeout` for `live_frame`, a stream that ends is a driver failure. The test `cdp_calls_are_bounded` scans the production source call by call and fails on a bare call; run against the HEAD source it names exactly those eight functions. It goes by receiver, not by a list of method names: every method called on a `page`, `guarded_page`, `dialog_page`, `element` or `browser` is a CDP round trip unless the test's `NOT_CDP` list names it with a reason (a clone, the handler's listener channel, the browser lock, the browser process), literals are skipped, and each function in `BOUNDED_BY_THE_CALLER` is checked at every one of its call sites. The calls that are not in the table, and why, are listed in the module doc of `deadlines.rs`. The test cannot see a call whose future is built first and wrapped later. A tab's URL or title read that does not answer keeps the tab's last known value (`update_tab_metadata` used to store the `None`).

**One profile directory per browser launch (WS3.F).** A live browser has one profile directory and one crash-dump directory under the profile root (`QUARRY_BROWSER_PROFILE_ROOT`, default the temp dir's `quarry-chromium`), named for the launching process; sessions and tabs share them. A crash and the relaunch leave one pair; `shutdown()` (new on `BrowserDriver`, default a no-op; the edge calls it after the run drain, in `main.rs`) kills the browser, waits for it (bounded) and removes both directories until they are gone, so nothing is left when it returns. A driver dropped without `shutdown` still removes them (`remove_after_kill`: once now, then every 200 ms for up to 5 s from a thread, because the renderer and the crash handler can still be writing for a moment); the same retry covers the directories of a browser that died and was replaced. `shutdown()` also stops the reaper and the prober, and takes the browser lock before it stops the event-loop task, so nothing relaunches a browser behind it (the prober used to find no browser, count two failed probes and relaunch one with a fresh directory pair; a launch under way at shutdown could keep a live, unwatched handler); the next `acquire` is the explicit start of everything again (`no_browser_comes_back_after_shutdown`, live, fails with a second launch when the stop is taken out). A process that was killed hard leaves its pair; the next process's start sweeps the pairs of processes that are gone. Live tests: `one_profile_directory_per_browser_launch_and_none_left_behind` and `a_dropped_driver_still_takes_its_browsers_directories_with_it` (the second says "gone shortly after", as Drop cannot wait).

**clippy, rustfmt and file sizes (WS2.B).** The build image is now `quarry-rust-env:2` (`quarry-rust-env:1` plus `rustup component add clippy rustfmt`; `qtest.sh` uses it). `cargo clippy -p quarry-edge -p quarry-browser -p quarry-runtime --all-targets` exits 0 and reports 93 warning lines across the three crates; in `agent_routes` and the files of this unit what is left is the `Atomic::fetch_update` deprecation (renamed `try_update` on the newest toolchain only; not changed so an older toolchain still builds) and `new_runs` reported unused in one target. `clippy::result_large_err` is allowed on `agent_routes` with the reason in `mod.rs` (the refusal is the response, on the cold path). Nothing in CI runs clippy for Quarry, and the code is not rustfmt-formatted (`rustfmt --check` on the new files reports diffs), so neither is a gate here. `cargo doc --no-deps -p quarry-edge -p quarry-browser` exits 0; its warnings (unresolved links, a private-item link, an unclosed `<title>`) are not in this unit's files. Sizes after the split: `lifecycle.rs` 561 lines (the close, finish and drain half moved to `close.rs`, 219 lines) and `types.rs` 592 (the query-string types moved to `queries.rs`, 93 lines), both under the 650 target; every `crate::agent_routes::*` path is unchanged.

**Evidence of this unit.** Edge: lib 394, `agent_run_control` 33, `run_sweeper_integration` 25, `agent_evaluate_denied` 6, `agent_step_timeout` 2, `start_run_grants` 7, `browser_grants_prod` 2, `log_field_discipline` 22 (all passed, in the container). quarry-runtime `step_receipts` 20; Postgres `postgres_step_receipts` 7 against a throwaway `postgres:16-alpine`. quarry-browser lib on the live lane (`CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium`) three times: 181 passed, 0 failed, 4 ignored, every time. Mutations, observed: `deny_raw_script` never denying fails all 6 `agent_evaluate_denied` tests; `taken_over_at` never added fails `a_takeover_refusal_says_when_the_person_took_the_browser` and `a_service_principal_is_admitted_again_after_the_browser_is_handed_back`; the bare-call scan fails on the HEAD source naming the eight functions. A first run of the live lane failed `one_profile_directory_per_browser_launch_and_none_left_behind` (the relaunch removed the lost browser's directories once and a renderer was still writing, so a pair was left): `retire_browser` now uses the same retry and the test waits for the pair to be gone.

**Round 5 review fixes (unit B1).** (1) The claim "every CDP call has a deadline" was false for `telemetry`'s `page.metrics()`; it now runs under the new `Op::Metrics` (3 s), and `cdp_calls_are_bounded` was rebuilt so it cannot be wrong that way again: it goes by receiver (every method on a `page`, `guarded_page`, `dialog_page`, `element` or `browser` is a CDP call unless `NOT_CDP` clears it with a reason), skips literals, and checks each `BOUNDED_BY_THE_CALLER` function at every call site. Mutation, observed: the `metrics` wrapper taken out makes it fail naming `telemetry` / `page.metrics`. (2) `update_tab_metadata` keeps a tab's last known URL and title when the read does not answer (`a_tab_read_that_does_not_answer_keeps_the_last_known_value`, which forces the timeout under paused time through `bounded_hint`; it tests the helper and the bound, not `update_tab_metadata` end to end, which needs a live page). (3) `the_denial_is_logged_without_the_script` counts the warn lines that name its own run and asserts exactly three; mutation, observed: with the `run_id` field taken out of the warn line it fails (2 of 3). (4) `shutdown` stops the reaper and prober, takes the browser lock before it aborts the event-loop task, and sets a flag the ticks check; the next `acquire` clears it. `no_browser_comes_back_after_shutdown` (live, 200 ms probe, 2 s wait) and its native twin; mutation, observed: with the stop and the flag taken out the live test sees a second launch (`left: 2, right: 1`) and the native one fails. The race in which a launch finishes between the handler abort and the lock is closed by the order (lock first), not by a test: forcing it needs a hook in `ensure_browser` that does not exist. (5) Found by the live lane while doing this: a dying renderer can create the profile directory again after the first removal worked and the old pair was then left for good (`one_profile_directory_per_browser_launch_and_none_left_behind` failed in 3 of 9 full-suite runs and 0 of 12 alone); the removal now always runs from the cleanup thread (75 attempts) and must see the directories stay gone for 500 ms (`a_directory_a_dying_child_creates_again_is_removed_again`; with the settle time at zero it fails), and `shutdown` hands what is left to that thread. The live lane after these changes: 185 passed, 0 failed, 4 ignored, four times in a row. Before the last change the full-suite runs also showed two failures that are not in this unit's code and that I did not chase: `a_control_under_its_own_label_passes_the_hit_test` (`target_unstable`) once, and a batch of five failures in one run on a loaded host (timeouts such as `a_handle_whose_realm_was_destroyed_by_a_navigation_is_not_attached`, the profile-directory test, and `no_browser_comes_back_after_shutdown` itself, whose message that run's filter cut off, so whether load or the shutdown change failed it is not known); all passed in the next runs, and the new test passed in all of the last four full runs. The commit subject of 3b167c50 ("every CDP call has a deadline") overstated what that commit did; this section and the `deadlines.rs` module doc state the rule as it now holds.

**Not done.** The `Atomic::fetch_update` rename; the 93 other clippy warning lines (`result_large_err` apart, none is in this unit's files); a real async teardown for the `Drop` path (Drop stays best effort).

## WS6.A crawl recipe: the `--self-test` calls `check()` (review fix of `8f82b83d`, unit D3, 2026-10-23)

- **Correction to the WS6.A `--self-test` lines above (round D3 and round 5).** Those lines say the self-test checks that the four values match and that a short job is refused. That was true of the script as first committed (`8f82b83d`, blob `d1ab0ce7`), but its self-test never called `check()`: it re-implemented the comparison. On that blob, three mutant copies of `check()` (always PASS; always FAIL; a `check()` that prints the right verdict but records nothing) each printed `SELF-TEST OK` and exited 0. A `json_get()` mutant (always `__undefined__`) was caught. Commit `f15eda53` makes `--self-test` call the real `json_get()` and `check()` with the recipe's own paths and expected values, and assert the verdict each one records and prints: four PASS, the short job FAIL, and a missing path FAIL.
- Committed blob of `f15eda53` (`git show`, run from a scratch copy; no network, no stack): `--self-test` prints six `self-test ok` lines and `SELF-TEST OK: check() passes the four good values, refuses the short job and a missing value`, exit 0. `bash -n` exit 0. `shellcheck` is not installed on this host.
- Six mutant copies of the fixed script, each run with `--self-test`: `check()` always PASS; `check()` always FAIL; `check()` that records nothing; `json_get()` always `__undefined__`; `json_get()` on the wrong path for the short job; `check()` printing the wrong word. Each prints `SELF-TEST FAILED` and exits 1, with 1 to 6 `self-test MISMATCH` lines.
- Missing-input path (no `GATEWAY_URL`, `CONTROL_URL`, `ORG_ID`, `SESSION_COOKIE_FILE`, `SEED_URL`): exit 2, no network. Usage path (`--bogus`): exit 2.
- The recipe itself is still **NOT RUN**. It needs an exit-gate stack and must not run against the live stack.

## Browser grant L2 contract: review corrections of round 6 (WS1.A2 part 2, unit A2b, 2026-10-23)

- **Count correction.** Commit `6e7ab12b` says `playwright test --list` shows "the same 12 tests in the local project", and `543730ba` says 11. The spec `tests/e2e/browser-grants-prod-contract.spec.ts` contributes **11** tests to the `local` project (A1 to A4, B, C, A5a to A5d, A6) and none to `e2e`. `playwright test --list --project=local` prints "Total: 18 tests in 7 files": the 11, six browser-workspace tests and one `local-setup` test. The 12 of `6e7ab12b` counted the setup test of another project. Commits are not amended; this is the record.
- **Legs B and C pin the refusal.** Until WS1.B is on the stack they pass only on the known refusal: the answer recorded at the session start, status 403, message "browser grant_id is required" word for word (`CONTRACT_BC_EXPECTED_STATUSES` and `CONTRACT_BC_EXPECTED_MESSAGES` widen it). A 401, any authentication-shaped message (token, sign in, credentials, session expired, CSRF, origin) and any answer after the session start fail the leg whatever the override says. The sign-in is checked before each leg (a quarry token must mint from the cookie jar), so a signed-out run fails at that check and not as a "known failure". The classifier is `tests/e2e/browser-grants-prod-contract-refusal.ts`; `node --test scripts/tests/browser-grant-refusal.test.mjs` runs 10 tests on it.
- **Negative legs assert the message.** A4 (a foreign and a sibling host: "browser navigation is outside the run's allowed domains"), A5b (a forged id and A5c's second start: "browser grant is inactive or expired") and A6 (403 only, message "browser grant is inactive or expired" or "browser grant belongs to another user"). A6 no longer accepts 404.
- **Wrapper self-test covers the fatal branch of `scan_hops`.** With a fake `docker` that returns a 403 line (and one that returns a host name), `scan_hops` must fail with `fatal=1` and must not fail with `fatal=0`: 4 new checks, 30 ok lines in all. Mutation: the branch inverted (`[ "$fatal" = 0 ]`) makes those 4 checks print FAIL and the self-test exit 1.
- **The credential's path into the validator.** `scripts/tests/browser-grant-contract.test.mjs` now also requires that `cross_plane_tokens` in `main.rs` is bound exactly once, is never reassigned, and that binding is the `match cfg.quarry_service_api_key` that builds `AuthCoreServiceTokenProvider::new(&cfg.auth_core_url, credential)`. A second `let cross_plane_tokens ... = None;` before the validator (the reviewers' scenario) fails it ("bound exactly once (found 2)"), and it still failed the wiring test it used to slip past. 25 tests pass.
- **NOT run:** the L2 wrapper and the spec on a stack (exit gate). No Rust changed in this round, so no cargo run was needed.

## ZDR run tombstone: round 6 review fixes, the erasure marker is behind its own flag (WS4.J, C2, 2026-10-23)

**Status is unchanged: built, OFF, counsel review gates ENABLING it.** Nothing here sets `QUARRY_EDGE__ZDR_TOMBSTONE` or `QUARRY_ZDR_TOMBSTONE_RECEIVER`, adds a default that turns either on, or adds an enable path. This section corrects the round 5 section above where they differ.

- **Correction: the erasure marker was live on deploy (review finding).** Round 5 wrote the marker in every `PurgeOrg` and said the table was untouched while the edge flag was off. That was false: quarry-control wrote and swept it in every deployment, so a GDPR erasure retained the erased org's id for at least 7 days (indefinitely under `QUARRY_ZDR_TOMBSTONE_DRY_RUN`, because a dry run returns before the marker sweep) with the feature "OFF". Now `QUARRY_ZDR_TOMBSTONE_RECEIVER` (quarry-control, default off, truthy values true/1/yes/on) is the one switch for the quarry-control half: only with it set **and** an HMAC secret does `cmd/control` mount `POST /v1/internal/zdr-tombstones` and call `ZdrTombstones().SetErasureMarkers(true)`. With it off the route answers 404, `PurgeOrg` writes no marker, and an erasure leaves no trace of the org id. `zdrTombstoneReceiverPosture` in `main.go` is the tested decision (flag unset, flag false, flag without secret, flag with secret). The empty `zdr_tombstone_erasures` table is still created by migration `024` on every deploy; an empty table retains nothing. The migration header was rewritten to say so.
- **What the counsel review now covers in the quarry-control half.** The marker keeps an erased org's id (and the erasure time) for `max(retention, 7 days)`; under a permanent `QUARRY_ZDR_TOMBSTONE_DRY_RUN` it is kept indefinitely, so do not leave dry run on. A marker insert that fails still fails the whole erasure on purpose: the NATS consumer redelivers, and a valid-shaped org id can only fail the insert when the database is already failing the rest of the transaction. A deployment that ran `377f7194` or `e36429a1` before this fix may hold markers written by erasures since then; they are swept after 7 days (not under dry run), or delete them by hand: `DELETE FROM zdr_tombstone_erasures;` is safe (it only reopens the late-tombstone window for orgs erased in the last 7 days, and nothing sends a tombstone while the edge flag is off).
- **Enable order, updated.** Step (2) of "Enabling it" above becomes: set `QUARRY_ZDR_TOMBSTONE_RECEIVER=1` on quarry-control (with its HMAC secret), restart it, check the log line "ZDR tombstone receiver is ON", then set `QUARRY_EDGE__ZDR_TOMBSTONE=1` on the edge. `tests/zdr_tombstone.rs::no_file_in_the_workspace_turns_the_flag_on` matches any line containing `ZDR_TOMBSTONE`, so a compose, env or Dockerfile in the workspace that sets the new flag on fails it too.
- **The Put's wait on the org lock is bounded (review finding).** While `PurgeOrg` of an org runs (one transaction holding `pg_advisory_xact_lock`), every tombstone `Put` for that org used to wait unbounded, each holding one of the shared pool's 16 connections, so a burst of runs of the erased org ending together could starve jobs, schedules and admission. Now at most 4 `Put`s run at once (the rest wait for a slot without a connection), a `Put` sets `lock_timeout` 2 s inside its transaction, and the whole `Put` is bounded by 5 s. On timeout it returns `ErrZdrTombstoneBusy`: the receiver answers 500 `INTERNAL` ("tombstone not stored"), the edge counts the delivery as failed (emission is fail open) and nothing is stored. A `Put` for an org that is not being erased is unaffected. Test: `TestPostgresPutBurstBehindAnErasureCannotStarveThePool` holds an org lock and sends 48 `Put`s (three times the pool), checks that `SELECT 1` on the pool keeps answering within 1 s while they wait, that the `Put`s hold at most 4 connections, that every one returns `ErrZdrTombstoneBusy` within the bound, and that nothing is stored.
- **Migration guard no longer type-blind (review finding).** `TestMigration024HoldsOnlyAnOrgIDAndATimestamp` matched only `TEXT`, `TIMESTAMPTZ`, `INT` and `NUMERIC` columns, so `reason JSONB`, `body VARCHAR(200)` or `hits BIGINT` added to the table passed. It now parses the `CREATE TABLE` body (balanced parentheses, table constraints skipped) and compares every column definition, whatever its type, with the golden list `org_id TEXT`, `erased_at TIMESTAMPTZ`. Migration `015`'s guard uses the same parser for its column names.
- **CI.** The sweeper tests (retention cutoff, marker floor, dry-run guard, huge retention), the in-memory store tests (erased-org refusal, marker switch) and the `cmd/control` posture test ran only in the report-only `go-coverage` job. The required `quarry-go-zdr-stamp-chain` job now runs `go test ./internal/janitor/... ./internal/store ./cmd/control/...` too (the Postgres store was already in its own required job). A required check still needs branch protection to bite; the repository does not show whether that is configured.
- **Mutation checks (workspace COPY, never the tree).** Native Go: the memory store writing a marker whatever the switch, the receiver flag always true, the posture ignoring the HMAC secret, the posture not switching the marker on, the posture on by default, migration `024` with an extra `reason JSONB`, `body VARCHAR(200)`, `hits BIGINT` or `reason TEXT` column, `erased_at` retyped to JSONB, migration `015` with an extra `note JSONB` column: all killed (log `mut6-go.log`). Postgres: see the commit message for the second batch (no slot limit, 16 slots, no put timeout, `PurgeOrg` writing the marker whatever the switch, `PurgeOrg` never writing it).

## Evidence probe scripts for WS4.D, WS4.E and WS4.G (built, NOT run)

Built in the wave that follows the "build first, depth later" strategy. They are recipes for a
side-by-side evidence stack; nothing here was run against any stack, and a run is owed. Each
prints `PASS`, `FAIL` or `NOT-RUN` per check and a `RESULT` line; `--self-test` checks the
helpers on canned data without a network.

- `scripts/evidence/ws4-image-probes.sh` (WS4.D, WS4.E): against a private rebuilt edge at
  `RUST_LOG=debug`: a ZDR scrape, a ZDR `scrape/stream`, a ZDR search with SearXNG dead and a ZDR
  agent start plus navigate each leave 0 occurrences of their unique marker in the edge log;
  the standard-identity scrape (control) leaves at least 1; the ZDR search still logs a
  `search.upstream` line with outcome `error`. `GET /v1/change/latest?url=` and
  `/v1/search/suggest` need `ZDR_TOKEN_FILE` (a GET has no body flag); suggest is a
  documented Gap row and is reported, not asserted. The script refuses a log source or URL
  that names the live edge.
- `scripts/evidence/ws4g-live-zdr-token-probe.sh` (WS4.G): with a bearer of a ZDR-organization
  user read from a file, `POST /v1/crawl`, `/v1/batch` and an agent start with a persistent
  profile return 403 with a ZDR message; `POST /v1/schedules` returns 200 today (the documented
  Gap, the probe deletes the schedule it made) or 403 with
  `zdr_durable_mutation_forbidden` once WS4.H stage B is on. Seeded dev organization only.
- The gateway probes (WS4.A clippy triage, the four deploy probes, the `/metrics` read) and
  the WS4.B and WS4.C scripts are under `apps/Frontend Plane/verevonv3/scripts/` and described in
  `docs/runbooks/gateway-search-cache-tenant-incident.md`.

## WS15.S0 schedule capabilities: hide what the backend cannot honour (U04, built, NOT run, 2026-10-23)

The Ingestions Schedules surface offers only what the gateway says this installation can run. Every flag is off by default. Nothing here changes Quarry-v2 itself; the switch lives in the gateway environment.

- Gateway env `QUARRY_SCHEDULE_CAPABILITIES` (JSON object, read once at startup, never per request). Unset, blank or malformed input means all off. A malformed value logs a warning without the raw value.
  - Shape: `{"trigger":false,"backfill":false,"overlap":false,"kinds":[],"needs_activation":false}`.
  - Example to offer crawl schedules only: `QUARRY_SCHEDULE_CAPABILITIES={"kinds":["crawl"]}`.
- `GET /api/ingestions/schedules/capabilities` (behind the session) returns `{trigger, backfill, overlap, kinds, needsActivation}` inside the `data` envelope.
- Refusals while the flags are off:
  - `POST /api/ingestions/schedules` with a kind outside `kinds` returns 400 `unsupported_schedule_kind`. The check runs before any token mint or quarry call.
  - `POST /api/ingestions/actions` with `trigger_schedule` (or `backfill_schedule`) returns 501 `schedule_action_unavailable` unless that flag is on.
  - `overlapPolicy` is not forwarded to quarry unless `overlap` is on; quarry then defaults to `skip`.
- SPA: with `kinds: []` the composer shows the notice "No schedule types are enabled" and Save is disabled. Trigger is hidden unless `trigger` is on. If the capability read fails, the page treats everything as off.
- Rollback: unset `QUARRY_SCHEDULE_CAPABILITIES` (or set `{}`) and restart the gateway. Later slices (S1 kinds, S2 overlap, S3 trigger and backfill) turn a flag on by changing this value only.
- Deployment check (owed, not run): rebuild the gateway image, restart the Vite dev server (HMR does not see host edits), open Schedules: the notice shows, Save is disabled, no Trigger. Set `{"kinds":["crawl"]}`, reload: crawl appears.

## Web policy catalogue and lattice: build notes (WS7.B, U02, built, NOT run, 2026-10-23)

- What it is: `quarry_core::web_policy`, a pure library. It has no flag and no runtime effect until WS7.C wires the edge. Layers only tighten; `resolve()` names the layer that decided each key.
- Reading a denial: `details.reason` is one of the closed set (`policy_denied`, `raw_script_denied`, `policy_unreadable`, `unknown_capability`, `ceiling_exceeded`, `domain_denied`, `domain_not_allowed`). `decision_source` is one of `invariant`, `zdr`, `org_policy`, `grant`, `baseline`, `request`, in that tie order. `policy_rev` is present only when `org_policy` decided.
- `policy_unreadable` is deliberate: an org policy that cannot be read refuses the request. It never falls back to the baseline alone.
- `raw_script_denied` keeps its reason and message from WS2.A and is decided by the invariant layer, so no layer can unlock `evaluate`.
- Changing the catalogue: edit `src/web_policy/catalogue.rs`, then regenerate with `UPDATE_CONTRACT_GOLDEN=1 cargo test -p quarry-core --test web_policy catalogue_json_matches_the_checked_in_file_and_its_pin` (in `apps/Ingestion Plane/Quarry-v2`). Then update the digest in the GOLDENS row of `scripts/tests/zdr-contract-sync.test.sh` and run that script. Key strings become public once the claim carries them, so review with Control before WS7.H.
- Rollback: revert the commit. Nothing else depends on the module until WS7.C.

## WS17.S0 CDP capability spike: run it and file the table (U05, built, NOT run, 2026-10-23)

The spike answers the three `[UNVERIFIED]` CDP statements (D16-12) before WS17.C1 chooses between "frame-less CDP" and "journal/flush". It is a test only; no product code changes.

- Test: `crates/quarry-browser/tests/profile_cdp_spike.rs`, test `cdp_capability_spike` (`#[ignore]`). It prints a `probe | outcome` table and does not assert the expected answers yet.
- Run in the live Chromium lane, through the docker lock (`qtest.sh`, image `quarry-rust-env:2` has chromium):
  `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide --test profile_cdp_spike -- --ignored --nocapture`
- P1, P2 and P3 navigate to `https://example.com` (outbound HTTPS needed). Offline, a probe stops at its first navigation and records `ERROR`; such rows are not evidence. P4 uses a loopback origin only.
- Record the Chromium version next to the table (the plan pins 149.0.7827.102, WS3).
- Before C1 starts: paste the table into C1's design notes and answer the three statements from it (P1 cookies, P2 DOMStorage, P3 IndexedDB). P4 answers seeding and the `pagehide` flush.
- Without `CHROMIUMOXIDE_TEST=1` the test returns at once, so the ordinary workspace job is unaffected.

## Log hygiene lint and the log_message helper (WS12.S6, unit W2-U06, 2026-10-23)

- `crates/quarry-runtime/tests/log_hygiene.rs` is a ratchet over every crate's production source (`crates/*/src`, `#[cfg(test)]` items skipped). Inside a `tracing` call it fails on `?action`, `?request`, `?observation`, `.text`, `.value`, `dom_summary`, `accessibility` and `url = %x` unless `x` is `redact_url(..)` or `log_url(..)`.
- Exemptions: a `// zdr-log-ok: <reason>` comment on the macro's first line or the line above it (the WS4.F convention; an empty reason does not exempt), or one line in `crates/quarry-runtime/tests/log_hygiene.allow` (tab separated: path relative to `crates/`, the trimmed source line, the reason). An allow entry that matches nothing fails the test. The allow-list is empty.
- Run it: `cargo test -p quarry-runtime --test log_hygiene`. A failure names the file, line, rule and text. Fix the call (log a count, a class or `error_class`, or wrap the URL in `redact_url`), or add a reasoned exemption.
- `QuarryError::log_message()` (quarry-core `error.rs`) returns the error message through the WS12 Durable rules, with no `details`. `Display` is unchanged. Use it at a call site that must log error text; the call sites the plan named already log `error_class` only.
- The browser hydration settle log (`quarry-runtime/src/browser_driver.rs`) logs the page URL through the Durable URL rules (userinfo and secret query or fragment values masked).
- No flag and no change to any response. Rollback: revert the commit. The lint does not see a raw error text (`error = %e`); WS4.F covers quarry-edge and the `zdr-log-ok` comments cover the runtime sites reviewed so far.


## Request id intake: `x-request-id` (WS10.S1, U01, built, NOT run, 2026-10-23)

- What it is: `quarry_edge::request_id` (layer, `current()`, span), `quarry_runtime::request_context` (task-local). Handlers take `meta.request_id` from `request_id::current()`. A codemod replaced every `RequestKind::new()` call site in `quarry-edge/src`.
- Behaviour: a valid inbound id is adopted, anything else is minted, the header is echoed on every response (including the 408 timeout and 413 body-limit answers, because the layer is the outermost one). Never a 400.
- Flag: `QUARRY_EDGE__REQUEST_ID_ADOPT`. Unset, `1`, or any value other than `0`/`false`/`off` adopts (default on). `0` always mints. It is read once per process, so a restart is needed.
- Support lookup: `docker logs quarry-edge` lines carry `request_id` on the request span. Filter by `(org_id, request_id)` in any store, never by the id alone.
- Gateway: `apps/Frontend Plane/verevonv3/apps/gateway/src/request_id.rs` applies the same rule and forwards the adopted id to upstream builders (`request_id::apply`). It exposes `x-request-id` to browsers through CORS.
- Rollback: set `QUARRY_EDGE__REQUEST_ID_ADOPT=0` (mints only; the header still exists), or revert the codemod commit alone (it is mechanical).
- Known limit: the id does not follow `tokio::spawn`. Code in a spawned task that calls `request_id::current()` mints a new id; the gateway `apply` adds no header outside a request scope.

## Crawl job statistics and the agent run-duration histogram (WS6.K, unit W2-U08, built, NOT run, 2026-10-23)

- Job statistics: a finished crawl or batch job carries `statistics` (`pages_visited`, `pages_failed`, `duration_ms`, `status_classes`, `drivers`, `error_classes`). The orchestrator builds it in `internal/workflows/statistics.go` and attaches it to the terminal event as `payload.statistics`; control migration 020 stores it on the job and `GET /v1/jobs/{id}` returns it. Every key is from a closed vocabulary, so no URL, title or error text can appear. Commit 369caa89 built this. No flag: the object is written for every non-ZDR crawl or batch job (crawls are refused under ZDR, so a ZDR job never has it).
- Run-duration histogram: `quarry_agent_run_duration_seconds{class}` with `class` = `interactive` or `agent`. `finish_run` (`crates/quarry-edge/src/agent_routes/close.rs`) records it once per run, from the run clock (`RunLive.started_ms`) through `record_run_duration` in `agent_routes/run_live.rs`. Both explicit closes and sweeper expiries record. It is a no-op until the WS6.I recorder is on (`QUARRY_EDGE__METRICS_ENABLED=1`, scraped on the internal `/metrics` listener, never the public port). No flag of its own; rollback is a revert of the commit.
- Check the object: `GET /v1/jobs/{id}` on a finished 5-page crawl shows `statistics.pages_visited` = 5; `psql` shows the same JSON in `jobs.statistics`.
- Tests written (not run in this unit): orchestrator `TestCrawlJobWF_TerminalEventCarriesStatistics`, `TestBatchJobWF_TerminalEventCarriesStatistics`, `TestStatisticsContainNoUrlOrErrorText` (`go test ./internal/workflows/ -run Statistic`); control `TestTerminalEventPersistsStatistics` (`go test ./internal/resources/ -run Statistics` in quarry-control); quarry-edge `run_duration_tests` (`cargo test -p quarry-edge run_duration_tests`).
- Known gap: the orchestrator has no Prometheus recorder, so the histogram lives only in quarry-edge. The histogram is not yet listed in WS6.I's `describe_all`.

## Crawl scope and robots: the crawl characterization tests and the go-test job (WS14.S0, WS14.S0b, unit W2-U12, built, NOT run, 2026-10-23)

- Contract: `docs/CRAWL_SCOPE_ROBOTS.md`. This change switches nothing on; the crawl behaves as before, and the two tests pin today's behaviour.
- Local run (orchestrator): `cd "apps/Ingestion Plane/Quarry-v2/services/quarry-orchestrator" && go test ./internal/workflows/ -run CrawlJob -count=1`. Expected: `TestCrawlJobWF_CharacterizesExternalLinkFollowing_Legacy` (a cross-host link is fetched) and `TestCrawlJobDurable_EnqueuesEveryLinkWithOneActivityEach` (one enqueue per link, each page fetched once) pass on the unmodified code. They were written in W2-U12 and compile-checked with `go vet ./...`; they were not run in the build.
- CI: the `go-test` job in `.github/workflows/quarry-v2-ci.yml` runs `go test ./... -count=1` for `services/quarry-control`, `services/quarry-orchestrator` and `pkg/quarrycontracts`, with no `continue-on-error`. `go-coverage` stays report-only. Branch protection must list the three `go-test (...)` checks as required; a workflow file cannot set that.
- A failure in a module you did not touch: name the failing tests in the PR. Do not quarantine a test without a named list in the PR (plan WS14.S0b).
- Rollback: revert the commit that added the job; the two tests go with it.

## Scrape capture and action types: contract only, dark (WS13.2, unit W2-U09, built, NOT run, 2026-10-23)

- New modules in `quarry-core`: `capture.rs` (screenshot and PDF options, the caps, `CapturePlan`, range checks) and `scrape_actions.rs` (the closed action set, the denied names, the credential-field heuristic, `validate_actions`, the outcome metadata types).
- `scrape.rs`: `formats` accepts `screenshot` and `pdf` (the string form means the defaults) and the object form `{"type":"screenshot"|"pdf", ...}`. `actions` and `actionsOnError` (`abort` default, `continue`; no retry) are parsed and checked. `ingest: true` with `actions` is `not_supported_with_actions`.
- Nothing runs. No driver, route or response field uses these types yet (WS13.3 to WS13.5). The flags `ScrapeLimits.captures_enabled` and `actions_enabled` default to off, so a request for either is `400 feature_disabled` (at `formats` or `actions`), never a 200 without the capture. WS13.4b and WS13.5 set them from `QUARRY_EDGE__SCRAPE_CAPTURE` and `QUARRY_EDGE__SCRAPE_ACTIONS`.
- Denied action names (`executeJavascript`, `evaluate`, `navigate`, `back`, `forward`, `clickPoint`, `mouseWheel`, `upload`, `download`, `dialog`) are `action_denied` at `actions[i].type`, in any letter case. Any other unknown type is `unknown_action_type`.
- `write` and `select` into a credential-looking selector are `credential_entry_denied`. This is a heuristic guard, not a guarantee (ADR-2, soft). Negative cases (`#search`, `input[name=q]`, `passage`) are not refused.
- Errors carry their entry path (`actions[3].selector`). Text and selectors never appear in an error message or in the effective-options echo, which carries counts, types, the wait budget and `on_error` only.
- Caps: 50 actions, 60 s total wait (a selector wait counts its timeout in full, default 5 s), 30 s per wait, 512-byte selectors, 4 KiB per write and 16 KiB in total, 8 captures per request across `formats` and `actions`, viewports up to 3840x2160 at device scale factor 1 to 2, PDF `maxPages` 1 to 200. Environment settings can lower these, never raise them (`CaptureLimits::lowered`, `ActionLimits::lowered`).
- Golden: `docs/schemas/scrape-options.schema.json` and `effective-scrape-options.schema.json` regenerated with `UPDATE_SCHEMAS=1 cargo test -p quarry-core --features jsonschema --test scrape_schema`. The Model Plane vendored copy is stale until its owner refreshes it.

## Canonical URL helper and redirect reconciliation (WS14 / E3.7.6, unit W3-B1-04, built, NOT run, 2026-10-23)

- What it is: `quarry_transform::canonical_url` (pure, no I/O, no tenant data). `canonicalize(input, CanonicalOptions)` gives one string per resource. `request_id_for_canonical` derives `cu1-` plus 32 lower-case hex digits (blake3 over `canonical-url-v1`, a newline, and the canonical string). `reconcile(requested, final_url, opts)` reports `redirected`, `same_host` and the request ids the frontier must mark as seen (`seen_request_ids`).
- Default (conservative) folding: scheme and host case, default port, fragment, dot segments, empty path to `/`, one trailing dot on the host, and percent-escapes of unreserved bytes (decoded; reserved escapes such as `%2F` stay encoded, upper-cased). Kept distinct on purpose: trailing slash, `www.`, `index.html`, query order and tracking parameters.
- Opt-in `CanonicalOptions { sort_query, strip_tracking_params }`: sorts query pairs, removes `utm_*`, `gclid`, `fbclid`, `msclkid`, `yclid`, `mc_cid`, `mc_eid`, `_ga`, `_gl` (keys matched case-insensitively). Nothing passes these yet.
- Refused with a typed class: other schemes than http and https, userinfo in the URL (`url_credentials`, which blocks the `http://example.com@evil.test/` trick), no host, unparseable, and input over 8 KiB. `CanonicalError::code()` returns only `url_*` class names; the URL is never in an error.
- Not wired. No caller exists in this unit. The enqueue path (`enqueue_batch` and the crawl seed through the scope filter) waits for W2 U60 (WS14.S3). The older `normalize_url` in `quarry-runtime/src/crawl_frontier.rs` (fragment and empty path only) is unchanged.
- Flag: none, nothing runs. Rollback: revert the commit; the module has no consumer.
- Tests written, not run: `cargo test -p quarry-transform --test canonical_url` (spellings, refusals, idempotence, request-id shape, and redirect reconciliation: no redirect, scheme upgrade, cross-host, `www.`, trailing slash, tracking-only redirect under each option set, refused sides).

## Job lifecycle reads: status, errors, origin (WS15.L1, unit W2-U11)

Built, NOT run. The contract is in `docs/CONTRACTS.md` ("Job lifecycle reads and origin").

**Deploy order.** Control first (migration `016_jobs_lifecycle.sql` and the new routes),
then the edge, then the orchestrator. The migration is additive and idempotent; the old
edge keeps working against the new control, since its paths are unchanged. An edge
that is ahead of control answers the new reads with 404 (the old control has no such
route), and the handoff still works because control accepts a body with or without
`origin`.

**Rollback.** Previous edge, then previous control. The columns stay: they are defaulted
and nothing reads them back except the new routes. No flag is involved.

**Check a job by hand.** With a user token for the org, `GET /v1/crawl/<job_id>` answers
the status document; `GET /v1/crawl/<job_id>/errors` the final failures and blocked
pages; `GET /v1/crawl/ongoing` the org's crawls still in flight. A foreign id is 404
(not 403: the org is not revealed). A batch id on the crawl path is 404.

**"Status says running but the crawl has finished".** Look at `control.pending` and the
terminal event. A terminal event moves the job to its terminal status once; a status that
is still `running` after `run_completed` is a missing event (check the job's events with
`GET /v1/jobs/<job_id>/events`) or a late write that the terminal rule refused. The
terminal rule never moves a job back, so a stale `running` write from the dispatcher
cannot undo `completed`.

**"Errors list is empty but pages failed".** Only the last attempt of a page is listed,
and only when it failed (the orchestrator marks it `final: true`). A page that failed and
then succeeded on a retry is not an error. Blocked pages are under `blocked`, never under
`errors`.

**"403 on the errors route".** The token carries the organization's ZDR floor, and error
URLs are not returned under it. This is the intended answer; the status counts still come
back on the status route.

**Filter a list by status.** `GET /v1/crawl/jobs?status=running,paused` (control accepts
the vocabulary `accepted`, `running`, `paused`, `completed`, `failed`, `cancelled`,
`timed_out`, `skipped`; any other value is 400).

**Origin on a job.** `origin.type` tells who started it: `user` (an interactive token),
`agent` (a Model Plane service; `origin.ref` is the run id), `api` (any other service or
API client). `unknown` is a row written before migration 016 and is not a fault.

## Crawl fixture site and the crawl-scope harness (WS14.S1, unit W2-U10, built, NOT run, 2026-10-23)

- Rust fixture: `crates/quarry-edge/tests/support/fixture_site.rs`, `FixtureSiteDriver`. It is an in-memory `Driver` that serves what a test configures, records every request (404s included) and follows redirects hop by hop. It opens no socket, so it does not exercise the connection-level SSRF guard. `FixtureSiteDriver::standard_site()` mirrors the live fixture content. Self-test: `cargo test -p quarry-edge --test fixture_site_selftest` (written, compile-checked, not run).
- Live fixture: `apps/Ingestion Plane/docker-compose.crawl-fixture.yml`, an overlay on the root stack. It adds two nginx services (`crawl-fixture-a` with aliases `fixture-a.crawl.test` and `docs.fixture-a.crawl.test`; `crawl-fixture-b` with `fixture-b.crawl.test`), a `crawl-fixture` network (192.88.99.0/24) and that network on `quarry-edge`. Content: `Quarry-v2/deploy/compose/crawl-fixture/{a,b}`. The overlay is test-only. No production compose file may name it; `crawl-scope-robots.sh --guard` checks that.
- Bring-up, last `-f` on the root stack's env-file chain: `cd "apps/Ingestion Plane" && docker compose --env-file ../../.env --env-file .env -f docker-compose.yml -f docker-compose.crawl-fixture.yml up -d --wait`. The overlay's merged config was checked in a scratch copy with placeholder env values (`docker compose ... config -q` exits 0). Not checked on a running stack: that the subnet is accepted by Docker Desktop on Windows, and that the edge reaches the fixture hosts.
- Harness: `Quarry-v2/scripts/e2e/crawl-scope-robots.sh --expect legacy`. It needs `DEV_BEARER` and an edge started with `ENVIRONMENT=dev` (the dev bypass is refused in production). It posts `/v1/crawl` to the loopback edge (`127.0.0.1:8082`), polls `GET /v1/jobs/{id}` on control (`127.0.0.1:28081`) until the record carries statistics, then reads the two fixture containers' access logs. `legacy` expects `GET /ext` on fixture-b and no `GET /robots.txt` on fixture-a. `observe` and `enforce` are planned sets for WS14.S2 to S4; they are not expected to pass yet. `--self-test` and `--guard` run offline.
- Rollback: nothing in a production path refers to the fixture. Stop the two containers, or remove the overlay file and the `crawl-fixture` network.

## Webhook egress guard in quarry-control (E3.5-2, unit W3-B1-05, built, NOT run, 2026-10-23)

- What it is: `services/quarry-control/internal/egress` is a copy of integration-corev2's `internal/egress` (`guard.go`, `client.go` and their tests, all unchanged except the package comment in `guard.go`), plus `conformance_test.go` for the webhook and catalog-watch cases. It is the single-resolution SSRF and DNS-rebinding guard: `ResolveAndVet` resolves a host once and refuses the whole lookup if any address is loopback, private (RFC1918, RFC4193), CGNAT, link-local, multicast, reserved, this-network, or NAT64-embedded, or if the host is a cloud metadata name.
- Where it is live: `internal/dispatcher/dispatcher.go` (the `hc == nil` fallback in `Run`, which `cmd/control/main.go` always uses) and `internal/catalogwatch/catalogwatch.go` (the `httpClient == nil` fallback in `NewClient`) now build their client with `egress.SafeClient`, keeping the old 30 s request timeout. This is a live change, not a dark one: webhook delivery in production goes through the guard from the commit that carries it.
- What a tenant sees: a webhook target that resolves to a blocked address fails the delivery with a transport error that wraps `egress: destination blocked by policy`. It takes the normal retry and dead-letter path (five attempts, then DLQ). A target that answers with a 3xx is no longer followed; the 3xx response is recorded as the result.
- Allowlist: `EGRESS_ALLOWLIST_HOSTS` and `EGRESS_ALLOWLIST_CIDRS` (comma-separated) are read once per process by `egress.Default()`. Leave both unset in production; the package doc says an entry should name one confirmed legitimate destination.
- Keeping the copy in step: the copy must match integration-corev2 except the header comment of `guard.go`. Check with a diff of the two `guard.go` files and ignore the header lines; a behaviour change in one copy needs the same change in the other.
- Not in this section: org scoping of webhook registrations and the fail-closed migration `021_webhook_org_scope` (E3.5-1), the text-fingerprint switch (E3.5-4), and per-job webhook fields.
- Verify (test phase, not yet run): `cd services/quarry-control && go test ./internal/egress/ ./internal/dispatcher/ ./internal/catalogwatch/`. Native `go vet ./...` in the module exited 0; the tests were not run.
- Rollback: revert the anchor lines in `dispatcher.go` and `catalogwatch.go` to `&http.Client{Timeout: 30 * time.Second}`. The `egress` package has no other caller and can stay.

## Shared browser driver foundation: background tabs, tab pins, per-tab egress (WS8.E, U07, built, NOT run, 2026-10-23)

Dark and without a caller. Nothing in the edge, the runtime or the SPA opens a background tab, pins a handle or installs a tab policy yet, so behaviour is unchanged for every existing run. The surface is the capability bits, the `BrowserDriver` defaults and the chromiumoxide implementation.

- What is new: `BrowserSession::pinned_to(tab_id)` and `tab_pin()`. A pinned handle addresses its tab and never the active tab; a closed pin is NOT_FOUND with `details.reason = "pinned_tab_closed"` and no fallback. `BrowserDriver::open_tab(OpenTabOptions { url, activate, egress_policy })` opens a background tab with `activate: false`, which leaves the active tab where it was (a session with no active tab still activates the new one). `configure_tab_egress_policy(session, tab_id, policy)` installs a per-tab egress policy: the tab's policy wins, then the session policy, then deny-all. `active_tab_id(session)` reads the active tab. `new_tab` now calls `open_tab` with `activate: true` and behaves as before.
- Observations through a pinned handle: devtools events, egress receipts and dialogs of other tabs are not returned; `respond_dialog` refuses a dialog that belongs to another tab.
- Capability bits (serde defaults `false` on the wire): `background_tabs` and `tab_pinning` are true for the chromiumoxide driver; `per_tab_egress` is true only with the pinned egress proxy, the same condition as `isolated_egress`.
- Code: `crates/quarry-browser/src/tab_control.rs` (new, pure helpers and unit tests), `lib.rs` (trait defaults, `BrowserSession.tab_pin`), `chromiumoxide.rs` (the implementation and seven live tests). Every `BrowserSession` literal carries `tab_pin: None`.
- Rollback: revert the commit. No flag, no schema, no wire change for existing callers.

Tests (written and compile-checked; NOT run in the build):

- Unit, no Chromium: `cargo test -p quarry-browser tab_control` (key lookup order, pin resolution, observation filters).
- Live lane: `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide --lib chromiumoxide::tests -- --ignored` runs `background_tab_leaves_active_tab_unchanged`, `pinned_session_clicks_pinned_tab_not_active_tab`, `per_tab_policy_blocks_third_host_in_agent_tab_only`, `human_tab_keeps_open_web_while_agent_tab_is_narrow`, `closing_pinned_tab_does_not_fall_back_to_active`, `pinned_observation_never_contains_other_tab_events` and `background_tab_is_not_throttled`. The last one asserts at least 10 ticks in 2 s, a floor chosen before the first run; record the real count and adjust the floor there. The per-tab tests use `https://third.example.test/` as a page subresource; the assertions read the policy decision recorded in the receipt, not whether the request succeeds, so an offline runner still proves the boundary.
- Without `CHROMIUMOXIDE_TEST=1` every live test returns at once.

Traps for the next reader:

- A background open still runs `require_configured_egress`: a session needs its own policy before a tab can open, even when the tab carries its own.
- `goto` on a pinned handle with a closed pin returns `pinned_tab_closed`; it never opens a stand-in tab.
- Tab policies are removed with the tab (`close_tab`) and with the session (`teardown_locked`, `ensure_browser`). A tab whose attach fails keeps its entry until the session ends; tab ids are not reused within a session.

## Quarry-v2 eval CI gates (E3.9.1, W3-B1-01)

Workflow: `.github/workflows/quarry-v2-eval.yml`. Runs on pull requests and pushes to `main` that touch Quarry-v2, and on demand. It runs three binaries in order, from the Quarry-v2 root: `quarry-eval` (fixtures, writes `lab/evals/scoreboard.json`), `quarry-bakeoff --challenger lab/evals/scoreboard.json` (solo until a baseline is checked in, writes `bakeoff.json` and `bakeoff.md`), and `quarry-release-matrix` (writes `release_matrix.json`, `release_matrix.md` and `release_score.json`). The job fails when a fixture fails or a gate fails.

Run it locally: `cargo run -p quarry-evals --bin quarry-eval`, then `cargo run -p quarry-evals --bin quarry-bakeoff -- --challenger lab/evals/scoreboard.json --challenger-label quarry`, then `cargo run -p quarry-evals --bin quarry-release-matrix`. The score file records `unknown` as its commit unless `QUARRY_RELEASE_COMMIT` is set.

Gotchas: the job deletes the committed `lab/evals/*.json` outputs before it runs, so the matrix can never pass on an old run. It builds only the three gate binaries, not `quarry-browser-benchmark`, because that bin constructs the `BrowserSession` struct of `quarry-browser` directly and breaks when that struct changes. The provider matrix and Chromium are not run here.

Not done: the Tier B corpus and the Chromium job (E3.9.2 onward), the scheduled Tier C run, and the default-on gate thresholds, which are still `[placeholder, ratify]`.

## Job policy name, job preset `requires`, and `/v1/blocklists` 410 (WS7.F, unit W2-U14, built, NOT run, 2026-10-23)

- **Rust name.** `quarry_core::policy::RunPolicy` is now `JobPolicy` (serde shape unchanged; the default is pinned by the serde golden `default_job_policy_serde_golden` in `crates/quarry-core/tests/contracts.rs`). The runtime's own `RunPolicy` (`quarry_runtime::policy`) keeps its name. Repo grep gate: `rg "policy::RunPolicy" apps/Ingestion\ Plane/Quarry-v2/crates` should find only the runtime type (`crates/quarry-runtime`), not `quarry-core`.
- **Go preset `requires`.** `GET /v1/presets` now carries `requires` (omitted when empty): `fast` has `robots.max_mode=ignore`; `careful` and its deprecated alias `stealth` have `proxy.max_tier=pool` (sticky proxy strategy). `polite` (baseline) has none. The keys are the names in `quarry_core::web_policy`. They are declared only: job creation does not check them. The test `TestEveryPresetThatExceedsTheBaselineDeclaresRequires` fails if a preset exceeds the baseline without declaring it.
- **`/v1/blocklists` is 410 Gone.** Every method on `/v1/blocklists` and `/v1/blocklists/{id}` answers `410` with the typed envelope: `error.code` `GONE`, `retryable` false, `details.reference` `docs/POLICY.md`. `GONE` is not in the closed code set (it mirrors the Rust `ErrorCode` enum), so the handler writes the envelope itself; `HTTPStatus()` would answer 500 for it.
- **Rollback (one line).** In `services/quarry-control/cmd/control/main.go` replace `resources.MountBlocklistsGone(r)` with `resources.MountBlocklists(r, db)`. The blocklist table and `store.BlocklistEntry` are untouched. No in-tree caller of `/v1/blocklists` was found (the edge `/v1/security/blocklist` shim is documented in GAP.md but has no code path here); an external caller would see 410.
- **Tests (written, compile-checked, not run).** `cargo test -p quarry-core --test contracts default_job_policy` (the golden); `cd services/quarry-control && go test ./internal/resources/ -run 'Preset|Blocklist'` (`TestEveryPresetThatExceedsTheBaselineDeclaresRequires`, `TestPresets_RequiresOnTheWire`, `TestBlocklistsReturnsGone`).
- **Evidence owed (not run).** Rebuild quarry-control. With the HMAC-signed internal call: `GET /v1/blocklists` returns 410 with `error.code` `GONE`; `GET /v1/presets` lists `fast` with `requires: ["robots.max_mode=ignore"]`; job creation with `preset: "polite"` still returns 201.
- **Not done.** Enforcement of `requires` at job creation (WS7.C and WS7.I). The `careful` `requires` is an addition beyond the plan's text (the plan named only `stealth`; the alias shares the policy, so both declare it). The org web-policy API that the 410 points to (WS7.H) is not built.

## Profile saves: ordering, deletion tombstones and exists (WS17.P1, unit W2-U15, built, NOT run, 2026-10-23)

Code: `crates/quarry-browser/src/session.rs` (`SaveMeta`, `SaveOutcome`, `clamp_saved_at`, the in-memory ordering), `crates/quarry-runtime/src/postgres_profile_store.rs` (the conditional upsert, the transactional delete, `exists`, `spawn_tombstone_sweeper`), migration `crates/quarry-runtime/migrations/0015_profile_ordering.sql`, `crates/quarry-runtime/src/cached_profile_store.rs` (invalidate-on-write), `crates/quarry-edge/src/profile_routes.rs`, and the two capture paths (`persist_current_page` in `chromiumoxide.rs`, `release` in `kernel.rs`).

- Behaviour. `ProfileStore::save` takes a `SaveMeta` (`saved_at` is the capture START, `owner`) and returns a `SaveOutcome`: `Stored { rev }`, `IgnoredStale` (the row already has a `saved_at` at or after this one) or `IgnoredByTombstone` (a delete at or after `saved_at`). An ignored save is not an error. Log lines carry the class only: `profile save ignored reason=stale|tombstone`, `browser profile capture ignored`, `kernel session capture ignored`. `saved_at` is clamped to now + 5 s. `delete` writes a 7-day tombstone in the same transaction as the `DELETE`. `exists` is a row probe, and `update_profile` uses it.
- Flags. None. Strict tightening. Rows with `saved_at` NULL (written before the migration) accept the next ordered save.
- Migration. `0015_profile_ordering.sql` adds `saved_at`, `rev` and `owner_user_id` to `quarry_profiles`, makes `snapshot` nullable and creates `quarry_profile_tombstones`. Idempotent. Rollback: revert the code; the columns and table are harmless to older pods.
- Sweeper. `main.rs` starts `spawn_tombstone_sweeper` once at boot, hourly, when the Postgres store is in use. The in-memory store keeps tombstones forever (dev and test only).
- Owner. `owner_user_id` is stamped on INSERT only, from the verified claims on `POST /v1/profiles` (save). The capture paths pass `owner: None` (the browser lease carries no actor id), and `create_profile` does not stamp an owner. WS17.P2 must close both before it enforces ownership.
- S3 store. Unordered: `save` ignores `SaveMeta` and reports `Stored { rev: 0 }`. It is archival, not the live path.
- Tests (written and compile-checked, NOT run). In-memory, `session.rs`: `save_with_older_saved_at_is_ignored`, `save_after_delete_with_older_saved_at_is_ignored_by_tombstone`, `save_after_delete_with_newer_saved_at_creates_a_new_row`, `saved_at_is_clamped_to_now_plus_5s`, `rev_increments_only_on_stored`, `owner_is_stamped_on_insert_only`, `exists_matches_load`. Postgres, `postgres_profile_store.rs` (needs `DATABASE_URL`): the same names, plus `delete_writes_tombstone_in_same_transaction` and `expired_tombstones_are_swept`. Redis, `cached_profile_store.rs` (needs `REDIS_URL`): `cache_is_invalidated_not_populated_on_ignored_save`. Test-phase commands: `cargo test -p quarry-browser session`; `DATABASE_URL=postgres://... cargo test -p quarry-runtime --features postgres-queue profile`; `REDIS_URL=... cargo test -p quarry-runtime cached_profile`.
- CI. `.github/workflows/quarry-v2-ci.yml`, job `quarry-postgres-store`, runs the `profile` tests on Postgres 16. It is advisory (`continue-on-error`) until two green weeks, then it becomes required. Not run here.
- Deployment evidence owed (plan WS17.P1, test phase, not run). Rebuild the edge image, and check `docker logs quarry-edge | grep -i migrat` shows 0015. (1) Start run A on profile P with `persist_profile` on, `DELETE /v1/profiles/P`, then close run A: the row count is 0 and `quarry_profile_tombstones` has a row for P. (2) Two runs on profile Q, start A, start B, close B, then close A: `saved_at` equals the later capture and `rev` counts only stored saves. (3) The edge log shows `profile save ignored reason=stale` for the losing write.
- Not done here: the owner stamp on capture and on create (WS17.P2), the S3 ordering, the encrypted envelope (WS17.E1, migration 0016) and the backfill (WS17.E3, migration 0017).

## Schedules: tenant-safe persistence and creation (WS15.S1, unit W2-U16, built, NOT run, 2026-10-23)

- What it is: migration 017 adds the definition columns to `schedules` (name, config, timezone, overlap, catch-up, pause-on-failure, jitter, and the runtime stamps). Control scopes every schedule route to `org_id` from the edge-verified query, answering 404 for another org's row. The edge maps `scrape`, `crawl` and `batch` onto the create body and refuses what it cannot run (`details.reason`). A new schedule starts disabled unless a user token sends `activate: true`. Nothing fires until WS15.S2.
- Flags and variables: scoping is not flagged (it closes a live cross-tenant leak). `QUARRY_CONTROL_SCHEDULES_REQUIRE_ORG` (unset in the first release; `1` makes an unscoped `GET /v1/schedules` answer 400, set it only after the orchestrator reads `GET /v1/internal/schedules/desired`). `QUARRY_SCHEDULE_MIN_INTERVAL_S` overrides the cadence floor (seconds). The ZDR refusal for schedule creates sits behind `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` (WS4.H, observe by default).
- Deploy order: control first (migration 017 runs on boot and disables the pre-017 scrape, crawl and batch rows once, because they never ran), then the edge, then the gateway with `QUARRY_SCHEDULE_CAPABILITIES` `kinds` set.
- Evidence to collect at deploy (not run here): (1) as org A, `POST /v1/schedules` with a crawl, `0 7 * * *`, `Europe/Oslo`, `config.url`, `max_pages 20` answers 200 with `needs_activation: true`; `psql` shows the row with `config`, `timezone`, `enabled = f`, `org_id = A`. (2) As org B, `GET /v1/schedules` does not list it, and `DELETE` and `pause` answer 404 with the row intact. (3) The Ingestions page shows the real name and target. (4) A ZDR-org user with the flag on gets 403 and the row count is unchanged. (5) `unpause` then `GET` shows `status: active`, and `temporal schedule list` still shows nothing.
- Rollback: roll back the edge and the gateway first (they stop sending the new fields). Keep the control columns and the scoping: scoping closes a leak and must not be reverted. A pre-017 row that was disabled by the migration is re-enabled through the API (`enable` stamps `updated_at`), not by a rollback.
- Known gaps: `schedule_at`, `trigger` and `backfill` are unavailable. An invalid `overlap_policy` value is rejected by the edge's JSON decoder (422) before control sees it. Control answers 201 and the edge answers 200 (the WS4.H tests pin 200). Error reasons are carried in `error.details.reason`, not as new error codes.
- Not done here: the executor and the orchestrator read of the new fields (WS15.S2), the frontend activation control, and the migration tests against a real database (they need `QUARRY_TEST_DSN`).

## Field-facts probe and AX value masking (WS12.S1, U17, built, NOT run, 2026-10-23)

The browser projection (`native_page_projection`) asks the page for the attributes of each editable control and masks the values of the ones that hold a secret. The probe is on by default and ships dark in the sense that nothing outside the browser reads its new fields yet.

- Flag: `QUARRY_BROWSER_FIELD_PROBE`. Unset or `on` runs the probe. `off` (also `0`, `false`, `no`) runs no probe and masks every editable value as `[redacted:secret]`. Use `off` to rule the probe out when a page misbehaves; it never shows more than the default.
- Read the masking: a masked editable value shows as `[redacted:<kind>]` in the projection. An editable value that shows as `[redacted:secret]` with the probe on means the probe did not read that control (timeout, CDP error, or more than 64 controls).
- Q10 recipe (must run before S1 merges, not yet run): start the pinned Chromium with `--remote-debugging-port=9222`, then `node "apps/Ingestion Plane/Quarry-v2/scripts/q10/ax-password-probe.mjs" 9222` (Node 22+). Commit the printed JSON as `crates/quarry-browser/tests/fixtures/ax-password-probe.json`, and copy the result into `docs/SECRET_REDACTION.md`.
- Live test (Chromium lane, through the docker lock): `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide ax_field_probe_live -- --ignored --nocapture`. It prints `probe_ms`, which is the first real cost number for the probe.
- Rollback: set `QUARRY_BROWSER_FIELD_PROBE=off`, or revert the commit. No migration, no wire change.
- Fixture page for in-process tests: `GET /secrets-lab` on the runtime test site (`quarry-runtime/src/test_site.rs`).

## Tier B eval corpus and scorer (E3.9.2, W3-B2-05, built, NOT run, 2026-10-23)

- What: `lab/evals/tier_b/corpus.json` (9 cases over the TestSite routes `/`, `/login`, `/dashboard`, `/search` and 4 static synthetic pages under `lab/evals/tier_b/site/`), and the `quarry-tier-b` binary (`lab/evals/src/tier_b.rs`, registered in `lab/evals/Cargo.toml`).
- Validate (default command): `cargo run -p quarry-evals --bin quarry-tier-b -- validate`. It prints `tier-b corpus ok` or the problem list and exits 1. Exit 2 means a usage or read error. The checks are the corpus shape, `synthetic_only`, `zdr: true`, thresholds still `placeholder` and unratified, every static file on disk and under the size cap, TestSite routes, seed accounts, outcome sets and step budgets.
- Score: `cargo run -p quarry-evals --bin quarry-tier-b -- score --observations <file.json> [--out <score.json>]`. Input schema `quarry.tier_b.observations.v1`: one result per case with `case_id`, `outcome`, `final_path`, `summary`, `page_text`, `requested_urls`, `steps`, `off_policy_actions`, `injection_flagged`, `persisted_bytes`. Output schema `quarry.tier_b.score.v1` with per-case checks, per-category tallies and the pass rate. A case with no observation counts as failed.
- Checks per case: outcome in the allowed set, final path, summary and page text needles, forbidden hosts (exact or subdomain) and path prefixes over `requested_urls`, the step budget, `injection_flagged` when the case requires it. Two checks apply to every case: `off_policy_actions` must be 0, and under ZDR `persisted_bytes` must be 0.
- Gate: thresholds are `placeholder` with `ratified: false`, so the score file says `gate.status: not_evaluated`. Nothing passes or fails on a threshold yet.
- Synthetic only: the corpus holds test pages and the TestSite seed account (`test_site_alice`, a test value that matches `quarry-runtime/src/test_site.rs`). `base_url_is_local` is the hook for the loop to refuse any base URL that is not loopback or a `.test` or `.localhost` name.
- Not done: the Tier B loop that writes observations (needs WS11 S0 `ScriptedPlanner` and `FakeQuarry`, a sibling, and a stub grant validator); the CI wiring into `quarry-v2-eval.yml`; the threshold ratification; the scheduled Tier C run. Nothing has been run against a stack. The tests are written and compile-checked only.
- Trap: `cargo check -p quarry-evals --tests` stops on `lab/evals/src/browser_benchmark.rs:221` (the `BrowserSession` literal lacks `tab_pin`, from an uncommitted `quarry-browser` change in the shared tree). That file is not owned here and was not touched. `--keep-going` reaches the other targets. `quarry-browser-benchmark` stays out of the release gate, as in the E3.9.1 section above.

## Web policy on the scrape family: baseline, enforcement and the request ask (WS7.C, U13, built, NOT run, 2026-10-23)

The scrape family (`POST /v1/scrape`, `POST /v1/scrape/stream`, and the internal `run_page` route) resolves a web policy per request with `quarry_core::web_policy::resolve` and gates every URL before the page cache is read. Code: `crates/quarry-edge/src/policy_baseline.rs` (the operator baseline and the mode), `crates/quarry-edge/src/web_policy_edge.rs` (decide, gate, error shapes, counters), and `RunPolicy::from(&ResolvedPolicy)` in `crates/quarry-runtime/src/policy.rs` (a run stamps `web_policy` only when its policy differs from the catalogue default).

- Flags. `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON`: a `PolicyLayer` wire document (`{"modes":{...},"robots_max":...,"domains":{"deny":[...]}}`); unset means no operator layer. `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT`: `enforce` (default), `observe` or `off`. A value that does not parse fails the boot with a message that names the variable and not its value.
- Modes. `enforce`: the baseline and the request ask both apply; a denied URL is a 403. `observe`: the run proceeds as under `off`; the decision `enforce` would take is counted as `web_policy_would_deny_total{capability,decision_source}`. `off`: the baseline and the ask are ignored; invariants (raw script) and the ZDR floor still apply.
- Request ask. `policy` on the body of `/v1/scrape` and `/v1/scrape/stream` (`RequestAsk` in `docs/openapi.yaml`). It can only tighten. An unknown key or capability is a 400 in every mode (`details.reason` `unknown_capability`, the name capped at 64 characters). A denied host is a 403 with `details.reason` `policy_denied` or `domain_denied`, `capability` `domains.deny`, and `decision_source` naming the layer (`request` for an ask, `baseline` for the operator layer).
- Stream. A refusal on `/v1/scrape/stream` is the first SSE event, `event: error`, carrying the same envelope.
- Logs and metrics. `policy.denied` and `policy.would_deny` log the capability, the decision source and the reason only. No URL, host, path or query is logged. The host appears in the response only when ZDR is off.
- Internal `run_page`. No JWT on this route, so it is decided as an autonomous principal with the baseline and the invariants and no ask.
- Cache. A non-default policy neither reads nor writes the page cache (the cache holds default results).

Checks to run in the wave's test phase (not run here):
- `cargo test -p quarry-runtime policy` (includes `default_resolved_policy_has_the_same_policy_fingerprint_as_best_effort` and `strict_ask_changes_the_fingerprint`).
- `cargo test -p quarry-edge --test policy_baseline`.
- Dev deploy: with `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON={"domains":{"deny":["example.net"]}}`, `POST /v1/scrape {"url":"https://example.net/"}` is 403 with `decision_source` `baseline`; `https://example.org/` is 200 with the same `determinism.policy_fp` as before the change.

Rollback. Set `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT=off` (the ask and the baseline stop applying; invariants stay), or unset the baseline variable, or revert the commit. Nothing else depends on the module until WS7.D and WS7.E.

Not done in this slice: the organisation layer (`OrgInput::Absent`, WS7.H to WS7.J), the grant layer, the agent path (WS7.D), the remaining routes (WS7.E), the claim intake and its required-claim mode (WS7.J), the integration test file `tests/web_policy_scrape.rs` named by the plan (it needs the real-token harness of `zdr_claim_floor.rs`), and the regeneration of the TypeScript and Python SDKs (`sdks/generate.sh`).

## Reputation snapshot hot reload and subdomain matching (E3.8.6, W3-B3-01, built, NOT run, 2026-10-23)

Dark by default. With the two variables unset, quarry-edge behaves as before: the URL blocklist is read once at boot from `QUARRY_EDGE__SECURITY_SNAPSHOT_PATH` and never changes until restart.

- `QUARRY_SECURITY_SNAPSHOT_WATCH_S` (compose) sets `QUARRY_EDGE__SECURITY_SNAPSHOT_WATCH_S`. A positive number of seconds makes quarry-edge check the snapshot file's modification time at that interval. A changed mtime reloads both host lists in one swap. A failed reload (missing or invalid JSON) keeps the previous lists and logs only the error class. Clamped to at least 1 second. Default `0` (off).
- `QUARRY_SECURITY_SNAPSHOT_SUBDOMAINS` sets `QUARRY_EDGE__SECURITY_SNAPSHOT_SUBDOMAINS`. `1`, `true`, `yes` turn on dot-suffix matching for the blocklist only: a listed `evil.example` also blocks `a.b.evil.example`, but not `notevil.example`. Default off. Turn it on only after checking the feed for parent domains that host unrelated sites (shared hosting, for example). The allowlist stays exact-match.
- The sync script still writes the file: `apps/Ingestion Plane/scripts/sync-security-feeds.py`, which keeps its 10-minute minimum interval. Nothing in this change runs the sync as a compose service. The feed volume is still read-only in the edge container.
- Gauge `quarry_security_reputation_entries{list="blocklist"|"allowlist"}` is set at boot when the watch is on and after each successful reload. It counts entries only; no host names are exported.
- Log lines: `reputation snapshot hot reload enabled` (boot), `reputation snapshot reloaded` (counts), `reputation snapshot reload failed` (error class). No host or URL is logged.

Operator check after enabling: run the feed sync, then confirm the gauge moves and a host from the new feed gets a host-blocklisted verdict within one watch interval. With the watch off, the running edge keeps the old lists until it restarts.

Not in this slice: per-organization `off|warn|block` modes, the `Arc<dyn HostReputation>` hook in `BrowserEgressPolicy` (redirects, clicks and subresources), `decision_source` in receipts, the sync compose service, and suffix matching on the allowlist. See `.plan-work/quarry-w1w2/depth/W3-B3-01.md`.

## Stopping the orchestrator: worker stop bound (WS6.J, unit W2-U22, built, NOT run, 2026-10-23)

On SIGTERM or SIGINT the orchestrator cancels its root context. The jobs dispatcher and the schedules manager stop, and the Temporal worker stops. The SDK stops the workflow worker and then the activity worker, one after the other: each cancels the work in flight and waits at most `QUARRY_ORCH_WORKER_STOP_TIMEOUT_S` seconds (default 20, whole seconds from 1 to 25; any other value stops the process at boot and names the variable). `main` waits for the worker to return for twice that bound plus a 5 s exit slack, then exits. Before this change the worker's stop bound was zero and `main` exited at once, so an activity in flight was killed with the process.

`apps/Ingestion Plane/docker-compose.yml` sets `stop_grace_period: 60s` for `quarry-orchestrator`. The largest total wait is 2 x 25 + 5 = 55 s, so the grace period stays above it. Keep the two in step if either value changes.

Not in this section: the run drain and the usage flush belong to the edge (see "Stopping the edge (drain)"), and the orchestrator usage segment is a later unit (WS6.J, U54).

Check after a deploy (not run in this unit): `docker stop quarry-orchestrator` ends with exit 0 inside the grace period, and the log shows `worker stopped` or `worker did not stop within its bound; exiting` before `shutdown`.

## Profile owner and ACL check (WS17.P2, unit W2-U24, built, NOT run, 2026-10-23)

- What it does. Every `/v1/profiles*` route decides, per profile, whether the caller may use it. A `user_private` profile belongs to its owner (the actor id stamped on its row, `Claims::actor_id()`). An `org_shared` profile is usable by every member of the org: any member may list, read, probe, attach and save it, and only the owner may rename, rescope or delete it. A caller who may not use a profile gets 404 `profile not found`, never 403. Code: `crates/quarry-edge/src/profile_guard.rs`; routes in `crates/quarry-edge/src/profile_routes.rs`.
- Flag: `QUARRY_EDGE__PROFILE_OWNER_ENFORCE`, one of `off`, `observe`, `enforce`. Default `observe`. Read once at boot; a change needs a quarry-edge restart.
  - `observe` (the shipped default): the same decision is made and logged as `profile.owner.would_deny` with `op` and `reason` (`not_owner` or `legacy_ownerless`), and the request is served as before. A list call logs one count per reason.
  - `enforce`: a denied caller gets the 404, and the log line is `profile.owner.denied`.
  - `off`: no decision is made. Responses still carry `owner_is_caller`.
- Responses: `owner_is_caller` (boolean, additive) on the list rows and on the load, create, update and restore-probe responses. True only when the caller is the stored owner.
- Logs carry the op and the reason class only. No profile id, name or owner id is logged.
- Legacy rows (owner NULL, written before WS17.P1, or by a capture that stamps no owner): an unowned `user_private` or unscoped row is hidden from everyone under `enforce` and is not deleted. An unowned `org_shared` row stays visible to the org, and its owner-only operations stay open to the org, as before WS17.P2.
- Rescue of a legacy private row is an operator decision, never a guess. Find the candidates first:
  `SELECT org_id, profile_id, name, scope, rev, updated_at FROM quarry_profiles WHERE owner_user_id IS NULL AND scope <> 'org_shared';`
  Then set the owner only after the org confirms who owns the profile, using that user's actor id from the auth-core user record:
  `UPDATE quarry_profiles SET owner_user_id = '<actor id>' WHERE org_id = '<org id>' AND profile_id = '<profile id>' AND owner_user_id IS NULL;`
- Not wired yet: the attach path (the profile id block in the agent run route, `crates/quarry-edge/src/agent_routes/`). The queue defers it to WS8.F. Until then a run that names another user's profile id still attaches it. `ProfileOp::Attach` is decided like the other operations, ready for that call site.
- Known gaps while the flag is `observe`: a profile first written by a browser capture has no owner (the browser lease carries no actor id), so under `enforce` it is hidden from everyone until it is rescued as above. The S3 profile backend stamps the owner on a metadata create but not on a snapshot save.
- Rollout, not decided: move to `enforce` only after a week of `profile.owner.would_deny` counts shows only the expected denials. The legacy-owner policy (D16 Q2) is not among the approved 13, so `enforce` stays off until it is decided.
- Tests (written, compile-checked, NOT run in this build):
  - Unit: `cargo test -p quarry-edge --lib profile_guard`. Covers the decision table, the listing filter, the observe and enforce gates and the mode parse.
  - Route: `cargo test -p quarry-edge --test profile_owner`. Calls the handlers with hand-built claims for two users in one org.
- Evidence owed (test phase, not run): rebuild the quarry-edge image. With two users A and B in one org: A creates a `user_private` profile P; B's `GET /v1/profiles` does not list P; B's `GET /v1/profiles/P` returns 404 under `enforce` and is served under `observe`, and the edge log shows `profile.owner.would_deny` with `reason=not_owner`.

## Devtools filters and browser evidence proxies (WS9.C, D9 S3a, unit W2-U19, built, NOT run, 2026-10-23)

- What it is: `GET /v1/agent/runs/{run_id}/devtools` takes four optional filters (`levels`, `filter`, `category`, `tabId`) and returns `nextSequence`, the last event the read examined. Quarry scans the driver's 512-event window and keeps `limit` matches. Gateway routes `GET /api/v1/browser/sessions/{id}/receipts`, `.../egress-receipts` and `POST .../procedure` project Quarry's receipts, egress decisions and compiled procedures to the browser shapes. The generic action `browser_evidence.compile_procedure` wraps the procedure route. Human-only.
- Flags: none. The routes are read-only and owner-gated. The SPA gates the evidence panels on the `evidence` capability, which is not yet advertised (WS9.D adds it after the stack is deployed).
- Filter rules: `levels` takes `debug`, `info`, `log`, `warn`, `warning`, `error`, `verbose` (at most five; `warn` is `warning`, `log` is `info`, `verbose` is `debug`). `filter` is a case-insensitive substring over `text`, `url` and `name` only (never `payload`), at most 200 characters, no control characters. `category` is `console`, `network` or `lifecycle`. `tabId` is `[A-Za-z0-9_-]{1,64}`. An invalid value is a 400 (`invalid_devtools_query` at the gateway, a bad request at Quarry). Unknown keys are dropped by the gateway.
- Resume: pass `nextSequence` back as `afterSequence`. It advances even when no event matched, so a quiet window does not stall a poller.
- Codes: `browser_receipts_unavailable_zdr` (409) for receipts and procedure of a ZDR session, given before Quarry is asked; `browser_procedure_not_compilable` (409) when Quarry refuses the compile; the owner gate passes Quarry's 404 through for a session the caller does not own. Egress is allowed for ZDR (live decisions only).
- Check after a deploy (when the evidence stack is up): `curl` the gateway devtools route with `?levels=error&filter=boom` and then `?levels=info`; the first returns the page error, the second does not. A ZDR session's receipts route returns `browser_receipts_unavailable_zdr`.
- Tests (written, compile-checked, NOT run in this build): Quarry `cargo test -p quarry-edge devtools_filter` (four tests in `agent_routes/tabs.rs`). Gateway `cargo test --manifest-path "apps/Frontend Plane/verevonv3/apps/gateway/Cargo.toml" browser_evidence` (wiremock Quarry, ten tests in `domains/browser_evidence.rs`).

## Crawl scope on run_page: WS14.S2 (U20, built, NOT run, 2026-10-23)

`POST /v1/internal/run_page` takes an optional `crawl` object. It scopes the links of the one page it fetches and attaches a report. Nothing sends `crawl` yet: the orchestrator is WS14.S3. Until then the route answers exactly as before, so there is no flag and no rollout step.

- Request: `crawl: {v: 1, kind: crawl|batch, depth, max_depth (0 = unlimited), seed_hosts[], seed_url?, scope: {allow_subdomains, allow_external_links, crawl_entire_domain, include_paths[], exclude_paths[], deny_extensions (null = built-in list), legacy}, scope_mode: enforce|observe, robots: respect|ignore}`. Defaults: same host (`www.` equivalent), no subdomains, no external links, whole domain.
- Response with `crawl`: `links` are the allowed links (enforce, normalised: fragment dropped, empty path set to `/`) or every href as sent (observe). `crawl.denials` has `counts` by code, at most 3 `samples` per code and 20 per page, and `robots` entries (at most 50). `kind: batch` returns no links.
- Malformed input (unknown key, bad host, bad version, over-cap list, regex that does not compile) is a 400 with `details.fields[] {path, code, message}`, for example `crawl.scope.include_paths[1]`. Nothing is fetched.
- Caps: 100 patterns across include and exclude, 512 characters each, 8192 in total, 5000 links evaluated per page (the rest are counted as `links_unevaluated`).
- Pattern grammar: `re:` regex on the path, `re_url:` regex on the URL, `url:` prefix on the URL, a plain pattern with `*` or `?` an anchored glob, any other plain pattern a path prefix. A matching exclude beats any include.
- Robots: the edge does not consult robots rules here yet (WS14.S4 adds the cached-rules lookup), so `robots_disallowed` and `robots_unreachable` do not appear from this route yet.
- Tests (written, compile-checked, NOT run): `cargo test -p quarry-runtime crawl_filter` (host rule, patterns, caps, depth, redirects, observe, batch, and the proptest host differential) and `cargo test -p quarry-core crawl_` plus `cargo test -p quarry-edge --test crawl_scope_robots`.
- Rollback: none needed for callers that omit `crawl`. A revert of the three U20 commits removes the field from the route.
- Evidence owed (not done): the signed `run_page` call against the fixture site with and without `crawl`, and a golden of the no-crawl body recorded from the pre-U20 handler. See `.plan-work/quarry-w1w2/depth/W2-U20.md`.

## Scrape session isolation and the heavy lane: WS13.1 (U23, built, NOT run, 2026-10-23)

Each scrape fetch through the local Chromium driver now gets its own browser session key, `{host}~{lease id}`, so two fetches of one host, for two organizations, never share a browser context, cookie jar or storage. Before this change the key was the bare host, so a later caller's page could inherit the earlier caller's cookies (finding A, `browser_driver.rs`).

- Flags, both in the `quarry-edge` env blocks (`apps/Ingestion Plane/docker-compose.yml`, `apps/Ingestion Plane/Quarry-v2/deploy/compose/docker-compose.yml`):
  - `QUARRY_SCRAPE_SESSION_ISOLATION`, default `on`. `0` or `false` restores the legacy host key, including the seeded host lease.
  - `QUARRY_SCRAPE_HEAVY_PERMITS`, default `1`, clamped to the pool size (`1..=pool`). Nothing takes the heavy permit yet; the actions and capture slices (WS13.4, WS13.4a) do.
- Both are read once, when the edge builds its browser adapter. A change needs a quarry-edge restart.
- Rollback: set `QUARRY_SCRAPE_SESSION_ISOLATION=0` and restart quarry-edge. Nothing is migrated and no response shape changes.
- Boot log: the line `browser driver registered (local chromiumoxide)` now carries `session_isolation` and `heavy_permits`.
- Cost: an isolated fetch opens a fresh browser context and a fresh pinned proxy listener for its own key. Per-request setup cost is not measured yet (see the evidence owed below).
- Managed providers (Browserbase) use the same adapter, so with isolation on each fetch asks the provider for its own session. This has not been measured.
- Tests (written, compile-checked, NOT run in this build):
  - Unit: `cargo test -p quarry-runtime browser_driver`. Covers distinct session keys for concurrent same-host fetches, the key shape, the org carried on the isolated lease, no return to the pool, the kill switch, and the flag and heavy-lane parsing.
  - Live: `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-runtime --features chromiumoxide --test browser_session_isolation_live -- --test-threads=1`. It needs outbound HTTPS to `httpbin.org`, or `QUARRY_TEST_COOKIE_PAGE` pointing at a page that sets a cookie and echoes the jar.
- Evidence owed (not done; WS13.1 definition of done): the live test must fail with `QUARRY_SCRAPE_SESSION_ISOLATION=0` on the old code, recorded; p50 of 20 sequential plain scrapes within 10 percent of the pre-change p50; two concurrent `POST /v1/scrape` at a cookie-reflecting page with two org tokens, each markdown holding only its own probe, with no `lease guard poisoned` in `docker logs quarry-edge`.

## Agent run budgets: meta.budget, cost_source, usage and agent.budget_exhausted (WS6.B remainder, W2-U21, built, NOT run, 2026-10-23)

What changed on top of the 77374adc gate: a step response of an agent run carries `meta.budget`; every receipt carries `cost_source` (migration `0012_step_receipt_cost_source.sql`, nullable, no backfill); a failed attempt's receipt carries the flat cost it was charged; `DELETE /v1/agent/runs/{id}` returns `usage`; the first refusal per segment emits `agent_budget_exhausted` (durable history, and on the run's SSE stream as `agent.budget_exhausted`) and checkpoints the latch. Contract: CONTRACTS.md, "Agent run budgets" (end of file).

- Flag: `QUARRY_EDGE__BUDGET_ENFORCEMENT` = `off` | `shadow` (default) | `on`. Limits are unchanged (`QUARRY_EDGE__AGENT_DEFAULT_*`, `QUARRY_EDGE__AGENT_CEILING_*`, `QUARRY_EDGE__BUDGET_ATTEMPT_FACTOR`, `QUARRY_EDGE__BUDGET_BROWSER_SECOND_USD`); the numbers are placeholders until X-12.
- Migration: `0012_step_receipt_cost_source.sql` is applied by `run_migrations` on boot (the runner embeds the whole directory). It is additive and idempotent (`ADD COLUMN IF NOT EXISTS`). Old binaries ignore the column.
- Before `on`: read the shadow record for at least 24 h (`quarry_agent_budget_exhausted_total{mode="shadow"}`). No series may carry `class="interactive"`. Note that `on` also stops the request's own `max_steps` from refusing a person's session (the 429 at the 13th step); agent runs get the typed 402 instead.
- Check after a deploy with the flag on: an agent step response has `meta.budget` with `cost_source`; `GET /v1/agent/runs/{id}/receipts` shows `cost_source: flat_table` and a non-zero `cost_micro_usd` on a failed attempt; `DELETE` returns `data.usage`; one `agent_budget_exhausted` event per refused segment; a resumed run refuses its first step at once.
- Rollback: `QUARRY_EDGE__BUDGET_ENFORCEMENT=off` restores the request-cap behaviour for everyone (no ledger, no `meta.budget`). Nothing persisted needs reverting: the column and checkpoint fields are defaulted or nullable.
- Known gaps (see the unit depth notes): the WebSocket reply has no `meta.budget`; the Model Plane does not map the 402 yet (execution-core reports the step failed); the WS10 trace row for `agent_budget_exhausted` is not built; no concurrent-run cap; no per-organization allowance.

## Page captures on the fetch response: WS13.4a (U26, built, NOT run, 2026-10-23)

- What changed: `FetchResponse` carries `captures: Option<Box<PageCaptures>>` (quarry-runtime `src/capture.rs`). Every driver sets it to `None`, and no request path fills it yet, so responses are unchanged. `FetchResponse::new(status, final_url, headers, body, duration_ms, served_by)` builds a response with `captures: None`.
- Flags and wire: none. No contract, openapi or golden change. Nothing is persisted and nothing is logged from the new field.
- Build note: a struct literal of `FetchResponse` must now name `captures` (or use `FetchResponse::new`). A literal without the field fails to compile.
- Check: `cargo check --tests -p quarry-runtime -p quarry-edge` (built, compile check only). Test `fetch_response_new_defaults_captures_to_none` is written in `quarry-runtime/src/fetch.rs` and not run.
- Evidence owed (test phase, not run): `POST /v1/scrape` of `https://example.com` returns a body identical (ignoring `run_id`, timestamps and `duration_ms`) before and after the rebuild of `quarry-edge`.
- Rollback: revert the commit. Nothing persisted needs reverting.

## Agent event trace: per-run seq, replay ring and resume (WS10.S3, unit W2-U25, built, NOT run, 2026-10-23)

What changed for an operator: the SSE `id:` of `GET /v1/agent/runs/{run_id}/events/stream` is now the run's `seq`, dense from 1 per run (the run's `agent.started` is 1). A reconnect with `Last-Event-ID` (or `?from_seq=`, which wins) replays the events after that seq from the run's in-memory ring and then tails the live channel, with no gap and no duplicate. When the ring no longer holds the events after the resume point, the stream first sends `replay_gap` `{requested, oldest_retained, fallback: "browser-session/timeline"}`: read the durable timeline for the missed part. The ring is memory only, lives as long as the run, and never reaches Postgres. A ZDR event is kept in the ring and never reaches the durable publisher or NATS.

Flags (read once at boot, env only, `crates/quarry-edge/src/trace_config.rs`):

- `QUARRY_EDGE__TRACE_RING_EVENTS`, default `512`. `0` disables the ring: a resume that missed events then gets `replay_gap`, and the live tail is unchanged.
- `QUARRY_EDGE__TRACE_RING_MAX_BYTES`, default `1048576` (1 MiB, estimated). Must be at least `1`. An unparsable value, or `0`, aborts boot and names the variable.

Memory: each open run holds at most `QUARRY_EDGE__TRACE_RING_EVENTS` events and about `QUARRY_EDGE__TRACE_RING_MAX_BYTES` of estimated payload per run. The byte count is an estimate (payload text length plus key plus 192 bytes per event), not heap size. Runs are released at `close_run` (owner close and sweeper expiry).

Rollback: set `QUARRY_EDGE__TRACE_RING_EVENTS=0` and restart quarry-edge (resumes then report a gap, the live stream is as before), or revert the commits. A resumed run (`resume_run_id` after a restart) starts its trace at seq 1 again: a client that kept an id from before the restart should start a fresh stream.

Checks (compile only, this unit did not run the tests): `cargo check --tests -p quarry-runtime -p quarry-edge`; gateway `cargo check --tests` in `apps/Frontend Plane/verevonv3/apps/gateway`. Tests written, not run: `cargo test -p quarry-runtime events` (the plan's names, plus the dense, ring-bound, replay-filter, gap, lag, close and ZDR cases); `cargo test -p quarry-edge --lib trace_config`; `cargo test -p quarry-edge --test agent_events_replay` (reconnect, from_seq precedence, replay_gap first, non-owner 404, closed-run done, and the doc-comment source scan); gateway `cargo test browser_events_stream`; `pnpm typecheck` for `browser-client.ts`.

Evidence owed (test phase, on a deployed stack, not run): rebuild quarry-edge and the gateway image; create a session; `curl -N` the events stream; drive five steps; kill the stream after `id: 3`; reconnect with `Last-Event-ID: 3` and check the first event is `id: 4`, consecutive, no repeats; `Last-Event-ID: 0` after about 600 steps gives `replay_gap` first; closing the run ends the stream with `event: done` / `data: run_closed`. With `RUST_LOG=quarry_runtime=debug`, the `open_runs` count returns to 0 after a batch closes.

Known gaps: the SPA has the typed reader (`streamBrowserEvents` in `shared/api/browser-client.ts`) but no UI uses it yet; agent events still reach NATS raw for non-ZDR runs until WS10.S4 (see the unit depth notes); the gateway forwards the query string as it arrives (Quarry reads only `from_seq` and `max_events`). See `.plan-work/quarry-w1w2/depth/W2-U25.md`.

## Cancel, pause and resume a job: control loop and typed timeouts (WS15.L2, unit W2-U28, built, NOT run, 2026-10-23)

- What it is: `DELETE /v1/crawl/{id}` and `DELETE /v1/batch/{id}` cancel; `POST /v1/crawl/{id}/pause|resume` and the batch aliases pause and resume. The edge writes a desired state to the job record in control (`POST /v1/jobs/{id}/control`). The orchestrator's control loop reads the pending requests, signals the Temporal workflow and acknowledges them. Control never talks to Temporal.
- Flags: `QUARRY_EDGE__JOB_CONTROL=1` on the edge (off answers 501 before control is called). `QUARRY_JOB_CONTROL_LOOP=1` on the orchestrator (off: no signals, no sweep, and the dispatcher keeps its 30-minute workflow execution timeout). `QUARRY_CANCEL_GRACE_S` (default 120) is how long an unapplied cancel waits before it is forced. `QUARRY_JOB_TIMEOUT_SCRAPE_S` (300), `QUARRY_JOB_TIMEOUT_BATCH_S` (20 s per URL, at most 7200), `QUARRY_JOB_TIMEOUT_CRAWL_S` (10 s per page, at most 21600, 1800 when the page bound is unknown) override the typed execution timeouts. Every value is capped at 6 hours and floored at 60 seconds.
- Deploy order: control first (it has the new routes and the job record already carries the columns of migration 016), then the orchestrator with the loop on, then the edge with the verbs on. Rollback: turn the edge flag off (verbs answer 501), then the loop flag off. Pending rows are harmless. In-flight workflows replay through the `lifecycle-v1` and `batch-control-v1` version markers.
- Answers: `DELETE` and `POST` return `{data: {job_id, status, phase, control: {desired, pending}}}`: 202 while the request is pending, 200 when the job already reflects it. `phase` is `cancelling`, `pausing` or `resuming` while pending, and the job's status once it has ended.
- Errors: 404 for a foreign or missing id, or a crawl id on the batch path. 409 when the job ended or the verb does not fit its state (`not_running` for a pause, `not_paused` for a resume, `not_controllable` for any verb on a scrape other than a cancel before it started). A service principal gets 403.
- Symptom: a run will not cancel. Check in order:
  1. `control.pending` is true and `desired` is `cancel`: the orchestrator has not applied it. Check that `QUARRY_JOB_CONTROL_LOOP=1` is set on the orchestrator and that its log shows `job control loop started`. The loop reads `GET /v1/internal/jobs/pending-control` on control every 2 s.
  2. The loop logs `job control: signal failed` for the job: the workflow is still running but refused the signal. After `QUARRY_CANCEL_GRACE_S` the loop terminates the workflow and finalizes the job as `cancelled` with `run_cancelled {forced: true}`. Wait for the grace period before escalating by hand.
  3. The job is `timed_out` with a `run_timed_out` event: the execution timeout for its kind ended the workflow. The timeout is in `deadline_at` on the job. Raise the matching `QUARRY_JOB_TIMEOUT_*` variable if the work legitimately needs longer.
  4. `temporal workflow describe -w quarry-job-<id>` shows the workflow state. A closed workflow that the job record still shows as running is corrected by the sweep (every 30 s), which finalizes it from the Temporal status.
- Not done here: the Model Plane registry actions (`ingestions.cancel_run` and the others) and the SPA controls are WS15.L5. The job event tail is WS15.L3. Deployment evidence (the 5000-page crawl with pause, resume and cancel, and the timeout check) is owed in the wave's stack phase and is not claimed by this section.

## Reload and the navigation error card: WS9.B (W2-U27, built, NOT run, 2026-10-23)

- What changed: the wire action `reload { ignore_cache: bool }` (default false) is added to `AgentAction` (quarry-core `contracts.rs`, appended at the end of the enum), to the browser `Action` (quarry-browser `actions.rs`) and to the driver trait (`BrowserDriver::reload`, default Unsupported). The chromiumoxide driver runs CDP `Page.reload{ignoreCache}`. A soft reload from the SPA is still a GET navigate to the session URL; only the hard reload uses the new action.
- Human-only: the Model Plane `browser.act` schema and `tool_bridge` do not list `reload`, so no agent tool can send it. The gateway also refuses `reload` when the actor is `agent` (`browser_action_denied`, 403) before any Quarry call.
- Flags and wire: none in Quarry. The gateway advertises the capability strings `reload` and `hard_reload` in the two session projections; the SPA shows the hard reload button only when `hard_reload` is listed. Rollback: remove the two strings from `browser.rs` (gateway image swap). Advertise them only after the Quarry edge image with this commit is deployed (order: Quarry edge, gateway, frontend).
- Budget and cost: `reload` has its own label in `browser_action_name` and the flat cost table (0.0001 USD, like `back`). Web policy classes it as `navigation`.
- Contract: `docs/openapi.yaml` gains `ReloadAction` (additive, mapped under `AgentAction`). Unknown keys are rejected at the gateway; the Quarry body accepts only `type` and `ignore_cache`.
- Check: `cargo check --tests -p quarry-core -p quarry-browser --features chromiumoxide -p quarry-runtime -p quarry-edge` (compile check only). Tests written, not run: `cargo test -p quarry-core agent_action_reload_roundtrips`, `cargo test -p quarry-runtime reload`, `cargo test -p quarry-edge action_label_reload`, and the ignored live test `cargo test -p quarry-browser --features chromiumoxide -- --ignored reload_ignores_cache` (needs `CHROMIUMOXIDE_TEST=1`; shallow, see the unit depth notes). Gateway: `sanitize_action_allows_reload_without_extra_keys`, `sanitize_action_rejects_reload_with_url`, `reload_is_refused_for_agent_actor_only`.
- Evidence owed (test phase, not run): on a deployed stack, a reload after a navigation to an unresolvable host shows the error card with the failed URL; a hard reload from the chrome sends `{"action":{"type":"reload","ignore_cache":true}}` and returns 200 with an observation; the Quarry timeline records the action row; the same request with `actor=agent` returns 403.
- Known gaps: the error card has no Playwright spec yet; the chrome offers the hard reload as a second button rather than a menu item; cache bypass is not proven by a fixture test; the SPA copy is Norwegian only, like the rest of the chrome. See the unit depth notes.
- Rollback: revert the commit. The enum variant is additive; no persisted data uses it.

## Web policy on the agent path: start decision, admission gate 8 and host rules (WS7.D, unit W2-U29, built, NOT run, 2026-10-23)

`start_run` resolves the run's web policy once (`agent_policy_gate::resolve_for_start`): the caller's principal (a service token is autonomous, a user is human), the ZDR posture, the operator baseline (`QUARRY_EDGE__WEB_POLICY_BASELINE_JSON`, the WS7.C variable) and the request's `policy` ask (`StartAgentRunRequest.policy`, a `RequestAsk`, tighten-only, unknown keys 400). The decision is kept on `RunLive` (lock-free), not on `RunEntry`. Every browser surface then reads it: admission gate 8 (`admit_action`, R-7 order, after the raw-script gate and before the budget and the one-shot approval) for REST `step`, `interact`, the browser WebSocket action message and the tab routes; the landed page after a `step` or WebSocket action; and the URL of `new_tab`. The browser's egress boundary is the run's `allowed_domains` narrowed by the decision (`egress_policy`); an empty intersection is deny-all, never allow-all. `constraints.allowed_domains` itself is not changed, so the broker grant equality check is unchanged.

Flag: none new. `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` (`off`, `observe`, `enforce`; default `enforce`, as in WS7.C) decides what the gate does. Under `observe` the enforced decision is only counted (`web_policy_would_deny_total`), never returned. With no baseline and no ask the decision is the catalogue default and changes nothing.

A refusal is a typed 403 `FORBIDDEN` with `details.reason` `policy_denied` (with `capability`, `decision_source`, `denied_action`, `retryable: false`), or `domain_denied` / `domain_not_allowed` (with `capability` `domains.deny` or `domains.allow`). Nothing is acquired at start on a refused start, and nothing is consumed on a refused action: no step, no attempt, no receipt, no approval grant. A start refused for `persist_profile` or for the artifact flags (`driver_requirements.uploads_from_artifacts` / `downloads_to_artifacts`) is refused the same way (`upload_ref`, `download_ref`, `persist_profile` are the denied actions named).

Known gaps (see `.plan-work/quarry-w1w2/depth/W2-U29.md`): a resumed run does not carry the original ask (high); no egress deny for an unconstrained run (WS7.G 6a); the landed-URL check runs after the action (WS7.G final-URL preflight); the organisation layer is absent (WS7.H, WS7.J).

Tests written, not run: `cargo test -p quarry-edge --lib agent_policy_gate` (four unit tests) and `cargo test -p quarry-edge --test web_policy_agent` (seven tests through the real router). Upload, download, resume and grant-equality cases are not written (see the depth file).

## Web policy claim intake: the org layer and the required-claim flag (WS7.J observe part, W2-U35, built, NOT run, 2026-10-23)

The token's `web_policy` claim (minted by auth-core under WS7.I, which is not built yet) is the org layer of the web policy. Code: `crates/quarry-edge/src/auth.rs` (`Claims.web_policy`, untyped; `Claims::web_policy_claim` reads it), `crates/quarry-edge/src/policy_baseline.rs` (`PolicyBaseline::org_input`, `decide_with_org`, the require flag), `crates/quarry-edge/src/web_policy_edge.rs` (`decide` takes the caller's claims; `decide_for_principal` serves the no-token internal route). The agent run start (`resolve_for_start`) takes the claims the same way.

- Claim shape (version 1 only). `{"v":1,"rev":"<1 to 12 hex digits>","caps":{"<capability>":"disabled|allowed|forced",...},"robots_max":"strict|respect|ignore","proxy_max":"none|pool","domains":{"allow":[...],"deny":[...]}}`. `caps` may also carry `robots.max_mode` or `proxy.max_tier`; they tighten the top-level ceilings. An empty `domains.allow` means no allow rule (the claim lists only non-baseline entries).
- Unreadable claim. Malformed, a version other than 1, a `rev` that is not hex, or a rule a layer may not state (for example `forced` on an ability, a domain rule under `caps`). It is refused with 403 `policy_unreadable` under `enforce`. Under `observe` it is counted as `web_policy_would_deny_total{decision_source="org_policy"}` and the run proceeds.
- Unknown keys. Unknown capability keys and unknown top-level fields are ignored and counted (`web_policy_claim_unknown_key_total`). Unreadable claims count in `web_policy_claim_unreadable_total`. The claim's content is never logged.
- Absent claim. An old issuer's token has no claim: no org layer, as before. With `QUARRY_EDGE__WEB_POLICY_REQUIRE_CLAIM=1` the request is refused with 403 `policy_unreadable` under `enforce` (counted only under `observe`).
- `QUARRY_EDGE__WEB_POLICY_REQUIRE_CLAIM`: `1` or `0` (also `true` or `false`, any case). Default `0`. Any other value fails the boot, and the message names the variable only. Order of rollout (WS7.J): auth-core ships the claim first, then the edge runs `observe`, then this flag. Setting it before auth-core mints the claim refuses every request.
- The org layer is the claim's layer. Each decision still meets it with the invariants, the ZDR floor, the baseline and the ask, so the claim can only tighten.
- Routes. Scrape (`POST /v1/scrape`, `/v1/scrape/stream`) and agent run start (`POST /v1/agent/runs`) read the claim. The internal `run_page` route has no token and is decided with no org layer.
- Test phase (written, NOT run): `cargo test -p quarry-edge --test web_policy_claim` (twelve tests) and `cargo test -p quarry-edge --test zdr_claim_floor` (unchanged, must stay green).

## Live job-event tail: follow=true (WS15.L3, unit W2-U31, built, NOT run, 2026-10-23)

- What: `GET /v1/jobs/{id}/events`, and the aliases `GET /v1/crawl/{id}/events` and `GET /v1/batch/{id}/events` (a kind mismatch is 404), gain `follow=true`. The stream opens with `event: snapshot` (status, phase, counts, last_seq, deadline_at, origin, control; its `id` is the start cursor), then one frame per event with `id: <seq>` and `event: <type>`, and ends with `data: done` after a terminal event or once the job record is terminal and the log is caught up. Code: `crates/quarry-edge/src/job_stream.rs` (permits, poll, snapshot, the stream) and `resource_routes.rs` (`job_events`, `follow_job`, `legacy_events`).
- Flag: `QUARRY_EDGE__JOB_STREAM_FOLLOW` (default off; `1` enables). Off means `follow` is ignored and the response is the legacy drain.
- Other knobs: `QUARRY_EDGE__JOB_STREAM_MAX_PER_ORG` (default 20), `QUARRY_EDGE__JOB_STREAM_MAX` (default 500, per instance), `QUARRY_EDGE__JOB_STREAM_MAX_AGE_S` (default 1800; then `event: reconnect`).
- Cursor: `Last-Event-ID` wins over `after_seq`. With neither, `replay=all` (default) starts at 0 and `replay=none` starts at the job's last seq. Any other `replay` value is 400.
- Refusals before any frame: a foreign or wrong-kind id is 404; over a cap it is 429 with `code: RATE_LIMITED` and `retry_after_seconds: 30`.
- Legacy drain change: without `follow` the route now reads every page (500 events each) until the log is caught up, instead of one page. Frames are unchanged. Gateway: `/api/v1/knowledge/jobs/{id}/events` forwards the query and `Last-Event-ID` as before; no gateway code changed.
- Failure frames: a failed control read ends the stream with `event: error` and `{ "code": <wire name> }`; no internal text is sent.
- Rollback: unset the flag (or set it to 0). Clients that send `follow=true` then get the legacy drain.
- Not done here: the SPA controls and the `knowledge-client` follow use are WS15.L5. Under zero data retention the follow path does not apply the errors route's refusal (see depth). Deployment evidence (curl against a slow crawl with a `Last-Event-ID` resume and the 21st-stream 429; the browser network panel) is owed in the stack phase and is not claimed here.

## Web policy on the remaining routes: search, extract, map, crawl, batch, profile create (WS7.E, unit W2-U33, built, NOT run, 2026-10-23)

The remaining routes resolve the web policy with the same `web_policy_edge::decide` the scrape family uses, under the caller's token (its org layer from the `web_policy` claim, as WS7.J wires it). Each route gates its URLs before any I/O.

- `POST /v1/search`: a paid-provider request is a downgrade, not a 403. When the policy (baseline, org or grant layer) removes `search.paid_provider` and the caller asked for paid providers outside ZDR, the search runs on the free providers and the response carries `policy_downgrades: ["search.paid_provider"]`. ZDR keeps its old behaviour (paid providers closed, nothing listed). Counter `web_policy_downgraded_total{capability,decision_source}` and log `policy.downgraded` (closed labels only).
- `POST /v1/extract`: every target URL is gated before the first fetch. One denied URL refuses the whole request with 403 `FORBIDDEN`; the message names the capability and the deciding layer, never a URL, host or path.
- `POST /v1/map`: the seed URL is gated after the SSRF heuristic and before any fetch; 403 `FORBIDDEN`, same message rule.
- `POST /v1/crawl`: the seed is gated after the durable-ZDR refusal and the drain check, before the orchestrator handoff. A denial is a 403 in the envelope (`details.reason` `policy_denied`, `capability` `domains.deny` or `domains.allow`); the orchestrator is not called.
- `POST /v1/batch`: every URL is gated before the handoff. One denied URL refuses the whole batch; `details.denied_count` gives the count. No URL or host is returned.
- `POST /v1/profiles` (create, persistent scope only): asks the policy whether `browser.profile.persistent` is allowed; a disabled key is a 403 with `details.reason` `policy_denied`. The ZDR refusal of profile creation stays with `profile_guard` (its flag is unchanged). Save, update and import are not gated by this slice.
- Answer (`POST /v1/answer`) takes no URL and no paid-provider flag from the caller; its candidate fetches are WS7.G's to filter. Nothing is added here.
- ZDR consolidation (replacing `claims.effective_zdr*` with `resolved.zdr()`) is the plan's last optional PR and is not built; the legacy refusals are all still in place.

Flags. No new flag. The enforcement mode is `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT` (WS7.C): under `observe` the route runs as under `off` and the would-be denials are counted in `web_policy_would_deny_total`; under `off` only the invariants and the ZDR floor apply. The baseline is `QUARRY_EDGE__WEB_POLICY_BASELINE_JSON`.

Checks to run in the wave's test phase (not run here):
- `cargo test -p quarry-edge --test web_policy_routes` (decision-level cases for each route above).
- `cargo test -p quarry-edge --test zdr_claim_floor` (unchanged; the ZDR refusals are still there).
- Route-level cases still to write in the test phase: `paid_provider_disabled_by_org_is_downgraded_and_disclosed` through the router (response `policy_downgrades`, no Brave or Serper call in provider telemetry), `crawl_and_batch_deny_a_denied_seed_before_the_handoff` with the orchestrator double (zero calls), `profile_create_denied_when_policy_disables_persistence` through `POST /v1/profiles`.

Rollback. Set `QUARRY_EDGE__WEB_POLICY_ENFORCEMENT=off` (the baseline and the org layer stop applying; invariants and the ZDR floor stay), or revert the commit. Callers that never send a denied URL see no change.

Not done in this slice: the route-level tests above; the ZDR consolidation PR; WS7.G's answer candidate filter; save and update of profiles; resumed runs; the deployment evidence of the plan.

## WS17.E1: profile envelope encryption (W2-U36)

Status: built, flag off by default. Tests written and compile-checked; the test suites and the deployment evidence are NOT run yet (test phase of wave 2).

What it does: the profile snapshot (cookies, storage, IndexedDB) is sealed with AES-256-GCM before it reaches Postgres or Redis. Each profile has its own data-encryption key (DEK), wrapped by a master key. A new DEK is used every 256th stored save. The AAD binds the ciphertext to the org, the profile and the snapshot id. Plaintext above 4 KiB is gzipped first. Redis caches envelope bytes only.

Settings (edge environment):

| Setting | Values | Default | Effect |
|---|---|---|---|
| `QUARRY_EDGE__PROFILE_ENCRYPTION` | `off`, `compat`, `on` | `off` | `off`: plaintext, as before. `compat`: reads both, writes plaintext. `on`: reads both, writes envelopes only, lazy re-seal of plaintext rows. |
| `QUARRY_EDGE__PROFILE_ENCRYPTION_REQUIRED` | `true` / `false` | `false` | Boot refuses any mode below `on`. Set only in the production overlay after the backfill (WS17.E3). |
| `QUARRY_EDGE__PROFILE_KEY_PROVIDER` | `env` | `env` | `org_core` is refused at boot (WS17.E2 is not built). |
| `QUARRY_PROFILE_MASTER_KEYS` | JSON `{"v1":"<base64 of 32 bytes>"}` | unset | Required for `compat` and `on`. Minted once; listed in the secrets ledger, never in a committed file. |
| `QUARRY_PROFILE_MASTER_KEY_ACTIVE` | a key id, for example `v1` | unset | Required for `compat` and `on`. Must name a configured key. |

Boot refusals (the edge stops, the message names the setting and never the value): an unknown mode; `org_core`; `REQUIRED` with a mode below `on`; `compat` or `on` without master keys or without an active id; a master key that is not 32 bytes; an active id that is not configured. `on` with the env provider logs a WARN: there is no crypto-shredding until WS17.E2.

Encryption needs the Postgres profile store (`QUARRY_EDGE__PROFILE_STORE_KIND=postgres` and `QUARRY_EDGE__DATABASE_URL`). Without it the edge refuses to start with encryption on.

Rollout, one step per fully rolled-out release:
1. Ship the code with `off`. Migration 0016 adds `envelope`, `enc_v` and `key_id` (idempotent, no data change).
2. `compat` on every pod. Writes stay plaintext; the fleet can run mixed.
3. `on` after every pod runs the new code. Old pods cannot read rows whose `snapshot` is NULL, which is why `on` waits for a full new fleet. Plaintext rows are re-sealed lazily on load (compare-and-set on `rev`; a row that moved on is left alone).
4. `REQUIRED=true` after the E3 backfill reports zero plaintext rows.

Rollback from `on`: set `compat`. Envelope rows stay readable by the new pods; `compat` writes plaintext again. Rollback past `compat` to `off` is not supported while envelope rows exist.

Key loss: losing `QUARRY_PROFILE_MASTER_KEYS` (every key id that wrapped a DEK) makes every profile it wrapped unreadable. A load then fails with `RuntimeNotReady` (HTTP 503 class; the message reads `profile key unavailable`). The route layer does not yet set a `details.reason` field (depth). It never starts a profile fresh, and the row is not changed. Keep the master key in the secrets ledger and in a second place before `on`.

Checks (for the test phase, not run here):
- Unit: `cargo test -p quarry-runtime profile` (crypto, keys, blob store, envelope store, Redis cache with `REDIS_URL`, Postgres blob row with `DATABASE_URL`).
- Boot: `cargo test -p quarry-edge profile_encryption`.
- Evidence (on the rebuilt edge image with `on`): a persistent run that sets a canary cookie; then `select enc_v, key_id, snapshot is null, length(envelope) from quarry_profiles where profile_id='<id>'` returns `1|env:v1|t|<n>`, and `select encode(envelope,'escape') from quarry_profiles` contains no canary; Redis keys under `quarry:profile:v2:*` hold no canary; a second run restores the cookie; a throwaway edge with a wrong master key returns 503 (`RuntimeNotReady`) and leaves `rev` unchanged.

Open items (depth file `.plan-work/quarry-w1w2/depth/W2-U36.md`): the pinned known-answer ciphertext; external review of `profile_crypto.rs` and `envelope_profile_store.rs` before `REQUIRED` in production; restore latency evidence; the org_core provider (WS17.E2).

## WS13.3: driver capture API, JPEG screenshot kind, single-capture limit (W2-U40, built, NOT run, 2026-10-23)

What: `BrowserDriver::capture_screenshot`, `capture_pdf` and `current_url` (`crates/quarry-browser/src/lib.rs`). The defaults serve legacy options (the viewport PNG, the default PDF) and refuse every other option with `unsupported`. `ChromiumoxideDriver` overrides them; it sets the capability flags `scrape_actions` and `capture_options`. Planning is pure in `crates/quarry-browser/src/capture_plan.rs`. `ArtifactKind::ScreenshotJpeg` (`screenshot_jpeg`, `.jpg`, `image/jpeg`) is a new kind; `kind_from_str` accepts `screenshot_jpeg` and `screenshot.jpg`. `wait_for`, `click`, `type_text` and `select` now carry `details.reason` (`element_not_found`, `wait_timeout`, `driver_error`).

Flag: none. No caller reads these methods until WS13.4b. Rollback: revert.

Limit: `SINGLE_CAPTURE_MAX_DEVICE_PX` is 16000 and PROVISIONAL. The spike that sets it is not run. The spike table is in `docs/SCRAPE_CAPTURE.md`; do not raise the value or rely on a tall full-page capture until that table is filled in.

Checks (test phase, not run here):
- Planning and header reads: `cargo test -p quarry-browser plan_screenshot png_header`.
- Defaults delegate and refuse: `cargo test -p quarry-browser capture_default_tests`.
- Artifact kind: `cargo test -p quarry-core artifact` (`jpeg_screenshot_has_jpg_extension_and_image_jpeg_type`).
- Live (Chromium), one name per run: `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide -- --ignored --test-threads=1 <name>` with `<name>` one of `captures_full_page_clip_and_jpeg_on_a_fixture_page`, `pdf_starts_with_percent_pdf_and_honours_max_pages`, `viewport_override_is_restored_after_capture_even_on_error`, `capture_does_not_navigate_or_mutate_the_page`.

## Browser and crawl usage metering (WS6.C-e)

Switch: `QUARRY_EDGE__USAGE_BROWSER_METRICS` = `off` (default), `shadow`, `on`. Read once at boot by
`usage_attrs::browser_metrics_mode_from_env`; a value that is not one of the three is `off` and logged.

Order of rollout:

1. Confirm the Lago billable-metric codes for the seven metrics exist (none do in the repository).
2. Set `shadow`, restart the edge. Watch `quarry_usage_shadow_events_total` by metric: a segment close
   produces `browser.second.*` and `agent.run` for an agent, one `browser.second.interactive` for a session.
   Nothing is published in this mode.
3. Set `on`. The corrected rules for the existing metrics start with this step (see BILLING.md). Check the
   billing rows for one agent run and one manual session with `psql` on `billing_usage_events`.

Rollback: set `off` and restart the edge. The existing metrics go back to their legacy rules at the same
time; events already published are not withdrawn.

## WS15.S2: executable scrape, crawl and batch schedules (W2-U38, built, NOT run, 2026-10-23)

Scope: a scrape, crawl or batch schedule fires as a tracked control job with origin `schedule`. Built dark. Nothing fires until the flag is on and the S5 go-live gate (WS14 scope deployed and WS6.A live) is met.

- **Flag.** `QUARRY_SCHEDULED_JOBS_ENABLED` (control, default `0`, read at serialize time). Off: scrape, crawl and batch schedules stay unmapped and no Temporal schedule is created for them. A Temporal schedule of such a kind that already exists is paused on the next reconcile pass (about 30 s). It is not deleted. Turning the flag on maps the kinds on the next pass.
- **Deploy order.** Control, then the orchestrator, then the edge. The orchestrator and the edge do not read the flag.
- **Per fire.** The orchestrator's `ScheduledJobWF` (the schedule action) runs these steps in order:
  1. `POST /v1/internal/schedules/preflight` on the edge (runtime bearer token plus `X-Quarry-Run-Sig` over `org_id` and `schedule:<id>`). S2 answers `allow: true`, `posture: unchecked`, `entitlement: floor`. S4 adds the ZDR and entitlement checks here.
  2. `POST /v1/internal/schedules/{id}/runs` on control, HMAC signed, with `Idempotency-Key: sched:<id>:<run_key>`. Body: `run_key`, `run_id`, `status` (`running` or `skipped`), `reason`, `workflow_id`. Org, kind and params come from the schedule row. A denied fire is a terminal `skipped` job with `terminal_reason: schedule_skipped` and a `schedule_skipped` event.
  3. The matching job workflow (`ScrapeJobWF`, `CrawlJobWF` or `BatchJobWF`) runs as a child with workflow id `quarry-job-<jobID>` and `REQUEST_CANCEL` on parent close. The child's input comes from the shared builders in `workflows/inputs.go`, the same builders the dispatcher uses.
- **Overlap.** Stored policy `skip`, `allow` or `cancel` maps to Temporal `SKIP`, `ALLOW_ALL` or `CANCEL_OTHER`. With `cancel`, the wrapper signals its child `cancel` on a disconnected context and waits at most 2 minutes.
- **Failure.** A child error writes `run_failed`, or `run_timed_out` for a Temporal timeout. The payload carries `schedule_id`, `org_id`, `created_by` and a reason class, with no URL. The wrapper returns the error, so the schedule's pause-on-failure option can see it.
- **Crawl bounds.** The reconciler resolves a crawl schedule's `max_pages` and `max_depth` with the orchestrator's `QUARRY_CRAWL_*` limits, and writes them into the fire's params. The execution timeout (`QUARRY_JOB_TIMEOUT_*`) is written into the fire's args the same way. A change in either is a spec change and is applied on the next pass.
- **Reconcile.** The reconciler reads the desired set from `/v1/internal/schedules/desired`. It compares what Temporal holds (`Describe`) with the desired spec and calls `Update` only when they differ. It pauses or unpauses only when the state differs. It never deletes a schedule without an org memo, and never reaps across orgs.
- **Check a fire.** `GET /v1/schedules/{id}/runs` lists the fire's jobs. `GET /v1/crawl/{job}` shows `origin.type: schedule` and `origin.actor: <schedule id>`.

Not run in this wave. The deployment evidence (flag on, one minute cron, Oslo time zone, overlap skip, cancel through `DELETE /v1/crawl/<job>`, flag off pauses the schedule, one fire each for batch and scrape) is owed at the stack phase. The Temporal replay fixture `testdata/scheduled_job_history.json` is owed too.

Test phase checks (not run):

- `cd services/quarry-orchestrator && go test ./internal/workflows/ ./internal/schedules/ ./internal/jobs/ ./internal/activities/ -count=1`
- `cd services/quarry-control && go test ./internal/resources/ -run "Schedule|WorkflowFor|RegisterRun" -count=1`
- `cargo test -p quarry-edge --test schedule_preflight` and `cargo test -p quarry-edge --lib schedule_preflight`

## WS17.E3: profile backfill, erasure hop and plaintext removal (W2-U41, built, NOT run, 2026-10-23)

Built to scope; the tests are written and compile-checked, not run. Deployment evidence (a live stack, a restore after backfill, a canary check in Postgres and Redis) is not run. Plan: `docs/plans/quarry-browser-build-plan-2026-10-14.md`, "Slice WS17.E3 detail". Migration `0017_profile_plaintext_null.sql`.

What ships, and what is dark:

- `profiles_backfill` (quarry-edge binary, `--features postgres-queue`): seals every plaintext profile row under the current master key with a compare-and-set on `rev`, then unlinks the legacy plaintext cache keys (`quarry:profile:*`, never the v2 keys). Operator-run, resumable, idempotent, counts only.
- `DELETE /v1/internal/orgs/:org/profiles` on quarry-edge: the org erasure of the profile family (rows, tombstones, Redis keys `quarry:profile:v2:{org}:*` and `quarry:profile:{org}:*`, cached DEKs in that process). Outside `require_auth`; the shared token `QUARRY_EDGE_INTERNAL_TOKEN` gates it. Idempotent: a repeat answers zeros.
- quarry-control, the GDPR consumer: for an `organization` erasure event it purges its own store, then calls the edge route. Any failure naks, so JetStream redelivers. The hop is OFF while `QUARRY_EDGE_URL` is empty (compose default). A set URL with no token, or a non-http(s) URL, fails every erasure and logs why. Nothing is silently skipped.
- Migration 0017: `UPDATE ... SET snapshot = NULL WHERE envelope IS NOT NULL AND snapshot IS NOT NULL` (changes no row in a consistent store) and a `CHECK (snapshot IS NULL OR envelope IS NULL) NOT VALID` constraint. It runs on the next edge boot (sqlx). It does NOT prove that no plaintext row is left; that is the gate below.

Order to enable (after WS17.E1 `compat` and `on` are live):

1. Gate on the backfill, not on the migration. Before REQUIRED, count the rows that still hold plaintext (expect 0 after the backfill):
   `SELECT count(*) FROM quarry_profiles WHERE envelope IS NULL AND snapshot IS NOT NULL;`
2. Back up `quarry_profiles` and `quarry_profile_tombstones` (migration 0017 is irreversible).
3. Run the backfill once per environment, with the same master keys the edge uses:
   `QUARRY_EDGE__DATABASE_URL=... QUARRY_PROFILE_MASTER_KEYS=... QUARRY_PROFILE_MASTER_KEY_ACTIVE=... QUARRY_EDGE__REDIS_URL=... profiles_backfill --batch 200 --pause-ms 50`
   It prints one JSON line (scanned, sealed, moved_on, failed, legacy_cache_keys_unlinked). Exit 0 means failed = 0. A `moved_on` row was changed by a live save: re-run until step 1 shows 0. A failed row is not logged; re-run after fixing the cause (the stored JSON is not a snapshot, or the key failed).
4. Repeat step 1. Only then set `QUARRY_EDGE__PROFILE_ENCRYPTION_REQUIRED=1` in the release that follows.
5. Turn on the erasure hop: set `QUARRY_EDGE_URL=http://quarry-edge:8082` and the same `QUARRY_EDGE_INTERNAL_TOKEN` on quarry-control and on quarry-edge. Check with one call and read the counts, then repeat it and expect zeros:
   `curl -s -X DELETE -H "Authorization: Bearer $QUARRY_EDGE_INTERNAL_TOKEN" http://quarry-edge:8082/v1/internal/orgs/<org>/profiles`

Rollback and what is not reversible:

- The erasure hop: unset `QUARRY_EDGE_URL` on quarry-control. The control store still purges its own rows. The edge profile rows of that org stay until a manual call to the route. Record the org ids of any erasure that ran while the hop was off.
- The backfill has no rollback to plaintext. Once a row is sealed, `off` mode cannot read it (its `snapshot` is NULL). Read-back needs `compat` or `on` with the master keys. Losing the master keys loses those profiles: no plaintext copy exists after step 3.
- A pre-0016 binary cannot read envelope rows and fails the new CHECK on a write that sets both payloads. It is not a supported rollback target once `on` has run.
- Keep every master key that wrapped a DEK still in use. Cryptographic erasure of one org is NOT achieved by this slice (the env provider has no per-org key; that waits for WS17.E2).

Test phase checks (not run):

- `cargo test -p quarry-runtime --features postgres-queue profile_backfill org_erasure` (the Redis-gated cases need `REDIS_URL`; the Postgres-lane cases need `DATABASE_URL`).
- `go test ./internal/gdpr/...` in `services/quarry-control` (the new tests are `edge_purger_test.go` and the `HandleErasureWithEdge` cases in `erasure_test.go`).
- Stack evidence: backfill a canary profile, confirm no canary in `quarry_profiles` or in Redis, erase the org through the control event, confirm the rows, tombstones and `quarry:profile:v2:*` keys are gone and a second erasure reports zeros.

## Human browsing on the open web: WS8.B (U39, built, NOT run, 2026-10-23)

Flags. `QUARRY_EDGE__HUMAN_BROWSE` (off by default; 1 in the dev compose, 0 in the production overlay) accepts a broker grant of kind `human_browse`. `QUARRY_EDGE__HUMAN_RECEIPT_MODE`: `metadata` (default: the action redacted, no observation), `full` (today's storage, for dev and procedure recording) or `none` (no receipt for a person's step). A ZDR run writes no receipt in any mode. Both are read at boot; an unparseable value stops the edge and names the variable.

What it does. Open web minus the broker's denied list (`EgressMode::OpenWeb`, quarry-browser). Private, loopback, link-local, CGNAT, single-label and cloud-metadata hosts are refused whatever the denied list says. A human-browse run takes no domains from the request and no max-steps, runtime or cost bound; a person's steps do not charge the step budget (plan R-8). The broker's answer must be `policy_mode = open_web` with a canonical denied list; an allow-list grant of kind `human_browse` is refused. A checkpoint keeps `policy_mode` and, for open web, the URL without its query, fragment or userinfo.

Receipts. A person's step on a non-ZDR run under `metadata` has no observation; typed text, selected values, key presses, dialog prompts, upload ids and evaluated scripts are `[redacted]` (an upload id becomes the nil ULID); a navigation keeps scheme, host with port, and path. The timeline decode skips kinds it does not know (shipped one release before WS8.F writes new kinds).

Refusals (403, the message is the reason): `human_browse_disabled`, `human_browse_service_refused`, `human_browse_owner_mismatch`, `human_browse_zdr_posture` (a ZDR grant backs only a ZDR run), `human_browse_policy_unknown`.

Rollback. `QUARRY_EDGE__HUMAN_BROWSE=0`: new starts with a human grant are refused. The image rollback is safe (the tolerant decode and the policy column are additive).

Not done in this unit (see the depth file): the broker side (WS8.C: `human_browse` issuance, `RenewGrant`, the policy column); idle 900 s and absolute 4 h lifetimes (a human run uses the default TTL); the integration test `tests/human_browse.rs`; deployment evidence.

## Browser and crawl usage metering (WS6.C-e)

Switch: `QUARRY_EDGE__USAGE_BROWSER_METRICS` = `off` (default), `shadow`, `on`. Read once at boot by
`usage_attrs::browser_metrics_mode_from_env`; a value that is not one of the three is `off` and logged.

Order of rollout:

1. Confirm the Lago billable-metric codes for the seven metrics exist (none do in the repository).
2. Set `shadow`, restart the edge. Watch `quarry_usage_shadow_events_total` by metric: a segment close
   produces `browser.second.*` and `agent.run` for an agent, one `browser.second.interactive` for a session.
   Nothing is published in this mode.
3. Set `on`. The corrected rules for the existing metrics start with this step (see BILLING.md). Check the
   billing rows for one agent run and one manual session with `psql` on `billing_usage_events`.

Rollback: set `off` and restart the edge. The existing metrics go back to their legacy rules at the same
time; events already published are not withdrawn.

## Span export and ZDR traces: WS10.S8 (F-t2-004 phase 2, unit W2-U42, built, NOT run, 2026-10-23)

What it does. Adds the trace side of ZDR: a ZDR trace (a span with `zdr=true`, or a trace flagged after a span recorded it) exports nothing. The sampler, the span processor and the Go exporter wrapper all judge against one bounded trace registry (FIFO). Span attributes named in the `zdr_sensitive` or `never` classes are stripped from every span that is kept. Contract: `docs/TELEMETRY_FIELD_CONTRACT.md`; golden `crates/quarry-runtime/contracts/telemetry-field-contract.json`.

Flags. `QUARRY_TELEMETRY_EXPORT` (default on; `off`, `0` or `false` builds no exporter and no provider, and the process logs `telemetry export disabled by flag`). `QUARRY_TELEMETRY_ZDR_REGISTRY_CAP` (default 100000). Export also needs `OTEL_EXPORTER_OTLP_ENDPOINT`; without it nothing changes.

Go side. `pkg/quarryotel` `Init` honours the same flag. quarry-control calls `quarryotel.MarkZDR(ctx)` where a ZDR posture is parsed after the request span started (change webhook body, source register with `zdr`).

Limits. A span that ended before its trace was flagged is already in the batch and is not recalled (the same limit as logs). `QUARRY_TELEMETRY_EXPORT=off` is not a privacy control on its own: it stops export, it does not change what the logs say.

Rollback. Set `QUARRY_TELEMETRY_EXPORT=off` (no redeploy of the logic). Unset `OTEL_EXPORTER_OTLP_ENDPOINT` to stop export altogether.

Evidence owed (not run in this batch). `deploy/compose/docker-compose.otel-test.yml` (throwaway) with `deploy/compose/otel-test/collector.yaml`: a non-ZDR and a ZDR scrape plus step; the collector shows spans for the non-ZDR trace only; the ZDR container log has no probe URL; with the flag off the collector receives nothing. Do not set `OTEL_EXPORTER_OTLP_ENDPOINT` in any real environment before that run.

Not done in this unit (see the depth file): event-level stripping on OTel span events; the Model Plane `mp-telemetry` adoption (sibling b); the span-vocabulary attributes of WS6 L2; the exact OTel 0.29 builder signatures are compile-checked only.

## Organization limits resolver: WS6.D-s (unit W2-U43, built, NOT run, 2026-10-23)

What it does. `AppState.limits` answers, per organization, what is left of `quarry.crawl.page`,
`quarry.batch.url` and `quarry.browser.second.agent`, and the plan capacity values. The crawl and batch handoffs
and `start_run` read it. The default and only built source, `static`, reports nothing for anyone, so a
default deployment behaves as before; the acknowledgement of a batch now carries `limits.urls` (counts).

Flags. `QUARRY_EDGE__LIMITS_SOURCE` (`static` default; `billing_core` is refused at boot: not built).
`QUARRY_EDGE__LIMITS_FAIL_MODE` (`floor` default, `open`, `closed`; the production overlay pins `floor` and a
change needs ADR-1). The boot log line is `limits resolver` with `source` and `fail_mode`.

Reading it. A 402 `BUDGET_EXHAUSTED` with `details.dimension = "org_allowance"` is a spent configured allowance;
`details.metric` says which. The gateway shows `crawl_allowance_exhausted`. An agent start refused for the
allowance counts in `quarry_agent_budget_exhausted_total{dimension="org_allowance"}`; in `shadow` the same line
is counted with `mode="shadow"` and the run starts. With `closed` and an unreadable source the answer is 503
`RUNTIME_NOT_READY`, `details.reason = "limits_unreadable"`.

Rollback. Unset `QUARRY_EDGE__LIMITS_SOURCE` (static). Nothing is stored; no migration.

Not built here (see the depth file): the `billing_core` source (per-org quota read, 30 s cache, stale window), the
billing-core and Control Plane credential and plan keys, the per-organization capacity wiring (WS6.G, WS6.H), the
compose rows, and the stack evidence of the plan's deployment steps.

## Secret redaction of observations, events and errors: WS12.S2 (unit W2-U44, built, NOT run, 2026-10-24)

What it does. Every observation Quarry returns carries a `redaction` stamp, and what an observation, an event, a JSON
artifact or an error body shows has passed the rules in `docs/SECRET_REDACTION.md` (`qr-redact/3`). Nothing is stored
in a new place and there is no migration.

Flag. `QUARRY_EDGE__REDACTION_MODE`, set in compose from `QUARRY_REDACTION_MODE` (default `enforce`).
`shadow` counts and returns the input unchanged, `off` does nothing; production (`ENVIRONMENT=prod|production`) accepts
only `enforce` and the edge refuses to boot with the others or with a value it cannot read. The boot log line is
`secret redaction active` (or the warning `secret redaction is not enforcing`) with `rule_version` and `mode`.

Reading it. `redaction.rule_version` is the rule set that ran. `redaction.values_masked` and `redaction.by_rule` count
what was masked per rule id (`field.password`, `shape.jwt`, ...) and never hold content. `redaction.probe` is how the
field-facts probe ended (`ok`, `partial`, `failed`, `skipped`) and `probe_ms` what it cost. `partial` or `failed` means
some editable values were masked without facts (fail closed). `ProofBundle.redaction_rule_version` is the same rule
version on the stored proof. The edge logs one `quarry.redaction.observation` line (counts only) per observation that
masked anything.

Staging, 3 to 5 days in `shadow` (not yet done). Set `QUARRY_REDACTION_MODE=shadow`, exercise the planner on real
pages, read `by_rule` on the observations and the log lines for rules that fire on ordinary page text, then set
`enforce`. A shadow stamp has `mode: "shadow"` and the page content is unchanged.

Rollback. In production there is no setting that turns masking off: redeploy the previous image. On staging or in
development set `QUARRY_REDACTION_MODE=off`. `QUARRY_BROWSER_FIELD_PROBE=off` is more masking, not less (every editable
value is masked).

Evidence owed (not run). A rebuilt `quarry-edge` (record the image id and `SOURCE_REVISION`), then: Flow A (non-ZDR):
start a run, navigate to a page with a password field, `type_ref` into it, `get_content`: no `S3cretP@ss` anywhere in the
response, `redaction.values_masked >= 1`, the `ActionStarted` event over `GET /v1/agent/runs/{id}/events/stream` shows
`"text":"[redacted:password]"`, and `nats sub 'quarry.>'` shows no message with the secret. Flow B (`"zdr": true`): the
same masked body, `GET .../receipts` empty. Flow C: `type_ref` with a stale ref and the secret as text: the 409 body has
no secret.

Not built here: the receipt, checkpoint and procedure masking (WS12.S3), the Model Plane scrub (WS12.S4), the screenshot
masking (WS12.S5).

## Scrape captures: `screenshot` and `pdf` on `POST /v1/scrape` (WS13.4b, unit W2-U45, built, NOT run, 2026-10-24)

What it does. A scrape may ask for a screenshot (`{"type":"screenshot","fullPage":true,"quality":80}`) or a PDF
(`"pdf"` or `{"type":"pdf","format":"A4"}`) in `formats`. The browser takes them after the page has settled and
before the session is released. A failed capture is a warning (`screenshot_failed`, `pdf_failed`,
`screenshot_truncated`), never an error: the page still returns. Not ZDR: each capture is an artifact, read at
`formats.screenshot` or `formats.pdf` (`{artifact_id, bytes}`), fetched with `GET /v1/artifacts/{id}` (PNG, JPEG or
PDF by kind). ZDR: nothing is stored or emitted; the bytes come back inline in `data.screenshot` or `data.pdf` as
base64 (`content_type`, `bytes`, `width`, `height`, `data_base64`), under the inline cap. `data.captures` reports
the sizes, the tile count and `blocked_requests` (a count only) in both modes.

Flags. `QUARRY_EDGE__SCRAPE_CAPTURE` (`off` committed; only the exact value `on` enables it; anything else refuses
the formats as a 400 field error `feature_disabled` at `formats`). `QUARRY_EDGE__SCRAPE_CAPTURE_INLINE_MAX_BYTES`
(default `6291456`, the model gateway's screenshot cap; clamped to 16 MiB). Rollback: set the switch back to `off`,
or revert. No data migration; nothing is written that a rollback must undo.

Refusals. A browser that cannot take captures answers 501 `UNSUPPORTED` with `details.reason =
"browser_capability_unavailable"`, refused before any lease or session is taken. An over-cap ZDR capture is left
out with warning `capture_exceeds_inline_cap` and listed in `omitted`. `POST /v1/scrape/stream` refuses a
`screenshot` or `pdf` entry with an error event carrying `details.reason = "unsupported_in_stream"`; it does not
drop the format silently.

Cache. A request with a capture neither reads nor writes the page cache (a capture changes what the page holds).

Reading a capture that failed. `warnings[].message` names the typed reason (for example `capture_too_large`), never
the raw driver error. A page that blocked requests reports only the count in `data.captures.blocked_requests`.

Not built here. The `actions` format (WS13.5), stitching of tall pages (WS13.6; a page taller than the
single-capture limit is cropped with `truncated: true`, and the single-capture limit is provisional, see the W2-U40
depth file), the meter's capture counts (usage_attrs owns its closed key set), and the gateway SPA wiring (WS13.7).
Evidence owed: a rebuilt `quarry-edge` with the switch `on` against a real Chromium: the PNG and PDF magic bytes,
the IHDR size against `captures.screenshot.width/height`, the 501 path with a browser that lacks the capability,
and the ZDR inline bytes with no artifact list change.

## Browser evidence panels (WS9.D, W2-U46)

What ships. In the browser chrome (`BrowserChrome.tsx`, new code in `features/browser/evidence/*`): an Egress tab in the devtools panel (live allowed and blocked outgoing requests, redacted to method, origin and path), a filter bar over the Console tab (level chips, a substring match of up to 200 characters on the message text, debounced 250 ms, Escape clears; filtering runs in the browser), and a Procedure section in the evidence drawer ("Compile procedure", redacted steps, quality verdict, and the session's recent receipts). The panels read the gateway routes of WS9.C (`egress-receipts`, `receipts`, `procedure`).

Flag. The only switch is the capability `evidence` in the two static session capability lists of `apps/Frontend Plane/verevonv3/apps/gateway/src/domains/browser.rs`. The SPA shows none of the panels without it. Deploy order: `quarry-edge` (WS9.C filters already in tree), then the gateway, then the frontend. Rollback: remove the string from both lists and redeploy the gateway; the console falls back to the plain list.

ZDR. A zero-data-retention session sends only the live egress read. The SPA never requests receipts or a procedure for it, the Compile button is disabled, and the gateway refuses both with a typed 409 (`browser_receipts_unavailable_zdr`). Egress rows are held in memory only; nothing is written to browser storage.

Polling. Egress is read every 5 s while the panel is mounted and the document is visible. Hidden documents are not polled; the poll resumes on `visibilitychange`. Switching session aborts the in-flight read.

Evidence owed (not run in the build phase). Frontend restart and the WS9.C stack: the Egress tab lists an allow row and a block row after navigating to a private address; "Compile procedure" on a session with two actions shows `quality.promotionAllowed` and typed steps as `skjult tekst (n tegn)`; the console filter hides non-matching rows (DOM count of `.knowledge-browser-console-row` before and after `Escape` and a level chip). The Playwright spec `tests/e2e/browser-workspace-evidence.spec.ts` pins the three owner-gate and refusal codes.

Not built here: a server-side console filter (the console filter is client-side only), the observed live view (WS9.J), and the frontend copy of the procedure's `expectedOutcome` is shown as the gateway sends it (review before enabling for a session that types secrets; see the depth register).

## Web policy backstops: policy security engine, final-URL recheck, egress deny list (WS7.G 6a, unit W2-U34, built, NOT run, 2026-10-24)

The edge gates the URLs it is given. These backstops hold a run to the same host rules where a URL arrives that the edge never saw. Code: `crates/quarry-runtime/src/policy_gate.rs` (`PolicySecurityEngine`), the final-URL block in `crates/quarry-runtime/src/pipeline.rs` (`run_inner`), `BrowserEgressPolicy::with_denied_domains` in `crates/quarry-browser/src/lib.rs`, `agent_policy_gate::{egress_policy, live_egress_policy, egress_denied}` and `ObservationContext::egress_denied` for the agent path.

- **Engine.** `web_policy_edge::security_engine` wraps the shared `state.security` with the request's resolved host rules (`PolicySecurityEngine`), in the same `security` slot of `PageRunner`, so no `PageRunner` literal changed. `preflight` and `check_discovered` apply the host rules first (deny always; an allow list binds an autonomous principal only), then ask the wrapped engine. `allow_private_hosts` is the wrapped engine's answer. A block reason is `policy_denied: <code> (<layer>)` and never carries a host, path or query; each enforced denial is counted as `web_policy_denied_total{capability,decision_source}` through the engine's observer. Wired at the three page runners of `routes.rs` (scrape, internal `run_page`, scrape stream).
- **Flag.** `QUARRY_EDGE__WEB_POLICY_FINAL_URL_CHECK=1|0`. Unset: on when `ENVIRONMENT` is `dev`, `development`, `local` or `test`, off elsewhere. On: when a fetch ends on a URL other than the requested one, the pipeline preflights the final URL once (host rules, blocklist, URLhaus and PhishTank reputation, private-host heuristics). A block returns `SecurityBlocked` (403, text starting `policy_denied:` for a policy denial), emits `page_blocked` with `reason` `redirect_target_blocked`, and stores nothing (no artifact, no ingest, no cache). A final URL equal to the requested one is not preflighted again. A value other than 1, 0, true or false fails the boot. The trait method is `SecurityEngine::recheck_final_url` (default false), so an engine that is not the policy engine behaves as before.
- **Egress deny list.** `ResolvedPolicy::denied_hosts()` feeds `BrowserEgressPolicy::with_denied_domains`. A host equal to or under an entry is refused for every http(s) request the browser makes (navigation, redirect, frame, sub-resource) in every egress mode, also for a run with an empty allow list and for a denied subdomain of an allowed parent. The proxy and the Chromium Fetch guard both decide through `allows_url`, so both honour it; the refusal is an egress receipt with policy `domain_grant_blocked` (visible in `GET .../devtools-egress`). An empty list is byte-identical to the policy built before. A tab opened mid-run now keeps the run's decision (narrowing and deny list) instead of rebuilding the policy from the allow list alone.
- **Corpus first (production).** Before turning the flag on in production, replay a corpus of known-good redirects (http to https, bare to www, trailing-slash, locale paths) through `POST /v1/scrape` on a staging deployment with the flag on and confirm none returns 403. The recheck can only tighten.
- **Limit.** The final-URL recheck runs after the fetch: the redirect target has already been contacted by the driver when the run is refused. It prevents storing, ingesting and caching what was fetched; it does not prevent the connection. The browser deny list blocks the connection.

Checks to run in the wave's test phase (not run here):
- `cargo test -p quarry-runtime policy_gate final_url_recheck` (engine unit tests and the pipeline tests `redirect_to_a_denied_host_returns_security_blocked_and_persists_nothing`, `final_url_equal_to_requested_is_not_preflighted_twice`).
- `cargo test -p quarry-edge --test web_policy_backstop` and `cargo test -p quarry-edge --lib agent_policy_gate`, `cargo test -p quarry-edge --lib policy_baseline`.
- `cargo test -p quarry-browser --test egress_deny_list`; live lane: `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 cargo test -p quarry-browser --features chromiumoxide egress_deny_blocks_a_subresource_on_an_allowed_page -- --ignored`.
- `cargo test -p quarry-core --test web_policy denied_hosts`.
- Deployment evidence (owed): with a baseline that denies one host and the flag on, scrape a URL on a local host that redirects to the denied host: 403 `SECURITY_BLOCKED` text starting `policy_denied:` and `GET /v1/artifacts` shows no new artifact; in a browser run, a page on an allowed host that embeds an `<img>` from the denied host shows the blocked request in the devtools-egress receipts.

Rollback. Set `QUARRY_EDGE__WEB_POLICY_FINAL_URL_CHECK=0` (the recheck stops; the engine still applies the host rules the edge gate already applies), or revert the commits. The deny list is empty unless the policy has deny rules.

Not done in this slice: 6b (the durable-job `policy_snapshot`, `RunPageInput.PolicySnapshot`, `InternalRunPage.policy_snapshot`, the HMAC binding of the snapshot; waits for WS14 and WS15 to be deployed), a distinct receipt label for a policy deny (the receipts say `domain_grant_blocked` for both), and the final-URL check on the agent path's own navigation (WS7.D gates the landed URL already).

## Receipt masking: typed text, observations, checkpoints, procedures (WS12.S3, unit W2-U49, built, NOT run, 2026-10-24)

What was stored in a step receipt before this slice: the action as the caller sent it (a password typed into a field was in `quarry_step_receipts.action`), the post-action observation as returned, and the failure message as the driver wrote it. Every read of a receipt, and a procedure compiled from receipts, inherited that. Now one function owns what a receipt may say (`quarry_core::redaction::action::redacted_for_record`), the receipt builder applies it on every write path (REST step, WebSocket action, failure, skip), and the receipts route masks on read.

- **Action.** Typed text is masked by the kind of the control it went to, read from the snapshot the action named **before** the action ran (`typed_field_kind`, passed to the builder with `.typed_field(kind)`): a password becomes `[redacted:password]`, and likewise `otp`, `card`, `phone` and `secret`. A ref-addressed typing action whose control kind is unknown is masked as a secret (fail closed). A raw `type` action names its control by selector only, so `[type=password]` or a label word in the selector (`#password`, `input[name=otp]`) decides. A selected value and a dialog prompt pass the `Durable` text rules (token shapes are masked wherever they sit); a navigation URL loses secret query and fragment values and userinfo.
- **Observation.** Every string leaf of the stored observation passes the `Durable` rules, the value of a secret-named key is replaced, and every `url` or `*_url` field passes the `Durable` URL rules. The observation's own redaction stamp is kept as it was. If the masked observation cannot be rebuilt the receipt stores no observation rather than a partly masked one.
- **Failure and skip.** The failure message passes the `Durable` text rules (an error can quote what was typed); the action is masked from the pre-action control kind exactly as a completed step's is.
- **Stamp.** `redaction_rule_version` (migration `0013_step_receipt_redaction_version.sql`, nullable, no backfill; the field is also in the JSON, `openapi.yaml` `StepReceipt`) holds the rule version the receipt was written under. It is set only when masking was enforcing (`QUARRY_EDGE__REDACTION_MODE=enforce`), and for a person's metadata-only receipt (`QUARRY_EDGE__HUMAN_RECEIPT_MODE=metadata`, which is stricter than either text policy and wins for human steps).
- **Read.** `GET /v1/agent/runs/{id}/receipts` and `POST /v1/agent/runs/{id}/procedure` mask every receipt that is not stamped with the current version, in memory; stored rows are never rewritten (receipts are append-only). A receipt with no stamp may hold raw text and an unknown control kind, so its ref-addressed typing actions read as `[redacted:secret]`; one stamped with an older version only passes the text rules again. A receipt stamped with the current version is returned as it is, so the pass is idempotent.
- **Checkpoint.** `current_url` of the run checkpoint passes the `Durable` URL rules: a token in the query of the page URL never reaches `quarry_agent_run_checkpoints`.
- **Procedure.** A compiled step whose action carries a redaction marker is flagged `redacted_input: true`, and `compare_replay` answers `repair_required` ("typed input was redacted at capture; the replay must supply it") for it, never `exact`.

Flag. `QUARRY_EDGE__RECEIPT_TEXT_MODE=sensitive|all`, read once at boot with the redaction mode (`redaction_boot::init_from_env`); an unparsable value refuses to boot. `sensitive` (default) keeps text typed into an ordinary control (a search query) in an agent receipt, so audit and procedure compilation stay useful; `all` masks every typed text, selected value and dialog prompt, and a procedure compiled from such receipts cannot replay without its input. Both are accepted in production. The default is the plan's recommendation; the product owner's answer to X-19 is still owed before this is relied on as the default.

Deploy and rollback. The migration runs on boot (`run_migrations` embeds the whole directory; `ADD COLUMN IF NOT EXISTS`; old binaries ignore the column). Rolling back is redeploying the previous image: the column stays, nullable. Rows written by the new binary hold masked text; that is not undone.

Checks to run in the wave's test phase (not run here):
- `cargo test -p quarry-core redaction::action`, `cargo test -p quarry-runtime receipts procedure`, `cargo test -p quarry-runtime observation_redaction` (the event helper now delegates), `cargo test -p quarry-edge --lib redaction_boot`, `cargo test -p quarry-edge --features browser-agent --test redaction_wire`.
- Postgres (`DATABASE_URL`, `--features postgres-queue`): `append_persists_redaction_rule_version`, `legacy_null_version_decodes`, and the migration test (now 0010 to 0013).
- Deployment evidence (owed): a Postgres-backed edge with `zdr:false`, the secrets-lab flow, then `psql -c "SELECT action::text, observation::text, redaction_rule_version FROM quarry_step_receipts WHERE run_id='<run>'"`: the typed step's action text is `[redacted:password]`, `observation::text` holds no typed secret and no card-number fixture, the version column is `qr-redact/3`; insert a legacy-shaped row by hand (raw text, NULL version) and read `GET /v1/agent/runs/{id}/receipts`: it comes back masked; `POST .../procedure` shows `redacted_input: true` for the typed step.

Not done in this slice: SDK regeneration (the generated `StepReceipt` models lack the new field; `sdks/generate.sh` needs a container), the Model Plane side of X-19 (confirm the default with the product owner), per-key masking for a `press` that types a password one key at a time, and a typed-kind probe on the admission-refusal receipt (it records an unknown kind, so a refused typing action reads as `[redacted:secret]`).

## Tall full-page screenshots: tile-and-stitch (WS13.6, unit W2-U50, built, NOT run, 2026-10-24)

What it does. A `fullPage` screenshot of a page taller than one Chromium capture (`SINGLE_CAPTURE_MAX_DEVICE_PX`, still the provisional 16,000 device px) is stitched from clip tiles, but only when the stitch flag is on and the heavy lane has a free permit. Otherwise the image is cropped at the limit as before (WS13.3/WS13.4b). Method A only: each tile is a clip of the page with `captureBeyondViewport`, no scrolling and no overlap. Method B (scroll and trim) is not built; it waits for the spike in `docs/SCRAPE_CAPTURE.md`.

**Flag.** `QUARRY_EDGE__SCRAPE_STITCH`, read when the browser driver adapter is built (`quarry-runtime`, `browser_driver.rs`). `on`, `1` or `true` (any case, surrounding space ignored) turn it on; unset or any other value is off. The committed compose files do not set it, so the default is off. `QUARRY_SCRAPE_STITCH_METHOD` is not read by the code yet: there is one method.

**Off (the default).** The image stops at the single-capture limit. The response says so: `captures.screenshot.truncated` is `true` and the warning is `screenshot_truncated`. Nothing else changes.

**On, heavy lane free.** One permit from the heavy lane (`QUARRY_SCRAPE_HEAVY_PERMITS`, default 1) is held for that one capture. The response says `captures.screenshot.stitched: true` with `tiles` (the tile count) and `truncated` (the page was cut at a limit). The permit is taken only for a full-page capture whose page is taller than one capture; a viewport capture or a short page takes none.

**On, heavy lane busy.** No wait. The capture falls back to the crop, with `screenshot_truncated`. (An action waits for the lane; a capture does not, because a stitch is the slow path and the page is still usable without it.)

**Limits** (`quarry-core/src/capture.rs` and `quarry-browser/src/stitch.rs`). A tile is at most 4,096 CSS px tall and never more device px than one capture holds. At most 16 tiles. The stitched content is at most 60,000 CSS px. At most 40 Mpx for a PNG and 24 Mpx for a JPEG (the pixel cap is taken at the stitched width). A page past any limit is cut at the lowest of them and `truncated` is `true`. The deadline is the lower of 10 s times the tile count and 60 s; each tile also has the 10 s screenshot deadline. A stitch that runs out of time is `timeout` with `details.reason` `stitch_deadline` and no image.

**Formats.** PNG is the default. A capture with `quality` set (JPEG) is stitched in JPEG: the PNG tiles are read in full and the JPEG is encoded at the end over a white background (the JPEG has no alpha). The wire shape does not change: the response is the same `captures.screenshot` object.

**Memory.** PNG output is streamed: one decoded tile (RGBA) is held at a time, plus the compressed output. JPEG output holds the whole RGB image. A tile at scale 2 and 1,280 CSS px wide is about 84 MB of RGBA; the tile decoder allows 256 MiB for one tile.

**Errors (details.reason).** `no_content_area`, `capture_too_large` (the page is too wide to stitch at all), `screenshot_unreadable` (a tile is not a PNG Chromium should have sent), `stitch_tile_mismatch` (a tile is smaller than its rows), `stitch_encode_failed`, `stitch_deadline`. The tile captures keep their own reasons (`driver_error`, `viewport_emulation_failed`).

**Checks.** Not yet run. The tests are written and compile-checked: `cargo test -p quarry-browser stitch` (the pure planning and PNG streaming tests in `stitch.rs`) and, with Chromium, `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide stitch_live -- --test-threads=1 --ignored` (the banded 25,000 px page with the band order sampled, and the permit-busy crop). The plan's sticky-header and lazy-image live tests are not written (they need the spike).

**Rollback.** Leave `QUARRY_EDGE__SCRAPE_STITCH` unset (the default), or revert the commits. No migration, no schema or wire change.

**Owed before the flag goes on anywhere.** The spike (`docs/SCRAPE_CAPTURE.md`): the single-capture limit and the method choice on the pinned Chromium, a banded 25,000 px page, a sticky-header page and a lazy-image page. Then the edge evidence from the plan: a stitched 30,000 px banded fixture through `POST /v1/scrape` returns `stitched: true` with `tiles >= 2`, the PNG height is 30,000 times the device scale, the band colours at the middle of bands 1, 15 and 30 are in order, a second concurrent stitch gets the cropped image with `screenshot_truncated`, and a 90,000 px fixture is `truncated: true` at the 60,000 px cap.

## WS6.I series: agent runs, steps, budgets, caches, pool refusals

Slice WS6.I, second half (the HTTP, scrape and pool series are in "Metrics" above). Same listener, same flag, same token: nothing here is recorded unless `QUARRY_EDGE__METRICS_ENABLED=1`, and with it off every call below is a no-op. No new flag.

Series (labels are closed sets; a value outside its set is recorded as `other`, never as the caller's text):

| Series | Labels | Recorded by |
|---|---|---|
| `quarry_http_requests_total`, `quarry_http_request_duration_seconds` | `route` is the router's matched template (for example `/v1/agent/runs/:run_id/step`) or `unmatched`; `status_class` | the HTTP layer; a path that matches no route is `unmatched`, so a probe or an id in the path never becomes a label |
| `quarry_agent_runs_active`, `quarry_agent_runs_pending` | `class` = `interactive`, `agent` | the 5 s sampler, from `RunRegistry::stats()` |
| `quarry_agent_run_starts_total` | `class`, `outcome` = `started`, `resumed`, `denied`, `draining`, `aborted` | `start_run` (`aborted` = admitted, then failed before the run was registered; a start refused while draining has no class yet and counts under `other`) |
| `quarry_agent_run_denied_total` | `reason` = `global_run_cap`, `org_run_cap`, `org_interactive_run_cap`, `org_plan_cap`, `org_allowance`, `draining` | `start_run` refusals |
| `quarry_agent_run_expired_total` | `reason` = `idle`, `ttl`, `takeover_idle`, `max_age`, `shutdown` | the run sweeper; the drain at shutdown (`shutdown`) |
| `quarry_agent_takeover_exempt_steps_total`, `quarry_agent_takeover_exempt_step_duration_seconds` | none (a run id would be unbounded; the per-run view is the run's `human` receipts under their control epoch) | `step` and the WebSocket action path: a person's step during a takeover of an agent-class run, which the step budget does not count (TAKEOVER-BUDGET). The histogram `_sum` is the seconds those steps ran. Read it in the X-12 shadow review: it is the unmetered takeover use |
| `quarry_agent_steps_total`, `quarry_agent_step_duration_seconds` | `action_class` (eleven values: `navigate`, `click`, `type`, `press`, `scroll`, `select`, `wait`, `capture`, `evaluate`, `file`, `other`), `outcome` = `ok`, `error` | `step` and the WebSocket action path, around the driver call |
| `quarry_agent_run_duration_seconds` | `class` | `finish_run` (already built, WS6.K) |
| `quarry_agent_budget_exhausted_total` | `dimension`, `class`, `mode` | the budget gate (already built, WS6.B) |
| `quarry_lease_pool_refused_total` | `pool`, `reason` = `pool_full`, `org_render_cap`, `wait_timeout` | the pool counters, published by the sampler |
| `quarry_lease_pool_waits_total`, `quarry_lease_pool_wait_seconds_sum` | `pool` | the pool counters; the mean wait is the sum over the count (it is not a histogram: the runtime crate has no recorder) |
| `quarry_cache_requests_total` | `cache` = `page`, `search`; `outcome` = `hit`, `miss`, `error` | the two Redis caches (`error` = backend or decode failure, served as a miss) |
| `quarry_edge_draining` | none | 1 from the moment SIGTERM starts the drain; 0 otherwise |

Not built: `quarry_usage_events_total{metric,result}` (waits for the usage emission unit), the Control Plane Prometheus job `quarry-edge` and its token secret (Control Plane files; Prometheus refuses its whole configuration when a `credentials_file` is missing, so the job lands with the secret), alert rules.

Checks (written, run at the end of the wave): `cargo test -p quarry-edge --test metrics_recorder` (the recorder is process-global, so these live in their own binary) and `cargo test -p quarry-edge --lib metrics`; `cargo test -p quarry-runtime lease_pool`.

Deployment evidence (owed): with the flag on, from a container on `inter-plane-bus` read `/metrics` with the bearer; start and close an agent run and expect `quarry_agent_run_starts_total{class="agent",outcome="started"}` to move and `quarry_agent_runs_active` to rise and fall within 5 s; a step moves `quarry_agent_steps_total`; `curl ... | grep -c "<probe-host>"` is 0; `docker stop` shows `quarry_edge_draining 1` on a last scrape.

Rollback: `QUARRY_EDGE__METRICS_ENABLED=0` and restart, or revert the commit.

## Scrape-time actions: `actions[]` on `POST /v1/scrape` (WS13.5, unit W2-U53, built, NOT run, 2026-10-24)

**What it does.** A scrape may carry `actions[]` (`wait`, `click`, `write`, `press`, `scroll`, `select`, `screenshot`, `pdf`). The browser runs them in order on the page the scrape opened, after the `waitForSelector` wait and before the page is read. Each action reports an ordered outcome. Off by default.

**Flag.** `QUARRY_EDGE__SCRAPE_ACTIONS`: `off` (default; `actions` is refused with the field error `feature_disabled`), `user` (user tokens only; dev only), `on` (user tokens, and service principals that hold `scrape:interact`). Turn `on` only after the ADR-6 ruling, which is not in the approved set. Rollback: unset the variable or set `off`; the router deadline then returns to 60 s.

**Who may send actions.** A user token, or a service principal with the `scrape:interact` scope (`SCOPE_SCRAPE_INTERACT` in `quarry-edge/src/auth.rs`). A request labelled `"initiated_by": "agent"` is refused with 403 `scrape_interact_scope_required` whatever the token is. The label can only tighten; it never widens a principal.

**Limits.** The built-in caps in `quarry-core/src/scrape_actions.rs` apply and the environment can only lower them: 50 actions, 60 s of total wait, 30 s for one wait. Each action has a 30 s driver timeout inside the run deadline (115 s with actions on; the router allows 125 s for `POST /v1/scrape` only while the switch is on). No action is retried. `abort` (the default) stops at the first failure and marks later actions `skipped`; `continue` goes on. A request with actions holds one heavy-lane permit (`QUARRY_SCRAPE_HEAVY_PERMITS`); when the lane is busy the request gets 429 `heavy_browser_busy` at once and is never queued.

**Errors.** 400 field errors (for example `actions[3].selector`); 403 `scrape_interact_scope_required`; 429 `heavy_browser_busy`; 501 `browser_capability_unavailable`; 502 `action_failed` with `details.actions` holding every outcome in order; 408 or the handler's `actions_deadline`. Messages carry reason classes only: never the text typed by `write`, never a driver message.

**Not in this build (open).** The response does not yet carry `data.actions[]` (the `ScrapeResponse` view is not wired), action output bytes are held in memory only (the `pipeline.rs` persistence is not built), the safe-mode runner is replaced by `None` fields on the page runner (`local_index`, `page_renderer`, `title_enricher`; `source_registrar` was already `None`), and the gateway does not yet pass a 125 s timeout or stamp `initiated_by` for Model-proposed scrapes. The stream route refuses `actions` (400 `unsupported_in_stream`); the batch route is not checked. `scroll` by direction wheels at the viewport centre of the lease (640 by 400 when the lease names no viewport).

**Checks.** Not run. Written and compile-checked only: `cargo test -p quarry-runtime action_runtime` (`attached_runtime_never_acquires_or_releases_the_session`, `run_scrape_runs_actions_in_order_and_records_ordered_outcomes`, `abort_marks_later_actions_skipped`, `continue_records_failure_and_keeps_going`, `no_retry_on_failure`, `scroll_direction_maps_to_mouse_wheel_at_viewport_centre`, `wait_budget_and_deadline_stop_the_run_with_timeout`, `cancel_token_stops_between_actions`, `error_messages_never_contain_write_text`, `missing_driver_or_session_is_an_error_not_dry_run`) and `cargo test -p quarry-edge actions_tests`.

**Owed before the flag goes on anywhere.** The live Chromium test (`scrape_actions_live`: post-click DOM extraction and the off-host denial), the edge `scrape_capture` integration tests, `agent_labelled_user_token_cannot_send_actions` over the wire, the 502 and 429 body checks, and the evidence steps in the WS13.5 detail of the plan. Depth bullets are in `.plan-work/quarry-w1w2/depth/W2-U53.md`.

**Rollback.** Set `QUARRY_EDGE__SCRAPE_ACTIONS` to `off` (or unset it). The default request path is unchanged: the router deadline is 60 s and no render hint carries actions.

## Screenshot masking: sensitive fields painted over, or the image withheld (WS12.S5, unit W2-U52, built, NOT run, 2026-10-24)

Every screenshot the Chromium driver takes is scanned for sensitive form fields (passwords, one-time codes, card numbers, phone numbers, secret-labelled fields, by the same classifier as the accessibility masking) and their rectangles are painted over before the bytes leave the driver. Where it applies: the passive screenshot of every observation, the explicit `screenshot` action, the page renderer, and the scrape captures (`screenshot` format, single and stitched). Rules and limits: `SECRET_REDACTION.md`, section WS12.S5.

**Flags.** `QUARRY_BROWSER_SCREENSHOT_MASK=on|off` (default `on`; `off` is honoured only when `ENVIRONMENT` or `APP_ENV` is not `prod`/`production`, so a production process ignores it). `QUARRY_BROWSER_SCREENSHOT_OOPIF=allow|mask` (default `allow`). Both are read on every capture; no restart needed to change them on a dev stack.

**What an operator sees.**
- The observation's `redaction.screenshot` is `masked` (with `rects` and `mask_ms`), `none`, or `withheld`; `redaction.oopif_frames` counts cross-origin frames the scan could not look into.
- A withheld screenshot has no `screenshot_artifact_id`, no artifact row of kind `screenshot`, `screenshot_annotated`, `page_image_clean`, `thumbnail`, `tiles`, `ocr_preprocessed`, `logo_candidate`, `rendered_palette`, `visual_change` or `visual_observation` for that step, and one `info!` line `quarry.screenshot.withheld` with `note=screenshot_withheld:<cause>`.
- The explicit `screenshot` action and a scrape `screenshot` capture return `SECURITY_BLOCKED` (403) with `details.reason` `screenshot_withheld:<cause>`. A PDF (`pdf` action, scrape `pdf`) returns `SECURITY_BLOCKED` with `details.reason` `pdf_withheld:sensitive_field_value` (a rendered sensitive field holds a value) or `pdf_withheld:sensitive_field_unresolved`.
- Causes: `sensitive_field_unresolved` (a sensitive field is on screen and has no usable rectangle, the scan failed or timed out, or the page scrolled during two captures in a row), `image_too_large` (over 40 megapixels), `image_unsupported` (a JPEG with something to mask, or a PNG that is not 8-bit RGB or RGBA), `mask_failed`.

**If many screenshots are withheld.** Look at the step's page first: a login or checkout form with a zero-size or `opacity:0` honeypot field that classifies as sensitive withholds every screenshot of that page by design. A page that cannot run the scan script (a navigation in flight, a crashed renderer) withholds too; observe again. Do not set `QUARRY_BROWSER_SCREENSHOT_MASK=off` outside a dev stack: in production it is ignored.

**Checks (written, not run).** `cargo test -p quarry-browser screenshot_mask`, `cargo test -p quarry-browser sensitive_rects`, `cargo test -p quarry-core redaction::stamp`, `cargo test -p quarry-runtime observation withheld_screenshot`, and the live lane `CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide screenshot_mask_live -- --test-threads=1 --ignored --nocapture`. Evidence owed: on a rebuilt `quarry-edge`, the `/secrets-lab` flow (same-origin iframe and the scrolled variant `/secrets-lab/scrolled`), then `GET /v1/artifacts/{screenshot_artifact_id}` and decode the PNG: the password box is the mask colour `#1f2937`; a control run with `QUARRY_BROWSER_SCREENSHOT_MASK=off` on a dev edge shows the typed text; a page with an unplaceable sensitive field gives no `screenshot_artifact_id` and a `withheld` stamp; `mask_ms` p95 under 150 ms for a 1280 by 800 capture on the pinned image (measure and state the number).

**Rollback.** Revert the commit or deploy the previous image; on a dev stack `QUARRY_BROWSER_SCREENSHOT_MASK=off` turns it off. No migration. The stamp gains `mask_ms`, `oopif_frames` and `rects` (all default 0, so an older reader is unaffected).

**Not done.** The `png` dependency is the one WS13.6 added (version 0.18; the plan said 0.17). Cross-origin iframes are counted, not scanned (`allow`) or blanked whole (`mask`). The Browserless, Browserbase and Kernel drivers report `none`. The live view is not masked. The p95 measurement and the stack evidence are owed.

## Stopping the edge: what a drained run leaves behind (WS6.J usage segment)

This extends "Stopping the edge (drain)" above and replaces its "Not done" line for the `agent.expired` event and the usage segment. No flag: the drain was already on, `QUARRY_EDGE__DRAIN_DEADLINE_S` (default 30) is unchanged.

The order on SIGTERM is now:

1. `drain::begin()` as before: `/ready` 503 `draining`, new runs, crawls and batches get the typed 503.
2. **Runs end first, at once.** Every live run goes through `finish_run` with the reason `Drain` (8 at a time; 5 s each for the wait on a running action, for the usage step and for the release). Each run: writes a timeline item `expired` (initiated by `system`) when it is not ZDR, leaves a non-ZDR checkpoint `active` (so `resume_run_id` works after the restart), hands its **last usage segment** to the meter (`quarry.browser.second.agent` or `.interactive`, `quarry.agent.run`, `quarry.agent.step`, outcome `expired`, id `quarry:<run_id>:s<segment>:<metric>`, so a resumed run's next segment never collides), releases the browser session once, and emits `agent.expired` with `{"reason": "shutdown", "idle_s", "age_s"}`. A ZDR run writes no checkpoint, timeline item or receipt; its usage event carries the ZDR allowlist only. The usage events are still subject to `QUARRY_EDGE__USAGE_BROWSER_METRICS` (`off` drops them, `shadow` counts them, `on` publishes them).
3. **Then the open streams are closed** (`drain::close_streams()`): an event stream (SSE), a frame stream and a browser WebSocket send a final `done` frame with `shutdown` and end, so graceful shutdown completes with a viewer connected instead of waiting for the deadline. The event stream of a drained run has already received `agent.expired` and then ends with `done: run_closed`.
4. `serve` returns (or the deadline passes and the rest is closed), a second `drain_runs_for_shutdown` pass catches a run the first missed, the browser is shut down, the usage meter is flushed (at most 5 s, so the NATS publishes in flight leave the process), the local index commits, exit 0.

Log lines: `drain: runs ended released=N failed=N in_use=N` (the early pass), `drain: browser sessions released ...` (the second pass; normally all zero), `drain: an action still holds this run` (a run whose action did not finish in 5 s is left to the exit and is not billed for that segment), `drain: the usage segment was not accepted in time`, `drain: the timeline record failed; releasing anyway`.

Metrics: `quarry_agent_run_expired_total{reason="shutdown"}` counts the runs this drain ended.

**Checks (written, not run).** `cargo test -p quarry-edge --test drain` (serial; the file sets and resets the process-global flags): `drain_refuses_new_runs_with_503_and_retry_after`, `drain_closes_streams_so_graceful_shutdown_completes`, `drain_releases_every_session_exactly_once`, `drain_keeps_non_zdr_checkpoints_active_and_writes_none_for_zdr`, `drain_emits_the_usage_segment_before_exit`, `a_busy_run_does_not_block_the_drain`. Deployment evidence owed: with an SSE `events/stream`, a frame stream and one manual session open, `time docker stop quarry-edge` ends under the deadline with exit 0, the SSE received `agent.expired` with `reason: shutdown` and then `done`, and with `QUARRY_EDGE__USAGE_BROWSER_METRICS=on` `billing_usage_events` has the segment row.

**Rollback.** Revert the commit; the earlier drain (release without events or usage) is the previous behaviour. No migration, no schema or wire change beyond one more `done` reason (`shutdown`) on the live streams.

**Not done.** A boot-time settlement of orphaned non-ZDR `Active` checkpoints after a crash (plan S9b, design level; a crashed ZDR run is unbilled by design, ADR-4); `job_stream` and `scrape_stream` do not yet end on `close_streams` (the deadline still bounds them).

## Scrape captures and actions: flags, the e2e target and what is owed (WS13.8, unit W2-U56, built, NOT run, 2026-10-24)

Flags (each read once at process start; all off by default):

| Variable | Default | Effect |
|---|---|---|
| `QUARRY_EDGE__SCRAPE_CAPTURE` | `off` | only `on` admits the `screenshot` and `pdf` formats (WS13.4b) |
| `QUARRY_EDGE__SCRAPE_ACTIONS` | `off` | `user`: user tokens may send actions (dev only); `on`: also service principals with `scrape:interact` (needs the ADR-6 ruling); any other value is off (WS13.5) |
| `QUARRY_EDGE__SCRAPE_STITCH` | `off` | `on` tiles and stitches a tall full-page screenshot (WS13.6) |
| `QUARRY_EDGE__SCRAPE_CAPTURE_INLINE_MAX_BYTES` | 6 MiB (16 MiB ceiling) | the ZDR inline cap for one capture |
| `QUARRY_SCRAPE_HEAVY_PERMITS`, `QUARRY_SCRAPE_SESSION_ISOLATION` | see the sections above | the browser lanes that captures and actions use |

Rollback: unset the switches (or set them to `off`). The default response then has no capture or action field, and the router deadline on `POST /v1/scrape` is 60 s again.

The e2e target (opt-in; it is not part of `ci-test`):

    make test-scrape-capture     (run in apps/Ingestion Plane)

It runs `Quarry-v2/scripts/e2e-scrape-capture.sh` against a running stack. It needs `E2E_FIXTURE_URL` (reachable from the edge container), a token (`E2E_TOKEN`, or `E2E_COOKIE_JAR` with the gateway URL in `E2E_GATEWAY`), and the edge set to `QUARRY_EDGE__SCRAPE_CAPTURE=on`, `QUARRY_EDGE__SCRAPE_ACTIONS=user`; add `QUARRY_EDGE__SCRAPE_STITCH=on` and `E2E_EXPECT_STITCH=1` for the tall-page check. It exits non-zero on the first failed check.

Built and compile-checked only (W2-U56 ran none of it): the e2e script (`bash -n` only), the OpenAPI additions and the docs. Owed before any flag goes on anywhere: the named cargo tests and the live Chromium tests (`screenshot_mask_live`, `stitch_live`, `scrape_actions_live`); `make test-scrape-capture` on a clean stack, with the transcript; the ADR-6 ruling and the HTTP check of the user-token-with-agent-label refusal; the proxy read timeout in front of the edge (at least 125 s with actions on; plan X-20, unverified); and the gateway per-request timeout for scrape actions (not built; the WS13.5 depth note records the gateway default as 25 s).

## Complete profile capture: every cookie, every visited origin's storage and IndexedDB (WS17.C1, unit W2-U55, built, NOT run, 2026-10-24)

What it changes: today a release saves the cookies of the last active page only, with no expiry and no SameSite, and the web storage of that one page. With capture v2 on, a release of a persistent-profile run saves the whole context jar (`Storage.getCookies` for the session's own browser context, with expiry, session flag, SameSite, priority, source scheme and partition key), and for every origin the session visited its localStorage, sessionStorage and IndexedDB (databases, stores, key paths, indexes, records), under size caps. The snapshot gets `snapshot_version: 2` and `origins[]`; the legacy flat `local_storage` / `session_storage` fields keep the active origin's entries so an older binary still restores something. Restore is WS17.C2 (not built): until it lands, capture v2 stores more than a restore uses.

| Variable | Default | Effect |
|---|---|---|
| `QUARRY_BROWSER_PROFILE_CAPTURE_V2` | `0` | `1` turns capture v2 on (read at each release and at each tab attach). `0` keeps the old single-page capture exactly |
| `QUARRY_BROWSER_PROFILE_IDB` | `1` | `0` leaves IndexedDB out while v2 is on (web storage and cookies still captured) |
| `QUARRY_BROWSER_PROFILE_MAX_BYTES` | 16 MiB | plaintext cap of one snapshot; per origin 4 MiB, 64 origins, 50,000 IndexedDB records are fixed |

Rollback: set `QUARRY_BROWSER_PROFILE_CAPTURE_V2=0`. Snapshots written by v2 stay readable (v1 code ignores the additive fields), and v1 snapshots stay readable by v2.

How a capture runs (10 s budget; `Op::ProfileCapture` moved from 5 s to 10 s): cookies first (2 s; if this phase fails, nothing is saved and the previous snapshot stays), then web storage of every live tab (4 s), then IndexedDB of every live tab (4 s). A phase that runs out leaves the origin flagged `truncated` and the capture carries on, so a slow IndexedDB never loses the login. Origins whose tab is already closed come from the origin journal: a `pagehide` handler installed on each profile tab sends the origin's web storage (never IndexedDB) through `Runtime.addBinding`; the driver accepts it only when the sending execution context's origin (reported by Chromium) equals the origin the payload names.

Caps: over a cap the largest IndexedDB stores lose their records first (the store's definition stays, marked `omitted`), then the largest web storage entries; the origin is `truncated`. A cookie is never dropped. If the cookies alone exceed the cap the save is refused (`profile_snapshot_over_cap`) and the previous snapshot stays.

Log events (counts only, no URL, cookie name or value): `browser profile captured (v2)` with `cookies`, `origins_live`, `origins_flushed`, `idb_records`, `truncated`, `storage_timed_out`, `idb_timed_out` and the three phase times; `profile_capture_empty_skipped` (warn) when a capture found no cookie and no origin while the profile loaded at attach had some (the save is skipped, so a dead renderer cannot erase a profile); `profile_capture_over_cap` (warn).

What the redacted `GET /v1/profiles/{id}` and `restore_probe` show once a v2 snapshot exists: per-origin `origins[]` with IndexedDB database names and row counts and the web storage entry counts, `truncated`, cookie `same_site`, and `earliest_expiry` over live cookies. (These edge changes sit in `profile_routes.rs` with WS17.P0; check they landed together.)

Not done, owed in the test phase: the named cargo tests (`cargo test -p quarry-browser`: `tagged_serializer_roundtrip`, `merge_and_cap_drops_largest_idb_first_and_sets_truncated`, `cookies_are_never_dropped_by_caps`, `journal_caps_at_64_origins_and_ignores_non_http`, `cookie_mapping_expiry_session_samesite_partition` with the `chromiumoxide` feature, and the rest of the plan's list); the five live tests in `profile_capture_live.rs` (`cargo test -p quarry-browser --features chromiumoxide capture_ -- --test-threads=1 --ignored`, `CHROMIUMOXIDE_TEST=1`, public network); the S0 spike table (`profile_cdp_spike`), which decides whether a closed tab's IndexedDB can be read through the frame-less `IndexedDB` domain (not built); and the deployment evidence in the plan (a persistent run through httpbin and example.com, `restore_probe` reporting `cookies_count >= 1`, `origins_count >= 2`, `earliest_expiry` about 24 h ahead).

## Restore a saved profile before the first navigation (WS17.C2, unit W2-U58, built, NOT run, 2026-10-24)

A persistent session now restores its saved profile once per browser context, in the context's first tab, before that tab navigates. The first request to an origin already carries the saved cookies. Nothing reloads (the old path navigated, hydrated, then reloaded; it is deleted, so rollback is a revert). Later tabs of the same context skip the restore entirely and share the live cookie jar, so a second tab can no longer write stale cookies over newer ones. The restore runs for v1 and v2 snapshots alike and is not behind its own switch; it needs a profile store and a lease with `persist_profile`, and `QUARRY_BROWSER_PROFILE_CAPTURE_V2` only decides what a capture writes.

What it does, in order (`crates/quarry-browser/src/profile_restore.rs`, the call site is `open_tab_page`):
1. Cookies go into the context with `Storage.setCookies { browserContextId }`, 100 at a time, with no page URL (a blank first tab is fine). Expired cookies are skipped. A session cookie older than `QUARRY_BROWSER_PROFILE_SESSION_COOKIE_MAX_AGE_S` (default 604800, seven days; `0` restores no session cookie) is skipped; the age is the snapshot's `captured_at`, and a snapshot written before that field existed has unknown age and keeps its session cookies. A cookie whose captured domain has no leading dot is host-only and is restored with `url: https://<host>/` and no `domain`, so `__Host-` rules survive. Chromium rejecting a chunk costs one retry cookie by cookie; a refused cookie is counted, never fatal.
2. Web storage and IndexedDB come back through one script installed with `Page.addScriptToEvaluateOnNewDocument`. It holds a map keyed by origin and acts only in a frame whose `location.origin` is a key and whose localStorage lacks the sentinel `__quarry_profile_seeded_v1`. It sets the sentinel first, then localStorage and sessionStorage, then recreates the IndexedDB databases (stores, indexes, records through the tagged decoder) inside the `upgradeneeded` of its own `open`, so a page that opens the same database next queues behind it. Capture strips the sentinel. An origin is seeded on its first visit in the session, never again.
3. The profile's locale and timezone are emulated on every tab of the context. The user agent is never overridden (honest identity); `ua_changed` only records that it differs from the one the profile was captured with.

Origins the session never visits are carried into the next capture unchanged (the origin journal keeps what the loaded profile held for them, with their IndexedDB), so a run that only touches one site does not erase the others.

If the profile cannot be read (`ProfileLoadError`):
- `KeyUnavailable` (the key provider says `RuntimeNotReady`, "profile key unavailable"): the first tab fails with `503 RUNTIME_NOT_READY`, `details.reason = profile_key_unavailable`, and no browser state is kept. The caller retries. Starting fresh instead would let the release save an empty profile over the real one.
- `Corrupt` (any other load error): the session starts fresh and `lease_restore_failed` is set; its release does not save (log `profile_save_skipped_restore_failed`). The stored profile is untouched until an operator looks.
- `KeyDestroyed`: treated as no profile (the organization's key was shredded).

A restore that does not finish (the 15 s tab-attach budget, a failed seeding-script install) also sets `lease_restore_failed` and reports `failed: true`.

What the session reports (counts only, no name, origin, URL or value): `profile_restore` on the driver telemetry, so on every observation's `telemetry` and on the live browser-session projection (`profileRestore`, filled once the run has an observation): `cookies_total`, `cookies_applied`, `skipped_expired`, `skipped_session_stale`, `cookies_rejected`, `origins_seeded`, `idb_databases`, `ua_changed`, `snapshot_age_s`, `truncated`, `failed`. The same numbers are logged once per restore as `profile_restored`. The UI copy ("restored 14 cookies, 2 origins, 3 expired skipped") is WS9's.

Flags: `QUARRY_BROWSER_PROFILE_SESSION_COOKIE_MAX_AGE_S` (seconds, default 604800). No other switch.

Not done, owed in the test phase: `cargo test -p quarry-browser` (`host_only_cookie_restores_with_url_not_domain`, `expired_cookie_is_skipped_and_counted`, `stale_session_cookie_is_skipped`, `seeding_script_embeds_only_matching_origin_map`, `sentinel_prevents_reseeding`, `restore_report_never_contains_values`, `legacy_flat_entries_target_initial_origin`, `journal_carries_loaded_origins_and_keeps_their_indexeddb`), the live tests in `profile_restore_live.rs` (`CHROMIUMOXIDE_TEST=1 QUARRY_BROWSER_NO_SANDBOX=1 CHROME=/usr/bin/chromium cargo test -p quarry-browser --features chromiumoxide restore_ -- --test-threads=1 --ignored`; the two store-failure tests need no network), the deployment evidence in the plan (httpbin `quarry_e2e` canary across two runs, `profile_restore.cookies_applied >= 2`, one navigation request for the restore URL in `egress-receipts`), and the counts-only `profile_restore` timeline event (no timeline sink in the driver; see the depth notes).

## Plan-aware run caps (`org_plan_cap`, WS6.G)

An organization's plan can bound how many runs it has live, per class: the billing-core plan keys `quarry.cap.agent_runs` (agent runs) and `quarry.cap.interactive_runs` (a person's own sessions), read through `state.limits` (`crates/quarry-edge/src/limits.rs`, `OrgLimits::run_override`) at each `start_run`. The default source is `static` and reports no plan caps, so today the static caps above answer exactly as before. The only source that exists is the static floor; a `billing_core` source is not built (the edge refuses to boot with it).

How a start is admitted (`RunRegistry::try_reserve_with`): the global cap (`MAX_CONCURRENT_RUNS`) first and always; then, for the class being started, the plan bound if the plan sets one, else the static per-organization caps. A plan bound replaces `org_run_cap` and `org_interactive_run_cap` for its class (a plan can raise an organization above `MAX_CONCURRENT_RUNS_PER_ORG`), and is capped by `MAX_CONCURRENT_RUNS` (a plan never lifts the process-wide cap). Over the plan bound the start is `429 RATE_LIMITED`, `details.reason = org_plan_cap`, `scope = org`, `limit` = the plan value, `active` = the organization's live and pending runs of that class, with `Retry-After` (`RUN_CAP_RETRY_AFTER_S`). The gateway shows it as `browser_capacity_reached`, the same as the other caps. A refused start admits nothing and leaves no pending slot; `quarry_agent_run_denied_total{reason="org_plan_cap"}` counts it.

Things to know. A plan value of `0` refuses every start of the class; a plan that says nothing is absent (`None`), never `0`. Under fail mode `open` the organization is bounded only by the global cap; under `floor` (the default) an unreadable source means no plan bound; under `closed` an unreadable source refuses an agent run (`QUARRY_EDGE__LIMITS_FAIL_MODE`). A person's session never fails because the source is down: it falls back to the floor whatever the fail mode. Counters are per edge process (one edge container today).

The render-pool share is separate: `QUARRY_EDGE__POOL_ORG_MAX_PERMITS` (default `0`, off) gives each organization that many browser-pool permits and refuses over it as `org_render_cap` (section above); it is not plan-aware yet (a plan value for `quarry.cap.render_permits` is not read).

Rollback: nothing to switch off while the source is `static`. Once a plan source exists, `QUARRY_EDGE__LIMITS_SOURCE=static` removes every plan bound.

Not done, owed in the test phase: `cargo test -p quarry-edge plan_cap_tests`, `cargo test -p quarry-edge --lib resolver_tests`, `cargo test -p quarry-edge --test org_capacity`, the gateway `cargo test forward_quarry_failure`, and the deployment evidence in the plan (billing-core key `quarry.cap.agent_runs = 1` for one organization on a live stack, which needs the billing source).

## Browser control strip and the stale-epoch take over (WS9.I, unit W2-U62, built, NOT run, 2026-10-24)

What it is: a strip under the browser toolbar (`apps/Frontend Plane/verevonv3/src/features/browser/dock/BrowserControlStrip.tsx`) that says who holds the browser (the agent is working, the agent has control, or you are in control and the agent is paused because you took over) and offers Take over or Give back. The strip renders from the projection only. Its button sends the control epoch it showed (`expectedEpoch`), so Quarry applies the change only if the epoch is still the one the person saw.

Wire contract (gateway, Verevon side):
- `POST /api/v1/browser/sessions/{id}/control` takes `{ mode, expectedEpoch? }` (camelCase on the SPA side; the gateway forwards `expected_epoch` to Quarry). Without `expectedEpoch` the change is unconditional, as before.
- The owner projection carries the epoch as `session.control.epoch`. A Take over moves it to 1, a Give back to 2 (the epoch moves only when the mode really changes).
- A stale epoch: Quarry answers `409 CONFLICT` with `details.control_epoch`; the gateway answers `409 browser_control_epoch_stale` with the same details and changes nothing.
- `browser_action.set_control_mode` (action registry) takes the same optional `expectedEpoch`; a non-integer is a 400.

SPA behaviour: on `browser_control_epoch_stale` the SPA refetches `GET /api/v1/browser/sessions/{id}` exactly once and shows the copy "Browser control changed elsewhere. It has been refreshed; try again." It never retries the change by itself; the person decides again. `USER_TOOK_OVER` stays the takeover error copy.

Tab provenance: a live tab opened by the agent (`tabs[].openedBy === 'agent'`, WS8.F) gets an agent marker, and a non-active agent tab gets a Reveal button ("Vis"). Quarry does not report `openedBy` until WS8.F is built, so no marker shows today.

Flags: none. The strip is shown whenever a live session is open. The Crawl composer's control chip is unchanged.

Not done, owed in the test phase (written, not run): `src/features/browser/dock/browser-control.test.ts`, `src/features/browser/dock/BrowserControlStrip.test.tsx`, `src/features/dashboard/home/browser-session-control.test.ts`, the gateway tests `control_body_forwards_expected_epoch` and `stale_epoch_maps_to_typed_conflict` (in `apps/gateway/src/domains/browser.rs`), and the Playwright test `take over and give back move the control epoch, and a stale epoch is refused` in `tests/e2e/browser-workspace-live-takeover.spec.ts`. The Playwright test needs the stack with WS2.C deployed (the stack evidence for WS9.I: `controlEpoch` 1 after Take over, a stale `expectedEpoch: 0` returns the typed 409, Give back gives 2).

## Crawl scope on a crawl: the switch, the default scope and the denial summary (WS14.S3, unit W2-U60, built, NOT run, 2026-10-24)

What it is: a crawl started through `POST /v1/crawl` carries a scope (same host by default; `allow_subdomains` and `allow_external_links` widen it) to the orchestrator, which sends a `crawl` context with every page to `POST /v1/internal/run_page` and enqueues the children the edge returns. A refusal is counted and summarised, never silent.

- Switch: `QUARRY_EDGE__CRAWL_SCOPE_MODE=off|observe|enforce`, default `observe`. Read once at boot; an unknown value stops the edge at boot (`crawl scope:` error, the value is not echoed). `observe` counts and drops nothing, so existing crawls keep their links; `enforce` drops the refused links before they are enqueued; `off` stamps `scope.legacy=true` and the orchestrator sends no context (the rollback position).
- Handoff: `HandoffAck.effective` on `POST /v1/crawl` echoes `scope`, `scope_mode`, `robots` (always `respect` here), `max_pages`, `max_depth`, `legacy` and `warnings` (`subdomains_followed`, `external_links_followed`). A job accepted before this change has no `scope` and runs as a legacy crawl.
- Version skew: the orchestrator marks the marker `crawl-scope-v1`. An edge that does not apply the scope (no `crawl.scope_applied` in the page report) ends the run with `run_failed`, code `edge_scope_unsupported`, and no link of that page is followed. The fix is to deploy the edge that matches the orchestrator, or set the switch to `off` and redeploy the previous orchestrator.
- Events: `run_started` is unchanged. Terminal `run_completed` gains `pages_blocked`, `links_denied`, `hosts_added` and `denials` (`mode`, `counts` by code, `samples` by code, at most 10 per code and 12 codes). A page the edge refuses emits `page_blocked` with `category` `blocklist`, `code` (the denial code, for example `robots_disallowed` once WS14.S4 lands), `message`, `remedy` and `source` `fetch`. It counts as blocked, not failed.
- Durable frontier: the children of a page under scope go in one `enqueue_batch` call (at most 500 items, `POST /v1/internal/queues/{queue}/enqueue_batch`). A re-sent item keeps its request id and is not queued twice.
- Operator checks: `run_completed.payload.denials.counts` shows what was refused and why. A crawl with `links_denied` above zero under `observe` had links that `enforce` would drop. The Knowledge composer summary row (nb and en copy) is not built; the summary is in the event payload and `knowledge-client.ts` reads it.
- Not in this section (owed): the replay histories (`testdata/crawl_pre_scope_history.json`), the `scripts/e2e/crawl-scope-robots.sh --expect observe|enforce` evidence and the gateway copy of the summary. See the W2-U60 hand-over.

## Browser broker: human_browse grants, renewal and attach grants (WS8.C, broker side, unit W2-U61, built, NOT run, 2026-10-24)

What it is: three additions to the BrowserBroker (`apps/Model Plane/go/services/browser-broker`) and the model-gateway door to it. Nothing here is deployed; every switch ships dark in production.

- `human_browse` kind: `AcquireGrant` with `grant_kind = "human_browse"` (proto field 50) issues an open-web grant for the signed-in user: `policy_mode = "open_web"`, no `allowed_domains`, a `denied_domains` deny-list (default `.internal .local .localhost .localdomain .lan .home.arpa .svc metadata.google.internal metadata.goog`), `expires_at = now + BROWSER_BROKER_GRANT_TTL_SECONDS`, `absolute_expires_at = now + BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS` (default 14400). A service principal, a request that names hosts, sensitive scopes, a parent or a budget claim, and any request while the switch is off are all refused (`PermissionDenied` / `InvalidArgument`). A ZDR user gets a `z_` grant held in the broker's memory only (same rule as session grants, X-2).
- `RenewGrant` (`POST /v1/browser/grants/{id}/renew` on the model-gateway): user bearer only; extends a `session` or `human_browse` grant to `min(now + TTL, absolute_expires_at)`, never shortens it. At the cap, expired, revoked, or another kind (`run`, `attach`, `sensitive_approval`): 409 `browser_grant_not_renewable` (`FailedPrecondition` at the broker). Another user's or another tenant's grant: the same 409 at the gateway (`NotFound` at the broker). An older broker answers `Unimplemented`, which the gateway maps to 502. A plain `session` grant has `absolute_expires_at == expires_at`, so renewing one is always at the cap: only `human_browse` is extendable in practice.
- `attach` derive: `DeriveRunGrant` with `purpose = "attach"` and `resource_id = <run id>` (field 50) issues a child of a `session` or `human_browse` grant bound to one run: user principal only, hosts explicit (1 to `BROWSER_BROKER_MAX_ATTACH_DOMAINS`, default 8; from a `session` parent they must be a subset of its hosts, from `human_browse` none may be under the deny-list), never inherited from open web, expiry `min(now + min(ttl, 15 min), parent.expires_at)`, not renewable, a ZDR parent is refused (`FailedPrecondition`), idempotent on `(parent, session_key)`, revoked with the parent. A `run` (ai_run) derived from a `human_browse` parent must also name its hosts (the model-gateway answers 400 `allowed_domains required: a human_browse session never lends open web` before calling the broker).

Flags (broker env; compose in `apps/Model Plane/deploy/docker-compose.yml`, production overlay in `docker-compose.production.yml`):
- `BROWSER_BROKER_HUMAN_BROWSE_ENABLED` 1/0/true/false; the Go default and the production overlay are 0, the dev compose is 1. Anything else stops the broker at boot.
- `BROWSER_BROKER_HUMAN_ABSOLUTE_TTL_SECONDS` (default 14400, clamped 30 s to 24 h), `BROWSER_BROKER_DENY_SUFFIXES` (comma-separated DNS names; a leading dot is a pure suffix; unset uses the default list; an invalid entry stops the broker), `BROWSER_BROKER_MAX_ATTACH_DOMAINS` (default 8).
- Boot log line `browser-broker grant limits` shows `human_browse_enabled`, `human_absolute_ttl_seconds`, `max_attach_domains`, `deny_suffix_count`; never a host.

Migration: `0004_browser_grants_policy.{up,down}.sql` (R-3: the design said 0002). Adds `policy_mode`, `denied_domains`, `resource_id`, widens the kind CHECK, allows an empty host list for `human_browse` only, and adds a partial unique index for attach idempotency. Re-applied on every start; every statement is idempotent. The down migration deletes `human_browse` and `attach` rows first. Still no `zdr` column.

Deploy order: broker (migration runs at start), model-gateway, then Quarry (WS8.B) and the gateway (WS8.D). Rollback: `BROWSER_BROKER_HUMAN_BROWSE_ENABLED=0` stops new `human_browse` grants at once (live ones run out or renew until the cap; revoke them with `DELETE /v1/browser/grants/{id}`); image rollback is safe because the new columns are nullable or defaulted.

Operator checks: counters `browser_broker_grants_issued_total{outcome}` (new outcomes `human_browse_disabled`, `attach_zdr_refused`, `attach_needs_owner`, `attach_resource_invalid`), `browser_broker_grants_renewed_total{outcome}` (`renewed`, `absolute_cap_reached`, `renew_not_allowed`, ...). A rising `absolute_cap_reached` is a session that hit four hours, not a fault.

Not done, owed in the test phase (written, not run): broker `go test ./...` and `GOWORK=off go test -tags integration ./internal/grant/...` (needs Docker or `TEST_DATABASE_URL`), `cargo test -p model-gateway --lib browser_grants browser_run`, `node --test scripts/tests/browser-grant-contract.test.mjs`, `bash scripts/tests/zdr-contract-sync.test.sh`. Not built here: Quarry reading the new `ValidateGrantResponse` fields (WS8.B vendors 50-53 into `quarry-runtime/proto/browser.proto`), the gateway keep-alive (WS8.D), the execution-core attach consumer (WS8.G), the sibling-(b) confirmation of the `browser_run.rs` hunk (R-27), the L2 evidence run and X-1.

## Batch URL check at handoff (WS14.S5, unit W2-U64, built, NOT run, 2026-10-24)

- What it does: `POST /v1/batch` checks each URL before a job exists, with no network work. A URL is refused as `not_crawlable` when it is empty or does not parse, or its scheme is not http or https. It is refused as `security_rejected` when it carries userinfo or points at a loopback, private or local host (`quarry_security::heur::check`). A repeat is `duplicate` once its fragment is ignored. A URL past the cap is `max_pages_reached`.
- Responses: when every URL is refused the answer is 400 `BAD_REQUEST` with `details.rejected[] {url, code, message}`, and no job is created. When at least one URL is accepted the answer is 200. The job takes only the accepted URLs, and `data.rejected` lists the refused ones (the field is absent when none were refused). The allowance (402) and queue (429) paths count the accepted URLs only.
- Order: the static check runs before the web-policy gate, so the gate sees accepted URLs only. A URL the gate denies still refuses the batch as before.
- Flag: `QUARRY_EDGE__CRAWL_BATCH_MAX_URLS`, default 500, valid from 1 to 10000. An unparsable value fails boot and the value is not echoed. The gateway keeps its own cap of 200. Validation has no off switch; rollback is a revert of the change.
- Robots: the batch job carries `params.robots = "respect"`. The orchestrator sends a `crawl` context of kind `batch` with each page behind the `batch-check-v1` version marker, and counts a page the edge refused with a blocklist denial in `pages_blocked`. Such a page also stays in `pages_failed`. Robots enforcement itself is WS14.S4 and is not built, so `QUARRY_EDGE__CRAWL_ROBOTS` has no effect on batches yet.
- Logs and events: no URL is logged. The rejected list is returned to the caller only, and it holds the caller's own URLs.
- Not built or not run: the WS14.S4 robots gate, the deployment evidence against the fixture stack (a `POST /v1/batch` with a `file://` URL, a duplicate and a private literal, checking the events and the fixture log), and the gateway cap.
- Test phase (not yet run): `cargo test -p quarry-edge batch_check`; `cargo test -p quarry-edge --test batch_handoff_check`; `cd services/quarry-orchestrator && go test ./internal/workflows -run 'TestBatch' -count=1`.

## robots.txt gate on crawl and batch pages, `Crawl-delay` and `/v1/map` (WS14.S4, unit W2-U63, observe build, NOT run, 2026-10-24)

What it is: a crawl or batch page (`POST /v1/internal/run_page` with a `crawl` context whose `robots` is `respect`) is checked against its origin's robots.txt inside `PageRunner::run_inner`, after the security preflight and before DNS resolution and the host scheduler slot. A refusal therefore never reaches `record_bad` (which would halve the host's concurrency). Discovery filters links against the cached rules only (`RobotsGate::peek`, never a network call). `/v1/map` shares the gate. A single `/v1/scrape` and a crawl that ignores robots are not checked.

- Switch: `QUARRY_EDGE__CRAWL_ROBOTS=off|observe|enforce`, default `observe`, read once at boot (an unparsable value, or one of the settings below, stops the edge: `robots gate:` error). `observe` fetches robots.txt, evaluates and counts `would_deny`; it blocks nothing, slows nothing and drops no link. `enforce` refuses a disallowed or unreachable page, honours `Crawl-delay` and filters discovery. `off` builds no gate (the rollback position). **Do not set `enforce` in production before the product owner signs off X-10.** `ignore` is a per-crawl value of `crawl.robots` and has no user-facing form.
- Unreachable robots.txt: `QUARRY_EDGE__ROBOTS_UNREACHABLE=disallow|allow`, default `disallow` (RFC 9309: the origin counts as fully disallowed). A 429, a 5xx, a timeout, an unresolved redirect, a refused target (private, loopback, metadata address) and a transport error are unreachable, after two attempts inside the fetch timeout. 401, 403, 404, 410 and other 4xx mean no robots.txt (everything allowed). With `disallow` and `enforce`, a 503 on robots.txt stops every page of that origin until the 5 minute entry expires.
- Other settings: `QUARRY_CRAWL_DELAY_MAX_S` (default 10, ceiling 60; a longer `Crawl-delay` is clamped), `QUARRY_ROBOTS_CACHE_MAX_ENTRIES` (default 10000, oldest entry dropped first), `QUARRY_ROBOTS_FETCH_TIMEOUT_S` (default 8, 1 to 60).
- Matching: the product tokens are `quarrybot` and `quarry` (one constant, `ROBOTS_PRODUCT_TOKENS`); the most restrictive answer wins. The rule is matched against path and query (`Disallow: /*?session=` works; `?` is a literal, only `*` and a trailing `$` are special). A `*` group applies to a token with no group of its own. Longest rule wins, `Allow` wins a tie.
- Cache: key `scheme://host:port`; rules 24 h, absent 1 h, unreachable 5 min. A request under ZDR fetches robots.txt and stores nothing (`get_uncached`); the cache holds only public robots.txt text and is shared across organisations.
- Fetch path: robots.txt is fetched through the guarded default driver with `fetch_conditional` (org identity, third-party processing denied, 512 KiB read cap that cuts a longer body instead of refusing it). There is no second HTTP client; `ReqwestRobotsFetcher` is deleted.
- Refusal contract: HTTP 403, `error.code SECURITY_BLOCKED`, `retryable false`, `error.details.denial = {code: robots_disallowed|robots_unreachable, message, remedy, user_agent?, path?, host?}`. The runtime emits one `page_blocked` (`reason`, `code`, `path`, `source: "fetch"`, `stage: "request"|"final_url"`) and no `page_failed`. A page served from the page cache gets the same decision at `stage: "request"` before it is served or metered; a refusal there is the same 403 and the same event (D2-161). After a redirect to another origin the final URL is checked against that origin's robots.txt before anything is parsed, stored or ingested.
- Crawl-delay: `page_fetched.payload.crawl_delay_ms` (orchestrator, under the `crawl-scope-v1` path) and `crawl.crawl_delay_ms` in the page report carry the cached delay in every mode but `off`. Only `enforce` feeds it to `HostScheduler::set_min_interval`, which spaces request starts on that host (zero interval is the unchanged fast path). A 10 s delay caps a crawl near 180 pages per 30 minutes; the limit is WS15's decision.
- `/v1/map`: with `enforce` the rules come from the gate (query-aware, token set, uncached under a ZDR token); an unreachable robots.txt under `disallow` is a 403. `off` and `observe` keep the earlier best-effort filter unchanged.
- Metrics (content-free): `quarry_robots_decisions_total{mode,outcome}` (`allowed` with `mode="any"`, `denied` and `unreachable` under enforce, `would_deny` and `would_unreachable` under observe), `quarry_robots_cache_entries`. No host, path or URL in a label or a log line.
- Soak readout (X-10, owner decision 2026-10-28): the go criterion is the share of decisions the robots file would have refused: `sum(rate(quarry_robots_decisions_total{outcome=~"would_.*"}[1h])) / sum(rate(quarry_robots_decisions_total[1h]))`, read across replicas (Prometheus sums them). Every robots decision counts once: a fetched page, a page served from the page cache (decided before it is served, D2-161), and a redirect to another origin (a second decision for the same page). The volume stop rule (enough crawls across enough distinct sites) is read from the crawl jobs; the counters do not carry a host, so distinct sites are not counted here.
- Built 2026-10-28 (unit D2-B-x10-robots, list B, flags off): the decision on a page served from the page cache (D2-161, `PageRunner::precheck_robots`), the composer row "N pages blocked by robots.txt" (`pages_blocked_robots` on the terminal event of a crawl or a batch), the `/v1/map` check and the unreachable and redirect legs of `scripts/e2e/crawl-scope-robots.sh`, and the env wiring `QUARRY_EDGE__CRAWL_ROBOTS` (default `observe`) and `QUARRY_EDGE__ROBOTS_UNREACHABLE` (default `disallow`) in `docker-compose.yml`.
- Not run on a stack: the e2e legs (`scripts/e2e/crawl-scope-robots.sh --expect observe|enforce|unreachable|redirect`; syntax-checked only). The unreachable and redirect legs also need fixture hosts the overlay does not serve yet: `UNREACHABLE_SEED_URL` with `FIXTURE_UNREACHABLE_CONTAINER`, and `REDIRECT_SEED_URL` with `FIXTURE_REDIRECT_CONTAINER`; without them they report NOT-RUN. The `enforce` flip itself (X-10) is the owner's, after the volume-based observe soak: set `QUARRY_EDGE_CRAWL_ROBOTS=enforce` in the stack's env file and restart quarry-edge; `off` is the rollback.
- Test phase (not yet run): `cargo test -p quarry-transform robots`; `cargo test -p quarry-runtime robots_cache robots_gate host_scheduler pipeline::robots_gate_tests`; `cargo test -p quarry-runtime --test robots_gate_ssrf`; `cargo test -p quarry-edge --test crawl_scope_robots`.

## Crawl request contract (WS14.S6, unit W2-U65, built dark, NOT run)

- Flag: `QUARRY_EDGE__CRAWL_REQUEST_VALIDATION=lenient` (default) or `strict`. Unknown keys are warnings in lenient and 400 in strict. Reserved keys are 400 in both modes.
- A bad request returns 400 `BAD_REQUEST` with `details.fields[] {path, code, message}`. Look up the codes in `docs/CRAWL_SCOPE_ROBOTS.md` section 12.
- `ignore_robots_txt: true` returns 403 with `details.reason = policy_denied` for every request until WS14.S7 is built.
- Rollback: set the flag back to lenient and restart the edge. The flag does not switch off the unsupported_option or policy_denied refusals; those stay on until their options are built, so a caller that sent them gets a 400 or 403 rather than a silent drop.
- Operator check: `request_warnings` in the crawl ack names the ignored keys (names only, never values).
- Test phase (not yet run): `cargo test -p quarry-edge crawl_request`; `cargo test --manifest-path apps/gateway/Cargo.toml normalize_crawl`; `pnpm vitest --run` on the action-registry guard test once it exists.

## Human browser sessions: grant, keep-alive and the dark switch (WS8.D)

Status: built, dark. Nothing is deployed and the switch is off in every environment until the dev and evidence stacks set it.

- **Switch.** Gateway `BROWSER_HUMAN_BROWSE` (`1`, `true`, `yes` or `on` turns it on; anything else, or unset, is off). It is read without `APP_ENV`. It only takes effect when `BROWSER_GRANT_ENFORCEMENT` is not `off`, because a human session needs a broker grant.
- **What changes when on.** A new session asks Model Gateway for a `human_browse` grant (no hosts; the broker's deny-list is the policy) and sends Quarry no `constraints` block, so no 12-step, 120-second or first-host literal reaches the run. If the grant is not issued (permissive mode), the session opens on the first-host policy, as before.
- **Keep-alive.** `POST /api/v1/browser/sessions/{session_id}/keepalive` renews the session grant by one broker TTL and returns `{"data":{"expiresAt":...}}` only. A grant that cannot be renewed answers 409 `browser_session_expired`: reopen the session. The SPA runs this every 300 seconds only while the tab is visible and the session advertises `human_browsing`.
- **Capabilities.** `human_browsing`, `tab_control` and `attach` are advertised only when Quarry's owner projection says so (`grantKind`, `tabControl`, `attachAvailable`). Until Quarry emits `grantKind`, no session advertises `human_browsing`.
- **Rollback.** Unset `BROWSER_HUMAN_BROWSE` (or set `0`) and restart the gateway. Sessions opened in the meantime keep their grant until it expires on its own TTL.
- **Check the switch.** With the switch on, a new session's grant is a `human_browse` grant (`kind` in the broker's grant record), and the keep-alive answer carries no grant id.

## Agent attach: shared browser tabs with a person (WS8.F, dark)

Status: built to scope, dark. `QUARRY_EDGE__ATTACH` is off in every environment. Nothing in production calls the routes: the gateway has no attach path yet (WS8.I). Enabling it needs the WS8.H lane evidence and X-13 (D8-6, D8-7). Tests are written and compile; they have not been run.

Switches:

| Variable | Default | Meaning |
|---|---|---|
| `QUARRY_EDGE__ATTACH` | `0` | `1` turns the routes on. Unreadable at boot fails the edge (`RunLimits::from_lookup`); unreadable at a request is off. |
| `QUARRY_EDGE__ATTACH_IDLE_TTL_S` | `180` | An attachment with no activity for this long is released. A value below 1 fails at boot. |

Routes (the owner's token unless the row says otherwise):

- `POST /v1/agent/runs/{run_id}/attach`: a service principal only. Body `{grant_id, start_url?, constraints: {max_steps 1..200 (default 20), max_runtime_s 1..3600 (default 120)}}`. Opens one agent tab in the background, with the attach grant's hosts as its egress policy.
- `POST /v1/agent/runs/{run_id}/step` (and `interact`) with `grant_id`: a delegate step. A caller that does not own the run is refused with 404 unless it is the live attachment's delegate. The tab gate, the attach grant, the attach budget and the run chain run in that order.
- `DELETE /v1/agent/runs/{run_id}/attach/{attach_id}?disposition=release|abort`: the delegate ends its own attachment, or the owner stops it. Closes the agent tab. Does not revoke the grant.
- `POST /v1/agent/runs/{run_id}/tabs/{tab_id}/control` `{holder: human|agent, expected_epoch?}`: the owner's own token only. `holder: agent` needs a live attachment on that tab.

Refusals a delegate or an operator will see (the typed reason is in `error.message`, or in `error.code` for the first six):

- `USER_TOOK_OVER` (409), `details.scope` `tab` or run-level: a person holds the tab or the run. The delegate's next step is refused until the person gives the tab back (`holder: agent`).
- `RATE_LIMITED` (429), reason `attach_steps` or `attach_runtime`: the attachment's own budget. Only delegate steps are counted.
- `SECURITY_BLOCKED` (403): the attach grant is no longer usable. The attachment ends as `revoked` (its `agent.detached` carries `disposition: revoked`). The grant is not changed here.
- `attach_disabled`, `attach_zdr_refused`, `attach_requires_grant`, `attach_grant_invalid`, `attach_requires_service_principal` (403); `attach_driver_unsupported` (UNSUPPORTED); `attach_exists`, `stale_tab_epoch`, `no_attached_agent` (CONFLICT, 409); `pinned_tab_closed` (NOT_FOUND).

Events and timeline: `agent.attached` `{attach_id, tab_id}` and `agent.detached` `{attach_id, disposition}` carry ids and dispositions only, never a URL or a title. The timeline carries `attach` and `tab_control` items. A ZDR run never reaches the attach path and writes nothing for it.

Rollback: set `QUARRY_EDGE__ATTACH=0` and restart the edge. Live attachments end at their next step (404 for the delegate). Receipts already written keep their `tab_id` and `attach_id` (migration `0018_step_receipt_tab_attach.sql` is additive and has no backfill).

Not done (open, see `.plan-work/quarry-w1w2/depth/W2-U67.md`): the list-tabs view for a delegate (filtered to its tab, with holder and openedBy); the projection fields `tabControl`, `attachAvailable` and `attach`; the sweeper's release of an idle or lapsed attachment; the `session_ended` release at `finish_run`; the per-lane step counter (`step_seq`, WS8.H); the enabled-path integration test with a driver double that supports `open_tab`; the ZDR feature-matrix rows for the new routes. A process restart ends every live attachment (the attach state is process-local).

## Execution lanes: the person's and the attached delegate's (WS8.H, unit W2-U68, built, NOT run, 2026-10-24)

- A live run has two lanes: the person's (`RunEntry.human_lane`) and each attachment's (`AttachState.lane`). A lane owns its observation context and its last observation. The run summary (current URL, page hash, successful step count, last observation) sits on the run entry.
- A step takes its lane, then the run entry. The entry is released while the browser call runs. So a person's action does not wait for a delegate step, the reverse holds too, and a projection reads the entry without waiting for a lane. Lock order: a lane, then the entry, never the reverse.
- The step key is reserved after admission from one run-wide counter, so event and receipt keys are unique across lanes. A refused action reserves no key. A failed browser call now uses up its key (before, a retry reused the number). The projection, the checkpoint and the legacy step cap still count successful steps only.
- Step keys stay unique across a resume (D2, R3-SC4 depth 2, 2026-10-29). A resume used to start the keys at the checkpoint's `step`, the count of successful steps, so a run with a failed step reused a key after a resume (live: receipts with step 1 twice, a completed wait before the resume and a failed wait after it; the same key in the `action.started` / `observation.ready` idempotency keys, the proof id and the SPA's replay entry id). Every checkpoint save now stores `next_step`, the run-wide counter (a JSON field in `state`, no migration), and a resume starts at the highest of `next_step`, `step` and one past the run's last receipt (`resume_next_step`). A failed step saves a checkpoint too (best effort, logged as `could not checkpoint a failed step`; not for a ZDR or closed run), because its receipt may not be kept (a person's step under `HUMAN_RECEIPT_MODE=none`, a failed receipt write); that save also keeps the failed step's budget charge. The receipt step covers a failed step whose checkpoint save failed and a run checkpointed before the field existed (it reads as 0). A resume reads only the run's highest receipt step (`last_step_for_org`, `SELECT MAX(step)` in Postgres, no payload decoded); if that read fails the resume fails, as a failed checkpoint read does. Left open: a failed step whose receipt write and checkpoint save both fail can still have its key handed out again after a resume. Keys may skip numbers after a resume (they already did for failed steps); `step`, the step cap, the projection's `step` and the key of a takeover's `skipped` receipt are unchanged.
- A close (explicit, expiry, drain) waits up to 5 s in total for a lane that is still running an action, then releases the browser session. A lane still held at that point is logged (`close: an action still holds a lane; releasing anyway`) and released anyway.
- No flag and no route change. A run without an attachment has one lane; its behaviour is the same apart from the key note above.
- Not built or not run: the browser-backed evidence (a delegate `wait` of 8 s with a person's `click` returning in under one second, measured), the sweeper release of an idle attachment and the `SessionEnded` end item at close (W2-U67 depth). Test phase: `cargo test -p quarry-edge agent_ lane` (the seven lane tests in `agent_routes/tests.rs`; they check the locks, not a real slow step).


## Profile routes: redacted reads, service principals, ZDR gate, import scope (WS17.P0, W2-U30)

What changed on `/v1/profiles*` (all flag-gated or fixed in this release, see the table below):

- `GET /v1/profiles/:id` answers the redacted view. `cookies[].value` is `null` with `value_len` set; local and session storage appear as counts; IndexedDB appears as origin, database names and row counts. No response of any `/v1/profiles*` route carries a cookie value, a storage key or value, or an IndexedDB value.
- `POST /v1/profiles/:id/restore_probe` adds `earliest_expiry` (nearest unexpired cookie expiry), `cookies_expired` and `origins_count`. A profile whose cookies have all expired is not restorable unless it holds storage.
- A service principal gets 403 on every `/v1/profiles*` route. This is fixed, not a flag.
- `POST /v1/profiles` (import) needs the `quarry:profiles:import` scope. No production token carries it. The dev bypass stub carries it, and the bypass is refused in production. A caller without it gets 403 `profile import requires the quarry:profiles:import scope`. Grant the scope only after the owner decides who may import.
- The import body is capped at 4 MiB; past it the edge answers 413 (a plain body from the HTTP layer, not the Quarry envelope). The rest of `/v1/*` keeps its 1 MB cap.
- Under a ZDR posture, `create`, `save`, `read`, `update`, `probe` and `attach` are refused only when `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` is on (default off). With the flag off the edge logs `zdr.would_refuse` (row `profiles`, action as named) and serves the request. With it on: 403 `zdr_durable_mutation_forbidden`, `details.zdr_refusal.action` set. `list` and `delete` are never refused (wind-down).
- Production boot logs a warning while the ZDR flag is off.

Flags: `QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` (default false; observe first). The service-principal refusal, the import scope and the redaction are on in every mode. Rollback: revert the commit, or set the ZDR flag back to false.

Reading a profile now: `curl -s -H "Authorization: Bearer $TOKEN" http://edge:8082/v1/profiles/<profile_id>` returns `data.cookies[].value_len` and `value: null`, never the value. The step above (`expires_at`, `viewport.is_mobile`) predates this change and is not rewritten here (this file is append-only for this batch); read `data.cookies[].expires` and `data.has_viewport` instead.

Deployment evidence owed (test phase, not run in the build): (1) a user token creates a profile, saves a snapshot whose cookie value is `CANARY-d16-p0`, and loads it: `grep -c CANARY-d16-p0` on the body returns 0 and the body shows `"value":null`; (2) `docker logs quarry-edge 2>&1 | grep -c CANARY-d16-p0` returns 0; (3) the execution-core service token gets 403 on `GET /v1/profiles` and `POST /v1/profiles`; (4) with the ZDR flag on, a ZDR-org token gets 403 on `POST /v1/profiles/create` and `GET /v1/profiles/:id`, and 204 on `DELETE /v1/profiles/:id`; (5) a 5 MiB import returns 413; (6) `GET /api/ingestions/profiles` through the gateway lists the profile (object rows are read now).

**Agent-run depth, what an operator or a client sees (D-B3-agent-depth, 2026-10-27).**
- A resume of a run that is still live (`POST /v1/agent/runs` with a `resume_run_id` the edge holds, or one another resume is committing) is 409 `CONFLICT` "agent run is already live". Before, the second start replaced the registry entry and dropped the first session without releasing it. A run the sweeper released, or the owner closed, is resumable as before (D1-021).
- The browser socket's error frame is `{type:"error", code, message, retryable, details?}`: `code` is the REST code (`USER_TOOK_OVER`, `FORBIDDEN`, ...), `message` and `details` are masked as the REST body is. It used to be `{type:"error", message}` with the error's Display (`UserTookOver: ...`). The SPA does not read the frame's message (O4).
- `GET /v1/agent/runs/{id}/tabs` is the owner's, and also the live attachment's delegate's: a service principal that is the attachment's delegate sends `?grant_id=<attach grant>` and sees only the tab its attachment opened. Anyone else is the 404 (D2-074).
- A delegate step whose attachment is released, revoked or ended while the browser runs it answers 404, advances nothing on the run, and leaves a failed receipt (`attach_released_during_step`) under its step key. The page effect of the one action that was already running is not undone (D2-031).
- An events stream that arrives after the run's trace was closed ends at once with `done` / `run_closed` instead of waiting on a channel nothing closes (D2-015). `EventSink::subscribe_open` is the subscribe that never creates a channel.
- The owner projection carries `grantKind: "human_browse"` for a person's open-web session and omits it for every other run (D2-030).
- A run carries the egress policy it was started under on its lane (`ObservationContext.egress_policy`) and every action installs that one. The web policy's narrowing and the open-web mode now hold past step 1 (D2-019). The delegate lane has no carried policy yet: see `.plan-work/quarry-w1w2/depth/D-B3-agent-depth.md`.
- Tab calls (open, select, close) are counted in flight only for the run's owner; a stranger's call is the 404 before it counts (O5).

## Receipt text default confirmed; the event follows the flag; key-by-key typing (X-19, D2-210, D2-065, built, NOT run, 2026-10-28)

Owner decision of 2026-10-28 (X-19): `QUARRY_EDGE__RECEIPT_TEXT_MODE` stays `sensitive` by default (it supersedes the sentence "the product owner's answer to X-19 is still owed" in the WS12.S3 section above). Receipts stay off for real users until four things hold: the event follows the flag, D2-065 is fixed, receipts have a retention or erasure path, and the stack evidence passes. This section records the first two as built. The retention path is a design only (`.plan-work/quarry-w1w2/D2-x19-retention-design.md`); nothing is built for it.

- **The flag now covers the event.** The `ActionStarted` event over `GET /v1/agent/runs/{id}/events/stream` is built under the same policy as the stored receipt. `sensitive`: an ordinary typed text is kept in both and a password, OTP, card, phone or secret-labelled text is `[redacted:<kind>]` in both. `all`: every typed text, selected value, dialog prompt and character key is `[redacted:secret]` in both. Before this change the event always used `sensitive`, so `all` still leaked ordinary text through the event. The raw `type` action with a selector that names nothing stays masked as a secret in the event (D2-064).
- **A password typed one key press at a time is masked (D2-065).** A `press` that types a character is masked by the kind of the control the run last focused; with no known focus it is `[redacted:secret]`. `Enter`, `Tab`, `Backspace`, arrows, function keys and Control or Meta shortcuts are kept. The run tracks the focus in memory (`ObservationContext.key_focus`); a resumed run starts with an unknown focus, so its first typed characters are masked. A compiled procedure reports `repair_required` for a masked key. Rules: `docs/SECRET_REDACTION.md`, "WS12.S3 follow-up".
- **Version.** `RULE_VERSION` is `qr-redact/4` (the WS12.S3 section above and its psql check say `qr-redact/3`; that is stale). It was not bumped for this change. A row written before it can hold a raw key press: the stack evidence below inserts one by hand.
- **Evidence owed (stack session, not run here).** In addition to the checks in the WS12.S3 section: on a rebuilt edge with a Postgres store and `zdr:false`, drive a login form by `press` one key at a time into a password field (click the field first, then one `press` per character), then `psql -c "SELECT action->>'key' FROM quarry_step_receipts WHERE run_id='<run>' ORDER BY step"`: every character key reads `[redacted:password]`, `Enter` reads `Enter`; the `ActionStarted` events on the stream show the same. Repeat with `QUARRY_EDGE__RECEIPT_TEXT_MODE=all` and a search box: the event and the row both show `[redacted:secret]`. The edge must be restarted for the flag to change.
- **Not done:** retention or erasure of stored receipts, the dropped-observation counter, the stack evidence, and a `RULE_VERSION` bump with a read-time re-mask of `qr-redact/4` key presses (needs the golden pin regenerated; decide after the retention design).

## Profile owner: stamp at run start, default scope, legacy count (D16 Q2, WS17.P2, built, NOT run, 2026-10-28)

This section corrects and extends "Profile owner and ACL check (WS17.P2, unit W2-U24 ...)" above and the WS17.P1 paragraph that says the capture paths stamp no owner and `create_profile` does not stamp one. Both statements are stale: `create_profile` stamps the caller (profile_routes.rs), and since the commit "stamp the profile owner at run start" a person's run does too. The attach call site that the older section lists as "Not wired yet" has been wired since D2-045 (`start_run` calls `profile_guard::authorize` with `ProfileOp::Attach`). The decision is D16 Q2, option C (owner order of 2026-10-28). `QUARRY_EDGE__PROFILE_OWNER_ENFORCE` is still `observe` by default: nothing here turns enforce on.

- Owner stamp at run start. `start_run` (`agent_routes/lifecycle.rs`) calls `profile_guard::stamp_run_owner` after the caps, the policy and the lease. It creates the row of the run's profile with the caller as owner, only if no row exists (`ProfileStore::stamp_owner`: Postgres `INSERT ... ON CONFLICT DO NOTHING`; an existing row, owned or legacy, is never touched). It refuses when a deletion tombstone newer than the request's start is on record, as `save` does, so a profile deleted while a run was starting does not come back. The row it creates has no name, the scope the run asked for (`profile_scope`; the default `ephemeral` is stored as `user_private`) and an empty placeholder snapshot. The run's own save at the end keeps that owner (the owner is stamped on INSERT only).
  - People only. A service principal (a model-driven run) is never stamped: `Claims::actor_id()` is the shared service id, and stamping it would make every model run in the org own those cookies, hidden from the person. Its profile stays ownerless, hidden under enforce. A token with no actor id is treated the same.
  - No stamp for: a resumed run (it keeps its checkpoint's profile), a run that keeps no persistent profile, a ZDR run (refused earlier), and `QUARRY_EDGE__PROFILE_OWNER_ENFORCE=off`.
  - A store error never fails the run start. The stamp is counted as `failed` and the row the run saves later has no owner.
  - Known cost: a run that fails after the stamp leaves an empty placeholder row, owned by its person. It holds no cookies.
- Default scope. A profile saved without metadata is `user_private` in every store (`ProfileMetadata::stored_default()`): Postgres by migration 0005, and now the in-memory stores and the S3 list as well. `ProfileScope::default()` is still `ephemeral` ("no scope given"); a stored row never carries it. The edge reads a row with no metadata as `user_private`, which is owner-only.
- Observe-mode signals (closed labels, no ids). Log lines `profile.owner.would_deny` (existing: `op`, `reason`) and `profile.owner.stamp` (`outcome`). Counters `quarry_profile_owner_decisions_total{outcome=would_deny|denied, op, reason}` and `quarry_profile_owner_stamps_total{outcome=stamped|already_exists|tombstone|failed|skipped_service}`. A `skipped_service` count is the model-driven runs that persisted a profile that will be hidden under enforce; it is the number that tells the owner whether to refuse them or wait for an acting-user claim.
- Legacy rows. A private row with no owner (written before the stamp, or by a model-driven run) stays hidden under enforce and is never deleted. A legacy `org_shared` row stays open to the org. To count them on the live stack (read only, counts and tenant ids, no payload): `QUARRY_PROFILES_DSN_FILE=<file with the connection string> bash scripts/evidence/d16-legacy-profile-count.sh --since <time the stamp reached the edge, ISO 8601 UTC>`, or `psql "$DSN" -X -v ON_ERROR_STOP=1 -v since="'<time>'" -f scripts/evidence/d16-legacy-profile-count.sql`. `--check` runs the offline guard (bash syntax, one READ ONLY transaction, no write keyword, no payload column). The script was run only against a throwaway Postgres 16 with the migrations and six synthetic rows (counts matched), never against a stack.
- Rollout, in order, none of it done: rebuild the edge so the stamp is live; run the count and note the time; observe a week (`quarry_profile_owner_decisions_total`, the stamp counter); choose between a manual, org-confirmed rescue (`UPDATE quarry_profiles SET owner_user_id = '<actor id>' WHERE org_id = '<org id>' AND profile_id = '<profile id>' AND owner_user_id IS NULL;`) and a backfill from run records (user actors only); then set `QUARRY_EDGE__PROFILE_OWNER_ENFORCE=enforce` and restart; then run the two-user evidence below.
- Tests (run in the build, Linux container): `cargo test -p quarry-edge --lib profile_guard` (16), `--test profile_owner` (8), `--test profile_guard` (8), `--test profile_attach` (10, run-path tests: person stamped under observe and enforce, org_shared scope, no persistent profile, service principal not stamped, off, ZDR refused with no row, existing row untouched), `cargo test -p quarry-browser --lib session` (52), `cargo test -p quarry-runtime --lib profile` (48). The Postgres store tests (default scope, stamp insert-only and tombstone) compile under `--features postgres-queue` and skip without `DATABASE_URL`: not run against a database. The Redis-gated `metadata_methods_pass_through_to_inner_uncached` was corrected and did not run (no `REDIS_URL`).
- Evidence owed (stack session): with two users A and B in one org, A starts a run with `persist_profile: true`; `GET /v1/profiles` as A lists it with `owner_is_caller: true`; as B it does not appear under enforce, and `GET /v1/profiles/<id>` as B is a 404 (served under observe, with a `profile.owner.would_deny` line). The S3 profile backend has no tombstones, so its stamp is not ordered against a delete.

## Scrape AI formats: enable, disable and the 501 reasons (INNH-1 U3, built, NOT run, 2026-10-31)

`json`, `summary`, `question` and `highlights` in `formats` of `POST /v1/scrape` are answered inline by a model from the page text (`data.json`, `data.summary`, `data.answer`, `data.highlights`, and a per-format report in `data.ai`). The contract is `docs/CONTRACTS.md`, "Scrape AI formats". The code is `crates/quarry-edge/src/scrape_contract/ai_formats.rs` (the switch, who may ask, the lane) over the engine in `crates/quarry-runtime/src/ai_scrape.rs`. They are dark: `QUARRY_EDGE__AI_FORMATS` is off by default, the committed composes set it to `0`, and no stack has it on.

**Enable (a development or side-by-side stack only; the owner decides, not the compose file).** The switch is read once, so it takes a restart of the edge.
1. Set `MODEL_PLANE_URL` (the gateway, for example `http://model-gateway:8080`). Unset: `501 model_plane_unavailable`.
2. Give the edge the development credential: the stack runs the cross-plane auth dev bypass and `MODEL_PLANE_TOKEN` carries a user ingress token for the gateway; `MODEL_PLANE_INFERENCE_TOKEN` carries a second token with `aud=inference-core` (without it the one token is sent in both headers). The gateway's `/v1/ai/chat` wants both. No credential: `501 model_lane_not_granted`.
3. Set `QUARRY_EDGE_AI_FORMATS=on` and recreate the edge. A deployed stack has no credential for this lane until the X-7 grant exists (the Auth Core minted path returns `model_lane_not_granted` without minting); do not set it there.

**Disable.** Unset it or set it to anything but `on`, `1` or `true`, and restart. Every AI format is then `400 feature_disabled` at `formats[i]` (never `reserved_format`, which no longer exists). Nothing else changes: a request with no AI format is byte for byte what it was.

**What a caller sees.**
| Answer | Reason |
|---|---|
| 400, `details.fields[]` with a path | a malformed AI format (`missing_field`, `schema_keyword_not_supported`, `feature_disabled`, ...); decided before the fetch |
| 403 `details.reason: extract_ai_scope_required`, `details.field: formats[i]` | a service principal without `extract:ai`, or `initiated_by: "agent"`; before the fetch |
| 501 `details.reason: model_plane_unavailable` | the switch is on and `MODEL_PLANE_URL` is unset |
| 501 `details.reason: model_lane_not_granted` | a URL, but no credential for the chat lane |
| 400 (SSE `error` event) `unsupported_in_stream` | an AI format on `POST /v1/scrape/stream`, switch on or off |
| 200 with `data.ai.<kind>.status` `skipped`, `ZDR_UNAVAILABLE` | ZDR (the token floor or the body flag): no model call at all, the page is a normal ZDR scrape; the 501 checks are not made |
| 200 with `status` `error` and a code | `MODEL_UNAVAILABLE` (with `reason`), `MODEL_RATE_LIMITED` (with `retry_after_s`), `SCHEMA_INVALID`; the page fields are still there |

**Limits and cost.** Per request: at most 8 model calls (repair included), 200,000 reported tokens, 60,000 page characters per call, 4 calls in flight, and 16 in flight in the process; the AI phase is bounded at 45 s, and while the switch is on `POST /v1/scrape` gets the 125 s router deadline (as with scrape actions). Nothing is billed: token usage is reported in `data.ai`, and pricing (owner decision OD-3) is open. Quarry logs the kind, outcome, code, reason and counts of each format, never the page, the schema, a prompt or an output.

**Evidence owed (stack session, not run here).** Everything above is proven by tests against a strict double of the gateway's chat route (`tests/ai_formats.rs`, `tests/ai_formats_off.rs`, `tests/ai_formats_real_token.rs`); no real Model Plane gateway has been called. On a side-by-side stack with the dev bypass and two hand-minted dev tokens: a scrape with `["markdown", {"type":"json","schema":...}, "summary"]` returns the fields and `data.ai.total.calls: 2`; the same with `zdr: true` returns `skipped` / `ZDR_UNAVAILABLE` and the gateway's log shows no call; with the switch off it is `400 feature_disabled`. The page-cache test (`an_ai_request_never_reads_or_writes_the_page_cache`) needs a Redis and skips without one.

## Change text diff: enable, disable, read and retention (INNH-4, built, NOT run, 2026-10-31)

A change monitor can keep the page text of each version and a paragraph diff between them, and `GET /v1/change/diff` returns both. The contract is `docs/CONTRACTS.md`, "Change text diff". It is off: `QUARRY_EDGE__CHANGE_TEXT_DIFF` is unset in the default composes, which is off, and no compose line is needed.

**Enable.** Set `QUARRY_EDGE__CHANGE_TEXT_DIFF=1` (also `true`, `yes`, `on`) on the edge and restart it. The edge must be the `postgres-queue` build with `DATABASE_URL` (the baseline store), as for any change recording. Deploy the edge before the orchestrator: the edge accepts `markdown_artifact_id` on the record call and returns it from `run_page`, the orchestrator only forwards it, and an old orchestrator simply sends none (the baseline then has no text and its first change is an unavailable diff, `no_previous_text`).

**Disable.** Unset the variable or set it to `0` and restart. Recording goes back to the one-line fingerprint note and baselines without text; `GET /v1/change/diff` answers 501. Baselines and diffs already stored stay (they are readable again when the flag returns). Nothing needs a migration either way.

**What a person sees.** A monitored page changes: the `change_detected` event and the `quarry.change.detected` webhook carry `diff_id` and `new_baseline`; `GET /v1/change/diff?diff_id=...` returns the added and removed paragraphs (`real_delta` false means the bytes changed and the text did not: an ad or a timestamp). The first change after enabling, for a page that was already monitored, answers `available: false`, `reason: no_previous_text`; the next one has a diff.

**Answers.**
| Answer | Reason |
|---|---|
| 501 `UNSUPPORTED`, message names `QUARRY_EDGE__CHANGE_TEXT_DIFF` | the flag is off |
| 403 `FORBIDDEN` | the caller is under Zero Data Retention |
| 404 `NOT_FOUND` | no baseline for the url, or an unknown id, or an id of another organization (the same message) |
| 200, `available: false`, `reason` | `no_previous_version` (one baseline), `no_previous_text` (recorded before text was kept), `no_text`, `artifact_gone` (a stored page was evicted) |
| 400 on `POST /v1/internal/change/record` | `markdown_artifact_id` is not a markdown artifact of that org; nothing was saved, the activity fails and Temporal retries it |

**Retention.** The page text lives in the artifact store like any page artifact. The artifact janitor deletes by age only; it is dry-run by default (`QUARRY_RETENTION_DRY_RUN` is not `false`). Arming it would delete the markdown of old baselines, and `GET /v1/change/diff` then answers `artifact_gone` for a pair whose diff was not stored (a stored diff keeps its own text). Do not arm it for a deployment that uses text diffs until it spares baseline artifacts. Organization erasure (`org_erasure.rs`) has no baseline handling today, so baseline rows are not removed by it: check that before offering text diffs to a customer who can ask for erasure.

**ZDR.** A ZDR run stores no markdown, returns no `markdown_artifact_id`, and a `zdr: true` record body keeps no text or diff even in observe mode (`QUARRY_EDGE__ZDR_REFUSE_DURABLE_RESOURCES` off), where its baseline row is still written. Reads are refused for a ZDR caller. To check by hand on a side-by-side stack: schedule a monitor on a fixture page, edit one paragraph, wait for the change, read `GET /v1/change/diff?diff_id=` with the id from the event, and confirm the edited paragraph is `added` and the old one `removed`; repeat with an ad-rotation fixture and confirm `real_delta: false`. Not done here: only unit tests with an in-memory store, no real Postgres or Temporal.

## `/v1/extract` with a `prompt` (INNH-2, built, NOT run, 2026-10-31)

`POST /v1/extract` now sends `prompt` to the model: with `schema` as extra instructions, alone as a free-form JSON object (`data`, status `ok`). The contract is `docs/CONTRACTS.md`, "`/v1/extract`: the `prompt` takes effect". This arm has no switch of its own: it runs when `MODEL_PLANE_URL` is set and the edge holds a model credential (`MODEL_PLANE_TOKEN`, or the Auth Core minted path). It calls the gateway's `/v1/invoke`, not the `/v1/ai/chat` lane of the scrape AI formats, so `QUARRY_EDGE__AI_FORMATS` does not gate it. On a deployed stack the lane is ungranted until WS16.S1 to S3 and decision X-7, so a call there is answered by the 501 "lane refused" of INNH-3 after one fetch (see "Enable structured extract and AI formats"); do not point a deployed edge at a gateway for this.

What a caller sees:
| Answer | Reason |
|---|---|
| 400 `BAD_REQUEST` with a `hint` | `prompt` empty after trimming, or more than 10000 characters; before any fetch, and the message never repeats the prompt |
| 501 `UNSUPPORTED` | a `schema` or a `prompt` was sent and the Model Plane is not usable: no URL, no credential, or the gateway refused the first call (three texts, see "Enable structured extract and AI formats"; not for a ZDR request) |
| 200, item `ok`, `schema_valid: false`, `schema_issues` | the model answered something that does not fit the schema; the data is returned, the caller decides |
| 200, item `ok`, `truncated_input: true` | the page was cut at 20000 bytes before the model read it |
| 200, item `error`, `ZDR_UNAVAILABLE: ...` | ZDR (body flag or the token's floor): no page text goes to a model; `structured` still comes back |
| 200, item `error`, message names the "request time budget" | 50 s passed before the model read this source; send fewer URLs |

Nothing in a log or a response carries the prompt text; log lines carry lengths only. To check by hand on a side-by-side stack: send a page with a schema and a prompt, read the request the gateway receives (the prompt before the `<<<PAGE-` line, the page inside), then repeat with `"zdr": true` and confirm the gateway sees no request. Not done here: no real gateway was called.

## Enable structured extract and AI formats (INNH-3, built, NOT run, 2026-10-31)

Read this before telling anyone to "turn extract on". Two Model Plane features share one URL and have different doors.

| | `POST /v1/extract` with `schema` or `prompt` | `json`, `summary`, `question`, `highlights` in `formats` of `POST /v1/scrape` |
|---|---|---|
| Switch | `QUARRY_EDGE__AI_FORMATS` (QB-T17 gates it; off by default); it then runs when the Model Plane URL is set and Quarry holds a model credential | `QUARRY_EDGE__AI_FORMATS` (off by default; see "Scrape AI formats" below) |
| Gateway route | `/v1/invoke` (token-request row `model-invoke`) | `/v1/ai/chat`, with an `x-inference-authorization` header |
| Credential | the Auth Core minted token (needs `QUARRY_EDGE__QUARRY_SERVICE_API_KEY`), or in development `QUARRY_EDGE__MODEL_PLANE_TOKEN` | `QUARRY_EDGE__MODEL_PLANE_TOKEN`, and optionally `QUARRY_EDGE__MODEL_PLANE_INFERENCE_TOKEN` (development only) |
| Not ready | 501 `UNSUPPORTED`, three texts below | 501 `model_plane_unavailable` or `model_lane_not_granted`; 400 `feature_disabled` while the switch is off |

`QUARRY_EDGE__AI_FORMATS` gates `/v1/extract` (QB-T17, merged from main into the integration; before it the switch never gated extract, and INNH-3 documented that interim state). The Innhenting audit (commit `14299e659`) and the 12-item brief said that extract needs `QUARRY_EDGE__AI_FORMATS=1`, and with the gate that is now true: the switch is decided at boot with the other Model Plane switches (see "Answer synthesis" below), and a switch that is on without a model path is refused at boot and stays off. The old 501 hint that said to set `MODEL_PLANE_URL` alone was wrong too.

**There is no production path today.** A deployed stack cannot run structured extract, and it cannot run the AI formats either. Both wait for the same two things: WS16.S1 to S3 (the metered service lane on the Model Plane gateway) and the owner decision X-7 (OD-1: the grant that lets Quarry call the Model Plane: audiences, retention, and the manifest rows set `granted: true`). Until then, in the committed files: the `model-invoke` row of `crates/quarry-runtime/contracts/cross-plane-token-requests.json` is `granted: false`, the Control Plane registry gives `quarry-edge` no `model-gateway` audience, and the gateway refuses a service principal on `/v1/invoke`. Setting `MODEL_PLANE_URL` on a deployed stack therefore enables nothing: it turns the first 501 below into 1b (the AI formats switch is off by default and a switch that is on is refused at boot without a model path), and it makes the edge refuse to boot unless `QUARRY_EDGE__QUARRY_SERVICE_API_KEY` is set. The live stack keeps `MODEL_PLANE_URL` unset. Do not set it.

**The four 501s of `/v1/extract`.** They are decided before any further page is fetched, they share one body, `{"error", "code": "UNSUPPORTED", "hint", "next_actions"}` (no other key; `next_actions` has two entries), and they never name a URL, a host or a token. A request with neither `schema` nor `prompt` (cleaned markdown) and a ZDR request (each source answers `ZDR_UNAVAILABLE`) never get one.
1. No Model Plane URL (`QUARRY_EDGE__MODEL_PLANE_URL` unset, empty or blank). `error`: `structured extraction needs the Model Plane, and QUARRY_EDGE__MODEL_PLANE_URL is not set on this deployment`. No page is fetched.
1b. The AI formats switch is off (QB-T17), or was refused at boot, with a URL set. `error`: `structured extraction is not available on this edge: QUARRY_EDGE__AI_FORMATS is off`. No page is fetched.
2. A URL, but nothing to authenticate with (no service key and no development token). `error`: `structured extraction needs a granted Model Plane lane, and this deployment holds no model credential`. No page is fetched.
3. A URL and a credential, but the gateway refuses the first model call (401 or 403, or Auth Core refuses to mint the token because the row is not granted). `error`: `structured extraction needs a granted Model Plane lane, and the Model Plane refused this service`. Exactly one page was fetched and one model call made; the other sources are not fetched and nothing is metered. Before INNH-3 this was a 200 in which every source (up to 25, fetched one after another) said `status: error`, and the fetches were paid. The stop applies only while no source has been extracted: a refusal after one source came back `ok` stays per source, and a timeout, a 5xx, a non-JSON answer or a 429 never stops the batch (429 keeps its own envelope).

`hint` always starts with "omit `schema` and `prompt` to receive cleaned markdown" and ends with a pointer to this section; `next_actions[0]` is the same retry. A caller that gets a 501 can repeat the same request without `schema` and `prompt` and receive `status: fetched` markdown.

**What works in development (a side-by-side or dev stack only; the owner decides).** Run the edge with the cross-plane auth dev bypass (`QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS=true`, refused in production). Set `QUARRY_EDGE__MODEL_PLANE_URL` to a gateway and `QUARRY_EDGE__MODEL_PLANE_TOKEN` to a token that gateway accepts on `/v1/invoke`. Leave `QUARRY_EDGE__QUARRY_SERVICE_API_KEY` unset: with a service key the minted path is used instead, and it is refused (state 3). Restart the edge; all of these are read at boot. The deploy compose forwards `MODEL_PLANE_URL` and `MODEL_PLANE_TOKEN`; the Ingestion compose forwards only `MODEL_PLANE_URL`, so give the container the token through an override file. Set `QUARRY_EDGE_AI_FORMATS=on` (compose spelling) too: QB-T17 gates the model arm of `/v1/extract` with it, and boot refuses it unless a model path exists (the dev bypass credential is one). Then `POST /v1/extract` with a `schema` returns `status: ok` with `data`. The AI formats of `/v1/scrape` use the same switch and, for the chat route, additionally need the inference token; follow "Scrape AI formats" below.

**For the Innhenting UI.** Show `error` as the reason. Offer the retry in `next_actions[0]`. Do not parse `hint` and do not tell a person to set a flag or a variable: they are for the operator, and no flag unlocks this on the live stack. The three states are told apart by the `error` text only (there is no `details`). The cross-plane asks are WS16.S1 to S3 and decision X-7.

**Checks.** `cargo test -p quarry-edge --lib extract_routes` (unit tests for the three 501 bodies, the key set, zero fetches, the stop after one fetch and one model call, per-source isolation for 5xx, the refusal after an extracted source, ZDR); `node --test scripts/tests/docs-contract.test.mjs` (every `QUARRY_EDGE__` name in these docs, the openapi, the composes and the 501 texts is a setting the code reads, so no phantom variable). Not done: no real Model Plane gateway was called, and the Auth Core refusal of an ungranted row is read from the code, not exercised against Auth Core.

## Scrape `max_age`: enable, disable and what to look at (INNH-7, built, NOT run, 2026-10-31)

`POST /v1/scrape` accepts `max_age` (milliseconds, alias `maxAge`): serve a stored result for the same organization, canonical URL and options if it is at most that old, else fetch. The contract is in CONTRACTS.md 9.1 ("`max_age`") and `docs/openapi.yaml` (`ScrapeRequest.max_age`).

| Variable | Default | Effect |
|---|---|---|
| `QUARRY_EDGE__SCRAPE_MAX_AGE` | `off` (compose: `QUARRY_EDGE_SCRAPE_MAX_AGE`, default `off`) | `on`, `1` or `true` accepts `max_age` and writes the URL result cache; any other value is off. Read once at process start. |

Needs Redis (`REDIS_URL`): without it `max_age` is ignored with the warning `cache_unavailable`. No other setting is needed. The Innhenting gateway does not send `max_age` yet; turning the switch on changes nothing for a request that does not carry it, except that every cacheable success is also written to Redis (about the size of a page-cache entry: the output without page text) for `cache.max_age_s`, default 3600 s.

**Turn on.** Set `QUARRY_EDGE_SCRAPE_MAX_AGE=on` for the edge and restart it. Check: scrape a page twice with `{"url": "...", "max_age": 60000}`; the second answer has `data.effective_options.cache_state: "hit"`, `cached_at`, `cache_age_ms`, the original `data.fetched_at` and no `scrape_id`, and the origin saw one request. `redis-cli --scan --pattern 'quarry:scrape:*'` lists the entries (the org id is in the key; the rest is a hash; no URL is stored in a key).

**Turn off.** Unset it or set it to `off` and restart: a request with `max_age` is a 400 `feature_disabled` at `max_age`, nothing new is written, and existing entries expire by their TTL (they are never read while the switch is off). To drop them at once: `redis-cli --scan --pattern 'quarry:scrape:*' | xargs redis-cli del`.

**Metric.** `quarry_cache_requests_total{cache="scrape", outcome="hit|miss|error"}` counts reads of the URL result cache; a request that did not consult it counts nothing.

**What is never cached** (the response says `max_age_ignored`, or `max_age_ignored_zdr` under ZDR): captures, actions, AI formats, the browser driver, a custom web policy, `ingest`, a private privacy class, a non-2xx page, and a 2xx page the run flagged as a bot wall (the challenge classifier, also under the default `QUARRY_CHALLENGE_ENFORCE=log` where the page is returned) or as a soft-404 ("page not found" in a 200). ZDR neither reads nor writes it and computes no key.

**Limits to know.** The TTL caps the effective `max_age`. A stored result whose artifacts expired is a miss (the page is fetched again). A hit is not metered as `scrape.page`; whether it should be is an open product decision. A redirect is keyed by the URL as requested, not the final one. The trailing slash and the query order are part of the URL identity.

**Tests.** `crates/quarry-edge/tests/scrape_max_age.rs` (Redis, `REQUIRE_CONTRACT=1` makes an unreachable Redis a failure), `tests/scrape_max_age_off.rs` (no Redis), unit tests in `cache.rs`, `scrape_cache.rs`, `scrape_contract.rs` and `quarry-core` (`scrape.rs`, `cache.rs`). The `quarry-v2-ci.yml` step "Page-cache and ZDR cache gates (Redis)" runs `cache_policy.rs`; `scrape_max_age.rs` is not in it yet (an owner/CI decision, recorded in INNH-7-status.md).

## Job results: enable, disable, read (INNH-12, built, NOT run, 2026-10-31)

The pages a crawl or batch fetched, with the ids of what was stored for each, come back from `GET /v1/crawl/{id}/results`, `/v1/batch/{id}/results` and `/v1/jobs/{id}/results`. The contract is `docs/CONTRACTS.md`, "Job results". It is off: `QUARRY_EDGE__JOB_RESULTS` is unset in the default composes, which is off, and no compose line is needed.

**Enable.** Set `QUARRY_EDGE__JOB_RESULTS=1` (also `true`, `yes`, `on`) on the edge and restart it. Rebuild both the edge and the orchestrator: the edge returns the artifact ids from `run_page`, the orchestrator copies them onto each `page_fetched` event, and control reads the rows back. Deploy the edge first. Pages fetched before the orchestrator carries the fields have rows without artifact ids (url, status and title only); nothing is lost and nothing needs a migration. Replaying an in-flight workflow is safe: only the payload of an event changes, no workflow command is added.

**Disable.** Unset the variable or set it to `0` and restart. The three routes answer 501; the events keep the extra fields.

**Answers.**
| Answer | Reason |
|---|---|
| 501 `UNSUPPORTED`, message names `QUARRY_EDGE__JOB_RESULTS` | the flag is off |
| 403 `FORBIDDEN`, `details.reason` is `job_result_urls` | the caller is under Zero Data Retention |
| 403 | a service principal |
| 404 `NOT_FOUND` | unknown id, an id of another organization, or a crawl id on the batch path |
| 400 `BAD_REQUEST`, `details.field` | `limit` outside 1 to 500, `after_seq` not a number, `latest` not true or false |

**Reading a run.** Page with `after_seq=<next_seq>` until `next_seq` is absent. Use `latest=true` for a dataset with one row per URL. Fetch a page with `GET /v1/artifacts/{markdown_artifact_id}`. On the in-memory artifact backend (`ARTIFACT_BACKEND=memory`, the default compose) the ids stop resolving when the edge restarts; use the filesystem backend for results that must outlive it. `GET /v1/artifacts?run_id=<run>&kind=markdown` lists one run's artifacts on the memory and filesystem backends (not on S3, which cannot list).

**Cost and scale.** A read is one indexed page of the job's events (`events_job_id_idx` is on `job_id`; the type and seq filters run over that job's rows). A 5 000-page crawl is 10 pages at the maximum limit. `latest=true` reads the whole job (up to 20 000 fetches) on every call, so page through it with `after_seq` rather than polling it.

**ZDR.** A ZDR run stores no artifact and its `page_fetched` carries none of the ids; the routes are refused for a ZDR caller anyway. The events already hold each page's url and title (existing posture); this makes them easy to read, which is why the routes are dark. To check by hand on a side-by-side stack: crawl five fixture pages, call `GET /v1/crawl/{id}/results` and confirm five rows whose `markdown_artifact_id` each return the page from `GET /v1/artifacts/{id}`; repeat with a ZDR token and confirm 403. Not done here: Go and Rust unit and route tests only, no stack, no Temporal and no Postgres were run.

## Webhook subscriptions: enable, disable, rotate, retry, erase (INNH-5, built, NOT run, 2026-10-31)

Customers can register an HTTPS endpoint that receives their organization's `change_detected` events, signed. The contract is `docs/CONTRACTS.md`, section 4 (what the endpoint receives) and "Webhook subscriptions" (the API). It is off: neither `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS` nor `QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS` is set in the default composes, and no compose line is needed. Two things are not behind a switch and ship with the control image: the fan-out only delivers to the subscriptions of the event's own organization (an event of unknown owner delivers to nobody), and `/v1/webhooks` and `/v1/webhook-deliveries` on control answer 410.

**Order.** Deploy control first: it applies migration 025 at boot (`webhooks` and `webhook_deliveries` gain `org_id`; nothing is dropped or rewritten, a legacy row keeps `org_id ''` and delivers to nobody). Then the edge. Then switch on: set `QUARRY_CONTROL__WEBHOOK_SUBSCRIPTIONS=1` on control (it mounts the routes and starts the delivery retention sweep), restart it, then `QUARRY_EDGE__WEBHOOK_SUBSCRIPTIONS=1` on the edge (also `true`, `yes`, `on`). With the edge on and control off the routes answer 501 and the message names the control variable. The edge also needs the change webhook to reach control for anything to be delivered: `QUARRY_EDGE__CONTROL_BASE_URL` and `QUARRY_EDGE__INTERNAL_SECRET` are set, which the change monitor already requires.

**Disable.** Unset the variables and restart. Subscriptions and deliveries stay in the database, and the switches do not stop delivery: control's fan-out and dispatcher read no switch, so while a subscription row exists its organization's events are still delivered. To stop one, `PATCH` it with `active: false` (needs the routes on) or delete it. To stop all at once, delete the rows: `DELETE FROM webhooks WHERE org_id <> ''` (deliveries go with them by cascade).

**Day to day.**
| Task | How |
|---|---|
| A customer lost its secret | `POST .../rotate-secret`; the new secret is in that response only, the old one stops at once. There is no way to read the current one |
| A customer's endpoint was down | `GET .../deliveries?status=dlq`, then `POST .../deliveries/{id}/retry` for each; a retry restarts the attempt count |
| An endpoint is refused as `blocked_target` | its host resolves to a private or loopback address (or a rebinding mix). The customer must serve it from a public address; nothing on the control side allows it. `EGRESS_ALLOWLIST_HOSTS` and `EGRESS_ALLOWLIST_CIDRS` must stay empty on control in production: they widen the guard for the dispatcher and for every other control egress |
| Too many finished rows | `QUARRY_CONTROL__WEBHOOK_DELIVERY_RETENTION_DAYS` (default 14, 1 to 3650; an invalid value logs and keeps 14). The sweep runs hourly while the control switch is on and deletes only `success`, `failed` and `dlq` rows |
| Organization erasure | `PurgeOrg` deletes the organization's `webhook_deliveries` and `webhooks`; `PurgeResult` reports `webhooks_deleted` and `webhook_deliveries_deleted`. Legacy rows with no organization are not matched by it |

**Legacy rows.** A `webhooks` row from before migration 025 (no tenant route could create one, so there should be none on a deployed stack) has `org_id ''`. It receives nothing and is not listed by any tenant route. Delete it by hand if the table has any: `SELECT id, url FROM webhooks WHERE org_id = ''`.

**Owner decision: the secret at rest.** The HMAC needs the raw key, so control stores `webhooks.secret` in plaintext; the API never returns it after create or rotate, and no log line carries it, but a database reader has every organization's keys. The proper fix is envelope encryption with a control key (a KMS-held data key wrapped per row); it is not built. Accept plaintext at rest for the first customers, or require the envelope before switching on? Until decided, keep the switches off outside a side-by-side stack.

**Owner decision: ZDR.** A Zero Data Retention organization cannot create a subscription, patch one or retry a delivery (403, `webhook_off_process`); it can read, delete and rotate. Nothing is emitted for a ZDR check in any case. Control cannot tell an organization's posture; the edge's token floor is the only gate, and an organization that turns ZDR on later keeps the subscriptions it made until it deletes them (they receive nothing from ZDR checks). Say if enabling ZDR should also deactivate them.

**Not done by this build.** Run against a real stack: the control Go tests ran against an in-memory store and, for the Postgres store, against a scratch Postgres 16; the edge tests run against a control double. No end-to-end run with a real receiver. To check by hand on a side-by-side stack: create a subscription through the edge with a public receiver (a tunnel, or a host on the test network the guard allows), run `POST /v1/change/check`-driven or scheduled changes twice with a changed fixture, confirm the receiver gets one POST per change, recompute `v1` from the documented recipe, then stop the receiver and watch the delivery go `pending` (attempt 1, then 2, ...) and `dlq` on the fifth failure; `rotate-secret` and confirm the next delivery verifies only with the new one.

## Crawl `sitemap` and `scrape_options`: enable, disable and read the result (INNH-6, built, NOT run, 2026-10-31)

`POST /v1/crawl` accepts `sitemap: include|only` and `scrape_options` (per-page cleaning). Both are dark. The contract is `docs/CONTRACTS.md`, "Crawl `sitemap` and `scrape_options`". The request errors are in `docs/ERROR_REASONS.md`.

**Enable (a development or side-by-side stack only; the owner decides, not the compose file).** Each flag is read once, so it takes a restart of the edge. The orchestrator needs no flag, only the new build: it acts on the job params.
1. Sitemap: `QUARRY_EDGE__CRAWL_SITEMAP=on`. The crawl scope must be on (`QUARRY_EDGE__CRAWL_SCOPE_MODE` is `observe`, the default, or `enforce`). The orchestrator reaches the edge's `POST /v1/internal/crawl/sitemap` with the runtime token and the internal secret it already uses for `run_page`.
2. Scrape options: `QUARRY_EDGE__CRAWL_SCRAPE_OPTIONS=on`.
3. Orchestrator first, then the edge flag, when both are rolled. The new orchestrator is inert without the params (a job with no `sitemap` or `scrape_options` runs exactly as before), and an old edge still answers 400 `unsupported_option` at the handoff, so no new crawl can start on it. The reverse order is the unsafe one: the edge stamps `sitemap` and `scrape_options` on the job params and echoes them in `data.effective`, an old orchestrator ignores unknown params, so with the edge flag on first it would crawl links-only with the default cleaning while the ack says otherwise. Do not turn an edge flag on until the orchestrator in front of it is the new build. Only a crawl already in flight can see a new orchestrator talk to an old edge (see Disable).

**Disable.** Unset the flag and restart the edge. The handoff answers 400 `unsupported_option` again. A crawl already running keeps its params: with the sitemap flag off the discovery route answers 501 (an `include` crawl goes on with links, an `only` crawl ends with `sitemap_unavailable`); with the scrape flag off `run_page` ignores `scrape` and does not echo it, so the orchestrator ends the crawl with `edge_scrape_options_unsupported`.

**Read the result.** `data.effective.sitemap` and `data.effective.scrape_options` on the ack. The job's `run_completed` event carries `sitemap` (`discovered`, `seeded`, `skipped{out_of_scope, robots_disallowed, duplicate, cap, invalid, sitemaps_refused, sitemaps_unavailable}`, `sitemaps_fetched`, `nested_fetched`, `truncated`). A high `sitemaps_refused` means the robots.txt names a sitemap on another host or an internal address: that is refused on purpose. `truncated: true` means a bound stopped the read (20 files, 100,000 entries or 30 seconds). A stored markdown artifact is the cleaned text: compare one with and without `exclude_tags` to see the difference.

**Limits to know.** Sitemap URLs are queued at depth 1; the in-memory frontier (an orchestrator without the edge queue credentials) takes at most 500 of them. Sitemap files are not metered. A page cleaned with non-default options skips the page cache, so a repeat crawl fetches those pages again.

**Tests (Linux container, `qtest-lane.sh`).** `cargo test -p quarry-edge --lib crawl_` (65 before the integration files), `--test crawl_sitemap`, `--test crawl_options_on`, `--test crawl_options_off`, `--test crawl_sitemap_scope_off`, `--test web_policy_routes`, `--test zdr_feature_matrix`; Go: `GOWORK=off go test ./...` in `services/quarry-orchestrator`; `node --test scripts/tests/*.test.mjs`.

**Evidence owed (stack session).** On a side-by-side stack with the crawl fixture (`deploy/compose/crawl-fixture/`) and a sitemap that lists a page no link reaches: `sitemap: include` finds it and `skip` does not; `scrape_options.exclude_tags: ["nav"]` removes the nav text from a stored markdown artifact. The in-flight replay of a history recorded before the marker is not tested (there is no recorded history to replay).


## Answer synthesis: enable, disable and the reasons (INNH-8a, built, NOT run, 2026-10-31)

`POST /v1/answer`, `POST /v1/answer/stream` and `include_answer` on `POST /v1/search` are answered by one pipeline: search, read the top pages, ask the Model Plane. The contract is `docs/CONTRACTS.md`, "Answer synthesis"; the code is `crates/quarry-edge/src/answer_status.rs` (the gates and the reasons), `model_plane_gate.rs` (the one boot gate that owns the switch) and `answer_routes.rs`. It is dark: `QUARRY_EDGE__ANSWER_SYNTHESIS` is off by default, both composes set it to `0`, and no stack has it on. `MODEL_PLANE_URL` alone does not turn it on.

**One switch, one boot gate (QB-T17).** `QUARRY_EDGE__ANSWER_SYNTHESIS` is decided at boot by `model_plane_gate` together with the other Model Plane switches. A switch that is on is REFUSED, and stays off, unless the edge has a model path: `MODEL_PLANE_URL`, and either a service identity whose manifest row `model-invoke` (`crates/quarry-runtime/contracts/cross-plane-token-requests.json`) is `granted`, or the development bypass credential. The shipped manifest grants nothing, so on a deployed stack the boot log shows an `ERROR` that names `QUARRY_EDGE__ANSWER_SYNTHESIS` and the routes answer `501` with `reason: model_path_refused`. The answer status is derived from that decision; the answer code has no switch of its own.

**The gates, in the order the edge checks them.** The edge reports the first closed one (`reason`, and on `/v1/answer` a `hint` that names the setting). The switch is read once, so every change takes a restart of the edge.
1. `QUARRY_EDGE_ANSWER_SYNTHESIS=1` (compose variable; the edge reads `QUARRY_EDGE__ANSWER_SYNTHESIS`). Closed: `synthesis_disabled`.
2. A model path, as the boot gate sees it: `MODEL_PLANE_URL` (the gateway, for example `http://model-gateway:8080`) and a granted `model-invoke` row or the dev bypass credential. The switch on without them: `model_path_refused`; without the URL: `no_model_plane_url`.
3. A search provider: `SEARXNG_URL`, `BRAVE_SEARCH_KEY` or `SERPER_KEY`. Closed: `no_search_provider`. (Live search works, so this is open wherever search works.) A URL the Model Plane client cannot be built from: `model_client_init_failed`, with the cause in the edge boot log.
4. A Model Plane that accepts the edge. This is the cross-plane part and it is NOT in Quarry's hands. With 1 to 3 open through the dev bypass and no real grant, a call is `model_plane_auth_refused` (503 on `/v1/answer`, `answer_status: unavailable` on search); on a deployed stack the boot gate stops earlier, at step 2 (`model_path_refused`). It needs, all of them (owner decision INNH-8b, not built): the Control Plane registry (`apps/Control Plane/config/plane-service-principals.json`) giving quarry-edge the `model-gateway` audience with scope `models:invoke` and the `zdr` retention lane; the Model Plane gateway letting a service principal reach the route Quarry calls (`authorize_principal_route` in `model-gateway/src/auth.rs` allows only `POST /v1/ai/chat` and `/v1/ai/embeddings`, Quarry calls `POST /v1/invoke`) or Quarry moving to `/v1/ai/chat`; Quarry sending the second `x-inference-authorization` header; and the `model-invoke` row of `crates/quarry-runtime/contracts/cross-plane-token-requests.json` (and Auth Core's copy) flipping to `granted: true` in the same change as the registry grant.

**Enable (a development or side-by-side stack only; the owner decides, not the compose file).** Set 1 to 3 above and recreate the edge. On a side-by-side stack with the cross-plane auth dev bypass, steps 2 and 4 are the development credential: `QUARRY_EDGE__CROSS_PLANE_AUTH_DEV_BYPASS` on and `MODEL_PLANE_TOKEN` carrying a user ingress token the gateway accepts on `/v1/invoke` (the same credential the AI formats use; not verified against a real gateway here). A deployed stack must not set the switch until the grant exists: the boot gate refuses it (`model_path_refused`, an `ERROR` in the boot log) and nothing is served. After the grant, flip the `model-invoke` row and the registry together, then set the switch.

**Check it worked.** `POST /v1/search` with `{"query": "capital of norway", "include_answer": true}` and a user token, read `answer_status`: `ok` (and an `answer`) is working. `unavailable` with `answer_reason` is the gate that is closed. Then `POST /v1/answer` with the same query: 200 with `answer`, `citations` and `model`. Cost: an `include_answer` search runs the search provider twice (once for the results, once inside the pipeline), reads up to 5 pages (about 10k input tokens at most) and calls the model once.

**Disable.** Unset the switch (or set it to anything but `1`, `true`, `on`) and restart. `/v1/answer` is then `501 synthesis_disabled`; `include_answer` says `unavailable` / `synthesis_disabled`; nothing else changes.

**What a caller sees.** The reason table is `docs/ERROR_REASONS.md`, "Answer synthesis". In short: 501 with a `reason` (`synthesis_disabled`, `model_path_refused`, `no_search_provider`, `no_model_plane_url`, `model_client_init_failed`) for a gate that is closed; 403 `zdr_unavailable` for a ZDR request; 503 `model_plane_auth_refused` for the cross-plane grant; 429 `model_plane_rate_limited` and 502 `model_plane_error` for a failing hop; `answer_search_failed` when the search or page fetch failed first. A failed synthesis is not cached: the search cache skips a request that asked for an answer and has none, so the first query after step 4 is true is answered.

**Billing.** One `quarry.answer.synth` per model call, on `/v1/answer`, the stream and an `include_answer` search (metadata `via: "search.include_answer"` on the last); never on a cache hit, a refusal, a failure, `no_sources` or ZDR. `quarry.search.query` is metered for the search as before.

**ZDR.** The model hop carries no ZDR posture (ADR-4), so no ZDR request reaches a model. `include_answer` under ZDR sends nothing to a model (`answer_status: skipped_zdr`, the search runs under ZDR). `/v1/answer` and `/v1/answer/stream` REFUSE a ZDR request (body flag or the token's floor) with `403 FORBIDDEN`, `reason: zdr_unavailable`, before the gate, the cache and the pipeline: no search, no page fetch, no Model Plane call, and the refusal comes before the 501 checks. The `answer` row of `docs/ZDR_MATRIX.md` is `refused`, enforced. This closes the exposure for a ZDR organization even with the switch on. If an operator sees 403 `zdr_unavailable` for an organization they did not expect to be under ZDR, check the token's `zdr` floor and the request's `zdr` field, not the synthesis settings.

**Evidence owed (stack session, not run here).** Everything above is proven by tests against a double of the gateway's `/v1/invoke` (`tests/answer_provider_status.rs`); no real Model Plane gateway was called. On a side-by-side stack with the dev bypass: one `include_answer` search with the switch on and a credential the gateway accepts (expect `answer_status: ok` and one `quarry.answer.synth` event), the same with the credential removed (expect `model_plane_auth_refused` and no event), and the same with `"zdr": true` (expect `skipped_zdr` and no request at the gateway); and `POST /v1/answer` with `"zdr": true` (expect 403 `zdr_unavailable` and no request at the gateway).

## API-key agent runs: enable, disable and the refusals (INNH-10, Quarry side built dark, NOT run, 2026-10-31)

An API key may drive a fenced browser run on `/v1/agent/*` once the whole chain exists. The contract is `docs/CONTRACTS.md`, "API-key agent runs"; the reasons are `docs/ERROR_REASONS.md`, "API-key agent runs"; the design and the asks to the other planes are `.plan-work/quarry-w1w2/INNH-10-design.md`. The code is `crates/quarry-edge/src/api_key.rs` (the route fence), `agent_routes/api_key_runs.rs` (the start rules), `agent_routes/admission.rs` (the per-step rules) and `grant_boot.rs`. **It is dark: `QUARRY_EDGE__API_KEY_AGENT_RUNS` is `0` by default and no compose file sets it.** With it off a key token is refused on every route and every other caller is served as before.

**What must exist first (other owners; none is built).** Auth Core mints the `api_key` token (a gateway-only exchange of the key, ask CP-1 to CP-4); the BrowserBroker issues key grants stamped with the key's id, with hosts inside the key's host ceiling and a mandatory budget, and revokes them when the key is revoked (MP-1, MP-2); the model-gateway has the key-grant door (MP-3); the Verevon gateway composes exchange, grant and run (GW-1). Until the broker's `ValidateGrantResponse` carries `key_id` and Quarry vendors the field into `crates/quarry-runtime/proto/browser.proto`, the gRPC validator always yields an empty `key_id`, so EVERY key run is refused with `api_key_grant_owner_mismatch` on a gRPC-validated edge (fail closed, on purpose). The HTTP shim validator reads `key_id` from its JSON today (development only).

**Enable order (the owner decides each step; do not change a live flag without that).** 1. Auth Core flag for the key exchange (CP-1). 2. BrowserBroker `BROWSER_BROKER_KEY_GRANTS_ENABLED` and the model-gateway door. 3. On the edge: `QUARRY_EDGE__BUDGET_ENFORCEMENT=on` (a key run is refused with `api_key_budget_not_enforced` otherwise: in `shadow` the grant's budget would bound nothing), a real grant validator (`QUARRY_EDGE__BROWSER_GRANT_VALIDATOR_GRPC_URL` and `QUARRY_EDGE__QUARRY_SERVICE_API_KEY`; or the HTTP shim outside production), then `QUARRY_EDGE__API_KEY_AGENT_RUNS=1` and a restart. Optionally `QUARRY_EDGE__MAX_CONCURRENT_RUNS_PER_KEY` (default 2, at least 1).

**Boot refusals.** The edge does not start with the switch on and no validator endpoint (`ApiKeyRunsWithoutValidator`: the no-op validator accepts any grant id, which for a key would be an open door), whatever the environment; it does not start with a value it cannot read (`QUARRY_EDGE__API_KEY_AGENT_RUNS` must be 0, 1, on, off, true or false). With the switch on the boot log carries a warning line.

**Kill switches, fastest first.** Revoke the key in Auth Core: new tokens stop at once and a live run's next call fails within the token's life (at most two minutes); the broker revokes the key's grants on the revocation event and the run's next step fails with `browser grant is inactive or expired`. Or set `QUARRY_EDGE__API_KEY_AGENT_RUNS=0` and restart: every key token is refused on every route again, the live key runs of that edge are released with it. A key run that stops stepping is released by the idle sweeper like any agent run.

**Check it worked (side-by-side stack, owner approval needed).** With a key that holds `browser:agent`: acquire a key grant through the gateway, `POST /v1/agent/runs` with the key's token and the grant id (expect 200 and `constraints.max_steps` no larger than the grant's), one `wait` step (receipt `agent.basis` `api_key`, `principalId` `apikey:<key_id>`), then revoke the key and step again (expect 403). Negative checks: the same start without the grant id (`api_key_grant_required`), with the owner's own session token and the key grant (`api_key_grant_owner_mismatch`), `GET /v1/artifacts` with the key token (`api_key_route_refused`), and a third run of one key (429 `per_key_run_cap`). **Not run here: nothing in this item has been run against a stack.**

**A refusal you did not expect.** `api_key_agent_runs_disabled`: the switch is off on that edge. `api_key_scope_missing`: the key lacks the permission. `api_key_grant_owner_mismatch` on a gRPC edge with a valid grant: the proto carries no `key_id` yet (above). `api_key_budget_not_enforced`: `QUARRY_EDGE__BUDGET_ENFORCEMENT` is not `on`.

## Semantic rerank (Data Plane `/v1/rerank`)

Quarry reorders the top search results with the Data Plane's rerank (`rerank-v1`, profile
`web-v1`, a paid Azure cross-encoder). No Model Plane is involved and no general model is prompted.

Switches on quarry-edge (each: default, boot check, effect):

- `QUARRY_EDGE__SEMANTIC_RERANK`: off in the binary, and off in the Ingestion compose
  (`QUARRY_EDGE_SEMANTIC_RERANK` defaults to `0`). Set it to `1` only after the ship check passes (see
  Deploy order). On needs `QUARRY_EDGE__DATA_PLANE_URL` and the cross-plane tokens. Boot logs
  `semantic rerank: enabled (Data Plane /v1/rerank)`; if an input is missing it logs one ERROR naming
  it and rerank stays off.
- `QUARRY_EDGE__SEMANTIC_RERANK_TOP_N`: default 10, clamped to 1..100 (the Data Plane's limit).
- `QUARRY_EDGE__RERANK_DEADLINE_MS`: the budget of one call in milliseconds, default 2000 (the compose
  sets it from `QUARRY_EDGE_RERANK_DEADLINE_MS`). It is sent as the Data Plane's `deadline_ms`, clamped to
  100 to 8000, the range the Data Plane accepts. Quarry stops waiting at the budget plus 0.5 s, and a miss
  degrades the search to its input order (`degraded`, reason `timeout`). Boot logs `deadline_ms` on the
  `semantic rerank: enabled` line.
- `QUARRY_EDGE__RERANK_SHADOW`: off in the binary. The Ingestion compose sets it to `1`
  (`QUARRY_EDGE_RERANK_SHADOW`) for the first deploy's smoke run, and the orchestrator sets that default to
  `0` after the live proof. In shadow mode the Data Plane is asked and its answer measured, but the results
  keep their input order and carry no score (`status: shadow`). Boot logs a WARN if it is set while
  `SEMANTIC_RERANK` is off.
- `QUARRY_EDGE__ZERO_SAAS_SEARCH`: on a sovereign deployment the reranker is never called
  (`skipped sovereign`, no call).

Data Plane switches (on `retrieval-engine`, default safe): `WEB_RERANK_ENABLED` and
`WEB_RERANK_PROCESSOR_APPROVED` are off until set, so the route answers `skipped disabled` or
`skipped processor_unapproved` and makes no provider call. `WEB_RERANK_ALLOWED_SERVICES` defaults to
`service:quarry-edge`.

Reading an outcome: each search response carries `rerank.status` (`applied`, `skipped`, `degraded`,
`shadow`, or `rerank-not-configured`) and `rerank.reason`. Every such response writes one
`search.rerank` log line with `outcome`, `reason`, `profile`, `score_kind`, `cache_hit`, `zdr` and
`hits`. The query text is never logged. In shadow mode `rerank.reason` is `would_apply` when the Data Plane's order would have reordered the head. Scores reach a caller only when it sends
`score_kinds: ["cross_encoder.v1"]`; the response then names `score_kind`.

What never reaches the Data Plane: a ZDR run (from the body or the user's token), and an org that is
not a tenant (`onboarding` or `global`, reason `input_not_tenant`). A service caller that omits `zdr` is refused
with `ZDR_REQUIRED`. Degraded and skipped answers are never cached.

Deploy order (owner ruling 2026-10-12). Do not ship the rerank client before the Data Plane gate and the
Control Plane grant are live: until they are, every non-ZDR search skips or degrades its rerank, the
outcome is never cached, and each search pays the search providers again. The order, the sign-off marker the
check accepts and the rollback order are in `apps/Data Plane v2/docs/web-rerank-v1.md` (deploy order for
quarry-edge). The check is `bash "apps/Data Plane v2/scripts/qb-preflight.sh" --live --ship-qb-t09`. The compose default is `0`, so a deploy is dark until `QUARRY_EDGE_SEMANTIC_RERANK=1` is set after the
check passes (exit 0). Do not set it on a deploy whose check fails.

The two Data Plane switches are `WEB_RERANK_ENABLED` and `WEB_RERANK_PROCESSOR_APPROVED`. The processor
switch is `true` only after counsel's Article 28 sign-off (OD-LEGAL), and the check accepts it only with
`DPV2_PREFLIGHT_ART28_CONFIRMED=yes` recorded. The Control Plane grant is `data:rerank` for `quarry-edge` on
`data-plane`, which is in the committed registry and live in auth-service (read-only check, 2026-10-13). Model Gateway sends no `score_kinds`
until QB-T12 ships its `score_kind` calibration in `relevance.rs`, so QB-T12 ships before or with any change
that makes Model Gateway ask for `cross_encoder.v1`.

Failures and what to do:

- `degraded`, reason `refused`: the Data Plane answered 401, 403, 404 or 422 to the rerank call, and the
  results keep their input order. One ERROR line is written a minute. That window is shared with the token
  refusals below, so a misconfigured principal writes one line a minute, not one per search. A 401 or 403
  means the quarry-edge principal lacks `data:rerank` or its audience: check the registry row. A 404 can mean the route is not deployed on the Data Plane (the live check of 2026-10-13 found an image
  without `/v1/rerank`), and a 422 means the `rerank-v1` contract has drifted: check the goldens.
- `degraded`, reason `token_unavailable`: Auth Core refused the `web-rerank` token (an ERROR line in that
  shared window when Auth Core answers 401 or 403) or could not be reached (a warning). The token log line
  says which.
- `degraded`, reason `contract`: the answer did not match `rerank-v1`. That is a 400, a 413 or another status
  other than 200, 429 or 5xx, an answer that does not decode, or an answer for another request or profile.
  Check the goldens and the Data Plane's version.
- `degraded`, reason `rate_limited`: the Data Plane's rate gate answered 429. The provider was not called,
  so the breaker does not count it.
- `degraded`, reason `provider_error` (a 5xx from the Data Plane) or `unreachable` (no answer, or a body cut
  off): both count against the breaker.
- `skipped`, reason `circuit_open`: five consecutive failed calls opened the breaker for 30 s. It then admits
  one probe, and a success closes it.
- `degraded`, reason `provider_rate_limited`: the Data Plane answered `degraded` because its rerank provider
  reported a rate limit. The breaker counts it.
- `degraded`, reason `no_scores`: the Data Plane answered `ok` with no usable scores for the head. The breaker
  counts it.
- `skipped`, reason `processor_unapproved`, `misconfigured` or `metering_unavailable`: the Data Plane's own gate
  declined before any provider call (see the Data Plane switches above). That is not a failure of the route.
- `degraded`, reason `timeout`: the Data Plane did not answer inside the budget (`QUARRY_EDGE__RERANK_DEADLINE_MS`,
  2 s by default).

Known gap: the candidates go to the processor for every non-ZDR tenant org, and they are not all public web
pages. The org's own indexed pages (the local index, always registered) are candidates whether or not the
vector leg is on, and with the vector leg on (`QUARRY_EDGE__VECTOR_RETRIEVAL`) its vector hits are too, so
their titles and snippets reach the processor. Quarry's per-request `allow_third_party_processing` privacy
flag is not carried on the search path. The processor gate that applies today is the Data Plane's deployment switch
`WEB_RERANK_PROCESSOR_APPROVED`, which the owner's legal confirmation (O2) decides before production.
