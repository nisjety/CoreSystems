# Billing — Usage Metering Contract

P3 / cluster #billing (shipped after P2).

Quarry emits per-request usage events to Control Plane's `billing-core`,
which dedupes them and forwards to Lago for invoicing.

## Wire shape

Payload exactly mirrors `billing-core`'s `UsageEvent` Go struct
(`apps/Control Plane/billing-core/internal/billing/types.go:43`):

```jsonc
{
  "event_id":   "quarry:<request_id>:<metric>",  // primary dedup key
  "org_id":     "org_01H...",                    // verified JWT claim
  "metric":     "quarry.scrape.page",            // free-form, matches Lago
  "quantity":   1.0,                             // f64; semantics owned by Lago
  "source":     "quarry-edge",
  "occurred_at":"2026-05-19T12:34:56Z",          // RFC3339 UTC
  "metadata":   { "user_id": "usr_...", ... }    // free-form context
}
```

## Transport

**NATS JetStream**, subject filter `usage.>`. Quarry publishes to
`usage.<metric>` so each metric gets its own subject and consumers can
filter granularly:

| Subject                       | Producer | Consumer            |
| ----------------------------- | -------- | ------------------- |
| `usage.quarry.scrape.page`    | quarry-edge | billing-core      |
| `usage.quarry.crawl.seed`     | quarry-edge | billing-core      |
| `usage.quarry.search.query`   | quarry-edge | billing-core      |
| `usage.quarry.answer.synth`   | quarry-edge | billing-core      |
| `usage.quarry.batch.url`      | quarry-edge | billing-core      |

billing-core's JetStream stream `CONTROL_PLANE_EVENTS` covers
`["user.>", "organization.>", "session.>", "billing.>", "usage.>"]`,
so the subjects are routable end-to-end without extra config.

**Recovery path:** `POST /api/v1/billing/orgs/:orgId/usage` on
billing-core accepts the same JSON shape over HTTP if NATS is unhealthy.

## Metric taxonomy

| Constant                            | String                       | When emitted                                     | Quantity                |
| ----------------------------------- | ---------------------------- | ------------------------------------------------ | ----------------------- |
| `usage_metrics::SCRAPE_PAGE`        | `quarry.scrape.page`         | After successful `/v1/scrape` (skipped on error) | `1.0` per page          |
| `usage_metrics::CRAWL_SEED`         | `quarry.crawl.seed`          | On every `/v1/crawl` job Control creates (not a replay) | `1.0` per seed URL      |
| `usage_metrics::SEARCH_QUERY`       | `quarry.search.query`        | On every successful `/v1/search`                 | `1.0` per query         |
| `usage_metrics::ANSWER_SYNTH`       | `quarry.answer.synth`        | On every successful `/v1/answer` and `/v1/answer/stream`, and on every `/v1/search` with `include_answer` that called the model (metadata `via: "search.include_answer"`); never on a cache hit, a refused or failed synthesis, an empty `no_sources` answer or a ZDR request | `1.0` per synthesis     |
| `usage_metrics::BATCH_URL`          | `quarry.batch.url`           | On every `/v1/batch` job Control creates (not a replay) | `len(urls) as f64`      |
| `usage_metrics::EXTRACT_LLM`        | `quarry.extract.llm`         | Per AI format that finished `ok` on `/v1/scrape`, and per `/v1/extract` source whose model run finished `ok`; only under `QUARRY_EDGE__USAGE_LLM_METRICS=on` (see "AI format metrics (WS16.S6)") | `1.0` per format |

**Pricing model:** these are free-form strings — Lago billable-metric
codes must match exactly. To add a new metric:

1. Define it in `crates/quarry-runtime/src/usage.rs::metrics`
2. Register a matching billable metric in Lago via the billing-core admin API
3. Emit from the handler

## Idempotency

billing-core dedupes by `event_id` in the `billing_usage_dedup` table.
Quarry constructs IDs as `quarry:<request_id>:<metric>` so a single
request that emits multiple distinct metric kinds won't collide with
itself. NATS-level redelivery is automatically de-duplicated.

**Crawl and batch (decision 3.11, D2-033).** The two durable handoffs are keyed by Control's
job id, `quarry:<job_id>:quarry.crawl.seed` and `quarry:<job_id>:quarry.batch.url`, never by the
request id (a client can send any `X-Request-Id`, and a reused one used to collapse two jobs into
one billing row). Only a job this call created is billed:

- A replay is not billed. Control answers 200, not 201, when the caller's `Idempotency-Key` names
  a job it already holds; the edge then emits no event. A client that retries a crawl or batch
  with the same key is billed once, in every browser-metrics mode.
- A refused or failed handoff (control unreachable, a 5xx, a full queue) created no job and is not
  billed, in every mode (before this change only `on` skipped it, and only the queue-full case
  was skipped in `off` and `shadow`).
- Calls without an `Idempotency-Key` make a job each and bill each.
- A job that Control created but whose first response never reached the client is billed when the
  edge saw the 201. If the edge died between Control's 201 and the meter call, the retry is a
  replay and the seed is never billed (an under-bill, recorded as a depth finding).

billing-core is unchanged. The synchronous routes (search, map, legacy extract, answer) are not
idempotent: a retry re-does the work, so it is billed again, under a fresh id each time. Real
idempotency for them needs a stored response and a body fingerprint and is not built.

## Tenant attribution

- `org_id` is **always** the verified JWT claim — never the
  client-supplied body field. P0 / cluster #auth+tenancy enforces this
  upstream by overwriting `req.org_id` with `claims.org_id` in every
  handler.
- `user_id` rides in `metadata.user_id` so billing-core can attribute
  cost back to the individual user when needed (Lago doesn't natively
  model per-user inside an org).

## Failure mode

| Failure                              | Behavior                                                  |
| ------------------------------------ | --------------------------------------------------------- |
| NATS broker down                     | Publish error logged; user request **succeeds** anyway     |
| Serialize error                      | warn-logged; user request succeeds                         |
| `NoopUsageMeter` (no NATS configured)| Silently discards; user request succeeds                   |
| billing-core temporarily down        | JetStream buffers (7-day retention); replays on recover    |

**Usage metering is never on the request critical path.** Every
`meter()` call `tokio::spawn`s the publish so the handler returns
immediately.

## Configuration

The meter is wired automatically in `main.rs`:

- `QUARRY_EDGE__NATS_URL` set + reachable → `NatsUsageMeter`
- Otherwise → `NoopUsageMeter` (warning log on boot)

No separate billing env vars — the meter rides on the same NATS
connection used for cross-plane event fan-out (P1 / cluster #nats).

## Tests

- `crates/quarry-runtime/src/usage.rs` — 5 unit tests:
  - `event_id_format_is_quarry_request_metric`
  - `serializes_to_billing_core_wire_shape` (pins JSON field names)
  - `subject_derivation_matches_usage_wildcard`
  - `noop_meter_silently_discards`
  - `recording_meter_captures_payload`

A `RecordingMeter` fixture is exposed at `crate::tests::RecordingMeter`
for downstream handler-level integration tests that want to assert the
emitted shape without spinning up NATS.

## Future work (not P3 scope)

- **Per-MP-token cost** — meter Model Plane spend on `/v1/answer` via a
  follow-up `quarry.answer.tokens.<model>` metric with quantity = total
  input+output tokens. Requires MP gateway to surface a token count
  per invoke (today the response carries only `model_used` + `content`).
- **Storage-day metric** — emit `quarry.storage.day` once per artifact
  per 24h for stored scrape outputs. Requires a background sweeper job.
- **Per-search SERP-provider cost** — split `search.query` into
  `search.query.brave` / `search.query.serper` so paid providers can
  attribute their unit cost to the right tenant.

## Browser and crawl metrics (WS6.C-e)

Seven metrics are added by `QUARRY_EDGE__USAGE_BROWSER_METRICS` (`off`, `shadow`, `on`;
default `off`). They are gated by `usage_attrs::ShadowMeter` in front of the meter, so the
call sites emit them unconditionally and the mode decides what happens:

| Constant                                 | String                              | Emitted by                                                | Quantity                          |
| ---------------------------------------- | ----------------------------------- | --------------------------------------------------------- | --------------------------------- |
| `usage_metrics::BROWSER_SECOND_AGENT`    | `quarry.browser.second.agent`       | agent run segment close (`finish_run`)                    | whole seconds of active time, floored |
| `usage_metrics::BROWSER_SECOND_INTERACTIVE` | `quarry.browser.second.interactive` | manual session segment close                              | whole seconds, floored            |
| `usage_metrics::AGENT_RUN`               | `quarry.agent.run`                  | agent run segment close                                   | `1.0`                             |
| `usage_metrics::AGENT_STEP`              | `quarry.agent.step`                 | agent run segment close                                   | completed steps of the segment    |
| `usage_metrics::CRAWL_PAGE`              | `quarry.crawl.page`                 | `/v1/internal/run_page` with `kind=crawl`, off ZDR        | `1.0` per page                    |
| `usage_metrics::EXTRACT_URL`             | `quarry.extract.url`                | `/v1/extract`                                             | URLs fetched or extracted (failures free) |
| `usage_metrics::MAP_CALL`                | `quarry.map.call`                   | `/v1/map`                                                | `1.0` per call                    |

**Event ids.** Run segments use `quarry:<run_id>:s<segment>:<metric>`; the segment is `0` for the
first start and goes up by one on each resume, so a resumed run never reuses an id. Other events
use `quarry:<request_id>:<metric>` (extract, map), `quarry:<run_id>:<sha256(url)[:16]>:quarry.crawl.page`
(crawl page: a retried page activity adds no second row), or `quarry:<job_id>:<metric>` with
Control's job id (crawl seed, batch; see Idempotency). Every id is checked against billing-core's rule (`^[A-Za-z0-9][A-Za-z0-9._:-]*$`, at most
128 bytes) before it is sent; a refused id is logged by class and not sent.

**Metadata.** The closed set is `outcome` (`ok`, `error`, `refused`, `cancelled`, `timeout`,
`budget_exhausted`, `expired`), `surface`, `class` (`agent` or `interactive`), `driver`,
`estimated`, `zdr`, `user_id`, `counts`; off ZDR also `host` (map only), `paid_provider` and
`llm_format`. Under ZDR the only keys are `zdr, outcome, class, surface, driver, estimated, counts,
user_id`: no URL, host, title or fingerprint is written. `surface` is `api` for any user token;
a service token names its surface in `x-verevon-surface` (closed set `chat`, `browser_ui`,
`space`, `api`, `schedule`, `mcp`, `internal`), and an absent or unknown value is `internal`.
Only the agent start route reads the header; the other routes derive `surface` from the token.

**Modes.**

- `off` (default): the seven metrics are dropped. The existing metrics keep their legacy rules.
- `shadow`: the seven metrics are counted in `quarry_usage_shadow_events_total{metric}` and published
  nothing. The existing metrics keep their legacy rules. Use it for a week before `on`.
- `on`: the seven metrics are published. The corrected rules for the existing metrics apply:
  a crawl seed or batch is billed only when the orchestrator accepted the job (no seed on a failed
  handoff; this rule now holds in every mode, see Idempotency); a scrape event carries the closed metadata set (no `url`, no `fingerprint`); `/v1/extract`
  is billed per URL as `quarry.extract.url` rather than as `quarry.search.query`; `/v1/map` is billed as
  `quarry.map.call` rather than as `quarry.search.query`.

**Prerequisite.** Each new metric needs one Lago billable-metric code before `on` reaches invoices. No
such code exists in the repository; billing-core's `usage_test.go` pin for the id shape stays with the
billing owner (WAITING-OTHER). Rollback is `off`.

**Not in this change.** Usage is emitted at segment close only, so a segment that ends with a crash is
not billed (ZDR runs are unbilled by design, ADR-4). The shutdown drain flush is WS6.J. Segment usage
needs budget enforcement not `off`, because the budget ledger is the clock.

## AI format metrics (WS16.S6)

`quarry.extract.llm` (`usage_metrics::EXTRACT_LLM`) meters the AI formats (`json`, `summary`,
`question`, `highlights`) under its own switch, `QUARRY_EDGE__USAGE_LLM_METRICS` (`off`, `shadow`,
`on`; default `off`; read once at boot, a value outside the set is `off` and logged once). It is
not one of the seven browser metrics and `QUARRY_EDGE__USAGE_BROWSER_METRICS` does not govern it.

- **Emitted by.** `POST /v1/scrape`: one event per AI format whose report is `ok`
  (`scrape_contract::ai_formats::run_ai_formats`). `POST /v1/extract`: one event per source whose
  `json` run is `ok`, after the request is known to answer 200 (a request that ends in the 429 or
  the lane-refused 501 emits none). A failed or skipped format emits nothing. Under ZDR no format
  runs, so nothing is emitted.
- **Id.** `quarry:<usage_id>:<url_hash16>:<kind>:quarry.extract.llm`. `usage_id` is server-minted
  (the scrape's run id; one minted id per `/v1/extract` request), never the caller's
  `x-request-id` (D2-033). `url_hash16` is the first 16 hex digits of SHA-256 of the page URL; the
  URL itself is never in the event.
- **Quantity.** `1.0`.
- **Metadata.** The closed set `kind` (`json`, `summary`, `question`, `highlights`), `repair`
  (`none`, `local`, `model`; `none` for every format but `json`), `zdr` (always `false` in practice),
  `input_tokens`, `output_tokens` (what the gateway reported for the format). No content, no URL, no
  schema, no prompt.
- **Modes.** `off`: nothing is built or sent. `shadow`: each event is counted in
  `quarry_usage_shadow_events_total{metric="quarry.extract.llm"}` and nothing is published. `on`:
  published through the usage meter like every other metric.
- **Prerequisite.** A Lago billable-metric code `quarry.extract.llm` before `on` reaches invoices.
  Rollback is `off`.

## Organization limits (WS6.D-s)

What an organization has left of a metered allowance is read through a resolver, not computed from the
events this file describes. Precedence when sources exist: a grant's claim (tighten only), then the
billing pull, then the static floor. Only the floor is built.

| Metric | Used by | What the remainder does |
|---|---|---|
| `quarry.crawl.page` | `POST /v1/crawl` | lowers `max_pages` (`clamped_by: entitlement`); 0 refuses (402) |
| `quarry.batch.url` | `POST /v1/batch` | keeps the first N URLs (`limits.urls`); 0 refuses (402) |
| `quarry.browser.second.agent` | `POST /v1/agent/runs` (agent class) | lowers the runtime bound (`by: entitlement`); 0 refuses (402) |

Rules. An organization with no allowance policy for a metric is *unconfigured*, which keeps the floor and is
never zero. An entitlement can lower a budget bound and can raise a capacity cap only up to the process-wide
cap (`effective_capacity`); the plan capacity keys (`quarry.cap.*`) are carried in `OrgLimits.caps` and read by
WS6.G and WS6.H later. Human browser seconds are metered and never capped.

Configuration. `QUARRY_EDGE__LIMITS_SOURCE=static` (default; `billing_core` stops boot, it is not built) and
`QUARRY_EDGE__LIMITS_FAIL_MODE=floor|open|closed` (default `floor`; read by every source, no effect on
`static`). A misspelt value stops boot naming the variable. The `billing_core` source also needs the
billing-core read credential (`billing:quota:read:any`, `billing:entitlement:read:any`), which this branch
does not provision.
