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
   grpcurl -plaintext model-plane-broker:9090 model_plane.v1.BrowserBroker/Health
   ```
2. Check grant TTL — default is 60 minutes. Long-running agents should refresh.
3. If the broker is down, Quarry falls back to `NoopGrantValidator` only when configured to (dev mode) — production should NEVER set `BROWSER_BROKER_GRPC=` empty.

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
