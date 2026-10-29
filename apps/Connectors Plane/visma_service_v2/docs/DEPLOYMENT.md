# Deployment Guide — visma_service_v2

Operational runbook. Landing zone: raw JSON in BigQuery staging (`dw_staging_visma` in the data
warehouse project `aquatiq-datavarehus`), see [bigquery_staging.md](bigquery_staging.md). The scheduler
runs every registered tenant (`config/tenants`) in three tiers with delete reconciliation; see § 4 and
§ 4a. What is ready and what is not, and the order to go live in, is in
[PRODUCTION_READINESS.md](PRODUCTION_READINESS.md).

## 1. Environment Variables

### Auth

| Variable | Required | Notes |
|---|---|---|
| `VISMA_CLIENT_ID` | yes | OAuth client id |
| `VISMA_CLIENT_SECRET` | yes | OAuth client secret |
| `VISMA_OAUTH_SCOPE` | yes | ERP scope string |
| `VISMA_OAUTH_SCOPE_DATAMART` | no | DataMart scope (omit until DM access provisioned) |
| `VISMA_OAUTH_SCOPE_EXPENSE` | no | Expense scope (omit to disable expense endpoints) |
| `VISMA_TOKEN_URL` | no | Default: `https://connect.visma.com/connect/token` |

### Endpoints

| Variable | Default |
|---|---|
| `VISMA_ERP_REST_BASE_URL` | `https://api.finance.visma.net` |
| `VISMA_DATAMART_GRAPHQL_URL` | `https://datamart.visma.net/graphql` |
| `VISMA_EXPENSE_API_BASE_URL` | — (unset = expense disabled) |

### Tenant

| Variable | Default | Notes |
|---|---|---|
| `VSV2_PILOT_TENANT_ID` | `33f319b9-4d57-11ee-960c-025417856183` | Aquatiq AS |
| `VSV2_AQUATIQ_DATAMART_COMPANY_ID` | — | Populate via `vsv2 dm-companies` |

### Storage

| Variable | Required | Notes |
|---|---|---|
| `VSV2_DATABASE_URL` | yes | Postgres DSN |
| `VSV2_REDIS_URL` | yes | Redis URL |
| `VSV2_POSTGRES_PASSWORD` | production | Required by `docker-compose.prod.yml` (letters and digits only: it is part of a URL); read only when Postgres creates an empty data volume |
| `VSV2_HTTP_BEARER_TOKEN` | `vsv2-http` | The HTTP service refuses to start without it |
| `VSV2_BIGQUERY_PROJECT` | no | GCP project, `aquatiq-datavarehus` by default in compose; omit for dry-run |
| `VSV2_BIGQUERY_LOCATION` | no | `europe-north2`, the location of the warehouse datasets |
| `VSV2_BIGQUERY_DATASET_STAGING` | no | Staging dataset (`dw_staging_visma`). When set, syncs write raw JSON there and the bronze/shadow settings are ignored |
| `VSV2_BIGQUERY_DATASET_BRONZE` | no | Bronze dataset (e.g. `visma_bronze`) |
| `VSV2_BIGQUERY_DATASET_SHADOW` | no | Shadow dataset — defaults to `<bronze>_shadow` |
| `GOOGLE_APPLICATION_CREDENTIALS` | no | Path to service account JSON (in compose: `/run/secrets/gcp.json`, mounted from `VSV2_GCP_CREDENTIALS_FILE`). The account needs BigQuery Data Editor on `dw_staging_visma` and BigQuery Job User on the project |
| `VSV2_LOCAL_BRONZE_POSTGRES_ENABLED` | no | Local Postgres dry-run sink; compose turns it off for the scheduler |

### Rate limits

| Variable | Default | Notes |
|---|---|---|
| `VSV2_RATELIMIT_ERP_REST_RPM` | `60` | ERP REST requests/min |
| `VSV2_RATELIMIT_ERP_REST_RPD` | `5000` | ERP REST requests/day |
| `VSV2_RATELIMIT_DATAMART_RPM` | `60` | DataMart requests/min |
| `VSV2_RATELIMIT_DATAMART_RPD` | `10000` | DataMart requests/day |
| `VSV2_RATELIMIT_EXPENSE_RPM` | `30` | Expense requests/min |
| `VSV2_RATELIMIT_EXPENSE_RPD` | `2000` | Expense requests/day |

### Scheduler intervals

| Variable | Default | Notes |
|---|---|---|
| `VSV2_SCHEDULE_TIER1_SECS` | `3600` | Hourly — ERP operational endpoints |
| `VSV2_SCHEDULE_TIER2_SECS` | `86400` | Daily — standard sync endpoints |
| `VSV2_SCHEDULE_TIER4_SECS` | `604800` | Weekly — full pass over every enabled endpoint, reconciles deletes |
| `VSV2_SCHEDULE_POLL_SECS` | `60` | How often the scheduler looks for due work |
| `VSV2_SCHEDULE_PARALLEL_TENANTS` | `4` | Tenants running side by side |

The compose scheduler also sets its own request budget (`VSV2_SCHEDULER_ERP_RPM`, default 240,
and `_RPD`, `_DM_RPM`, `_DM_RPD`): the limiter is one bucket for the whole process, so the
`.env` values meant for a single manual sync (60 per minute, 5,000 per day) would stretch the first
full pass over days.

## 2. Bootstrap

```bash
# Start Postgres + Redis
docker compose up -d postgres redis

# The state schema (migrations/*.sql) is applied by `vsv2 schedule` and `vsv2-http` when they start,
# from the files embedded in the binary; `vsv2 migrate` does it on its own, and `make migrate` (psql)
# still works. A recorded file that is edited afterwards is refused: a changed schema is a new file.
cargo run -p vsv2-cli -- migrate

# Validate all endpoint configs (0 errors / 135 endpoints)
make validate-config

# Discover DataMart company id — set VSV2_AQUATIQ_DATAMART_COMPANY_ID in .env
cargo run -p vsv2-cli -- dm-companies
```

## 3. Pre-deploy Gate

All of these must pass before deploying; `.github/workflows/visma-service-v2.yml` runs them on every
pull request that touches this folder:

```bash
make validate-config                              # 0 errors / 135 endpoints
cd core && cargo fmt --all --check
cd core && cargo clippy --workspace --all-targets -- -D warnings
cd core && cargo test --workspace                 # needs the compose Postgres and Redis (make up)
uv run ruff check . && uv run ruff format --check .
uv run pytest                                     # tooling, scripts and the Dataform tools
(cd dataform && dataform compile)                 # the warehouse project compiles
docker compose build vsv2                         # the image builds and every vsv2-bq-* command starts
```

`make validate-config` enforces:
- **Invariant #4** — Tier 1 endpoints cannot have `datamart.ready: true`.
- JSON Schema compliance for all endpoint configs.

## 4. Run

```bash
# One-shot sync (all endpoints for a tenant)
cargo run -p vsv2-cli -- sync --tenant 33f319b9-4d57-11ee-960c-025417856183

# One-shot sync (single endpoint)
cargo run -p vsv2-cli -- sync --tenant 33f319b9-4d57-11ee-960c-025417856183 --endpoint customers

# Complete scan of one endpoint: no checkpoint, rows Visma no longer returns get deleted_at
cargo run -p vsv2-cli -- sync --tenant 33f319b9-4d57-11ee-960c-025417856183 --endpoint accounts --full

# Scheduler for every tenant in config/tenants (hourly / daily / weekly full); --once = one round
cargo run -p vsv2-cli -- schedule
cargo run -p vsv2-cli -- schedule --once --tenant 9b1fa69b-5568-11ee-960c-025417856183
```

## 4a. Container

```bash
docker compose build vsv2                 # builds the Rust binaries + the Python vsv2-bq-* tools
docker compose up -d vsv2                 # the scheduler, all tenants (the state schema is migrated at start)
docker compose logs -f vsv2
docker compose run --rm vsv2 schedule --once --tenant <id>   # one bounded round
```

On a server, add the production file. It has no password defaults (the stack refuses to start without
`VSV2_POSTGRES_PASSWORD`), publishes no Postgres or Redis port, rotates the logs, gives the scheduler
120 s to finish a write when it is stopped, and starts the HTTP API only with `--profile http` and a
bearer token:

```bash
docker compose -f docker-compose.yml -f docker-compose.prod.yml up -d vsv2
```

Credentials are mounted, never baked: `VSV2_GCP_CREDENTIALS_FILE` (default: the gcloud ADC file of
the user running compose) is bound read-only to `/run/secrets/gcp.json`; the container user is
uid 1001, so on a Linux host the file must be readable by it. The image build fails if any
`vsv2-bq-*` command is missing. What the scheduler did is in `vsv2.scheduler_pass`:

```sql
SELECT tier, tenant_id, status, started_at, attempted_endpoints, failed_names
FROM vsv2.scheduler_pass ORDER BY started_at DESC LIMIT 20;
```

## 4b. Open-set reconciliation

A sync that follows `lastModifiedDateTime` cannot see a payment: the invoice keeps its timestamp, so
staging holds it as `Open` until the weekly full pass (232 such invoices, 8.07 M NOK, in the old
warehouse; 211 paid since v2's own snapshot). A part payment is the same blind spot in a smaller
form: the invoice stays `Open` and its timestamp stays, only its `balance` moves.

**What it does.** For endpoints with `"open_set": true` (`customer_invoices`, `supplier_invoices`),
once a day per tenant, at the end of the endpoint's hourly sync: `vsv2-bq-open-keys` reads the
documents staging holds as open (one query, the tenant's live rows with `status = 'Open'`: each key
and its staged `balance`); Visma's own list is read (`GET <api_endpoint>?status=Open`, 1,000 a page,
no `expand*` flags; a row that is not `Open` means Visma ignored the filter and fails the run). Three
kinds of document are then fetched by key (`/v1/customerinvoice/<referenceNumber>`,
`/v1/supplierInvoice/<documentType>/<referenceNumber>`, with the endpoint's `expand*` flags) and
upserted through the normal write path (validated, quarantined if they fail, `vsv2-bq-stage`, 100 at
a time): keys open in staging only, keys open in Visma only, and keys open on both sides whose
`balance` differs by more than half a cent from the one on Visma's list row. Both sides read the
field `balance`, the remaining balance in base currency (`balanceInCurrency` is the same in the
document's currency and moves with it); on 2026-10-13 all 917 open customer and 218 open supplier
invoices of Aquatiq AS in staging hold it as a JSON number, and staging was written from the list
rows of the same endpoints. A balance missing or unreadable on either side is unknown, not equal:
nothing shows that no payment was made, so that document is fetched too. It costs one request, and
a document that has no balance at all comes back as what staging holds, so nothing is written.
Nothing is deleted: a 404 is counted and left to the weekly full pass. The sync's checkpoints are
not touched. It runs only after a successful hourly sync, under the tenant lock, and only with
BigQuery staging. A run fetches at most 500 documents (the rest wait: the queue is the keys open in
staging only first, then those open in Visma only, then the part-paid ones, each in key order),
gives up after 3 failed fetches in a row, ends after 4 minutes or when the scheduler shuts down,
and keeps what earlier batches wrote. A failure is logged and recorded and never fails the sync.
Its stage write adds a `_sync_heartbeat` row like any other, so the population gate's "last sync"
can be a few minutes later than the incremental sync it follows.

**It sits inside the hourly pass.** The run is not a job of its own: it is the last step of the
flagged endpoint's sync, under the tenant's lock, so while it runs (4 minutes at the most; seconds
when little differs; due once a day for each flagged endpoint, every 2 hours after a failed or
partial run) the tenant's other endpoints wait their turn. A run that needs longer, a full queue of
500 fetches can, is cut there, keeps the batches it wrote and goes on after 2 hours. The bound is
`RUN_TIMEOUT` in `open_set.rs`; it ends every request inside it too, so a stuck Visma or BigQuery
cannot hold the lock longer.

**Cadence.** One `vsv2.sync_checkpoint` row per tenant and endpoint, partition `_open_set`
(`last_sync_at` = when the last attempt began, `last_id` = `succeeded`, `partial`, `stalled` or
`failed`; no watermark). Next run: 24 hours after a success, and after a stalled run (nothing it asked
for changed a row of staging: every document was 404, did not validate, or came back as it was
already staged, and no request failed, so asking again gets the same answers); 2 hours after a
failed or partial one, which includes a run that changed some rows and has more to go. A restart does
not repeat a run. A row dated more than 5 minutes ahead of the clock is ignored, and a marker in it
that this version does not know counts as a failed run (retry in 2 hours), not as a success; a row
without a marker counts as one.

**Documents Visma answers 404 for** cannot be written: marking a row deleted is the weekly full
pass's job (it sets `deleted_at`, and the key query reads live rows only). Until then such a key is
in every run's diff and takes one of the 500 places, asked again each time. It can hold back the
documents behind it only when 500 or more of them stand in front: each takes one place and the
others still get the rest, so a queue with fewer of them keeps moving. A run that asked for nothing
but 404s is stalled and asks again after a day, so the worst case is 500 requests a day until the
weekly pass, and the run's note says so when it asked for 10 or more documents and every answer
was 404 (`every by-key answer (N) was 404`). The queue is not reordered for them: telling "gone"
from "not yet fetched" takes the very request.

**Cost** (per tenant and endpoint, per run): one BigQuery query, 1.95 MB (customers) or 2.16 MB
(suppliers) processed, measured on 2026-10-13 for Aquatiq AS (the scan is table-wide, so the same
for every tenant), billed at the 10 MiB minimum (a dry run reports 84.2 and 80.0 MB, an upper
bound). The balance is one more JSON path of `payload` next to the status and the key fields, never
the rest of it: it added 0.25 MB per query (the same query without it processed 1.70 and 1.92 MB),
and the total is still about a fifth of the minimum, so the bytes billed did not change. One list
request (the pilot's 931 open customer invoices on 2026-10-12 fit one page; above 1,000 it is two);
one request per differing document. Per day, for 12 tenants and 2 endpoints: 24 queries (49.3 MB
processed, 252 MB billed) and 24 list requests; the hourly tier covers only the 8 active tenants, so
really 16 (32.9 MB, 168 MB) and 16. By-key requests: at most about 50 a day for closed invoices (339
warehouse-open documents were closed in Visma between the first load of 2026-10-01/02 and the gate
of 2026-10-09, some with a moved timestamp that the hourly sync fetches anyway), about 2 % of the
2,880 requests the hourly tier makes at least, plus one for each part payment (not measured yet: the
first runs show it as `balance_differs`). A run that keeps failing repeats every 2 hours: up to 12
attempts a day, each a query plus the requests it got to.

**By hand.** No command of its own; it runs inside `vsv2 schedule` when due. To force it, delete the
cadence row and let the next hourly pass pick it up (`schedule --once` runs it only if the hourly
tier is due):

```sql
DELETE FROM vsv2.sync_checkpoint
WHERE partition_key = '_open_set' AND tenant_id = '<id>' AND endpoint = 'customer_invoices';
```
```bash
docker compose run --rm vsv2 schedule --once --tenant <id>
# the staging half alone, read only, one line per open document (key fields and `balance`,
# {"referenceNumber": "023672", "balance": "1450.5"}); --dry-run only asks BigQuery for the bytes:
docker compose run --rm --entrypoint vsv2-bq-open-keys vsv2 --project <project> \
  --dataset dw_staging_visma --table customer_invoices --tenant <id> --pk referenceNumber
```

**Result.** A row of `vsv2.sync_run` per run, endpoint `<endpoint>:open_set` (`records_in` = documents
that differed, of the three kinds, `records_written` = rows the write changed, `partial_success` for
partial and stalled, `error` = what went wrong), its list requests in `vsv2.sync_page_metric`, and in
the log an `open-set: compared` and an `open-set: finished` line with every count (`stale_in_staging`,
`missing_in_staging`, `balance_differs`):

```sql
SELECT tenant_id, endpoint, status, started_at, records_in, records_written, error
FROM vsv2.sync_run WHERE endpoint LIKE '%:open_set' ORDER BY started_at DESC LIMIT 20;
```

**After the first run, check two things.** First, the by-key answer replaces the staged document, so
it must carry what the list row carries. That was compared on one *open* invoice per endpoint, not
on a closed one, and `applications`, the section a payment fills, is non-empty on 98 % of closed
customer invoices and on 4 of 917 open ones. Look at what the run wrote (`last_run_id` = the
`run_id` in its log lines); if `with_applications` is 0 for the closed ones, by-key does not return
the expanded sections (the weekly full pass restores them):

```sql
SELECT JSON_VALUE(payload, '$.status') AS status, COUNT(*) AS docs,
       COUNTIF(ARRAY_LENGTH(JSON_QUERY_ARRAY(payload, '$.applications')) > 0) AS with_applications
FROM `<project>.dw_staging_visma.customer_invoices`
WHERE tenant_id = '<id>' AND last_run_id = '<run_id>' GROUP BY status;
```

Second, that part payments settle. A part-paid invoice is fetched because the `balance` of its list row
is not the staged one, and the by-key answer has to carry the list row's `balance`, or the invoice is
fetched again every day and the write changes nothing. After a run whose log line has
`balance_differs` above 0, force the next run (see By hand) and read its line: `balance_differs` should
be 0. A document that stays in it with `written=0` is one where by-key and the list disagree about
`balance`. Checked live on 2026-10-13 (Aquatiq Aps, three rows per endpoint, two GETs): Visma's open
list rows carry `balance` as a number, without `expand*` flags, for both `customerinvoice` and
`supplierInvoice`, so the list side is not missing it. If a run ever shows `balance_differs` close to
`staging_open` with `written=0`, that is the first thing to look at.

## 4c. Dress rehearsal (local, one tenant, a scratch dataset)

What it shows: the container image, the migration runner, the scheduler's three tiers, the staging
writer with its heartbeat and the open-set run work together against real Visma and real BigQuery,
without touching the live staging dataset or the live state database. Nothing here needs a server
or a go-live decision, and every request to Visma is a GET.

```bash
# 1. A scratch dataset where the warehouse lives, and an empty state database.
bq --project_id=aquatiq-datavarehus --location=europe-north2 mk --dataset \
  aquatiq-datavarehus:scratch_vsv2_rehearsal
docker compose up -d postgres redis
docker compose exec -T postgres psql -U visma_v2 -d postgres -c "CREATE DATABASE visma_v2_rehearsal"

# 2. One round, one tenant: every tier is due once. Own state, own staging dataset, a request
#    budget low enough to share the tenant's limit with whoever else is reading it.
VSV2_SCHEDULER_ERP_RPM=60 VSV2_SCHEDULER_ERP_RPD=86400 \
VSV2_SCHEDULER_DM_RPM=30 VSV2_SCHEDULER_DM_RPD=43200 \
VSV2_BIGQUERY_DATASET_STAGING=scratch_vsv2_rehearsal \
docker compose run --rm \
  -e VSV2_DATABASE_URL=postgres://visma_v2:visma_v2@postgres:5432/visma_v2_rehearsal vsv2 \
  schedule --config-dir config/endpoints --tenants-dir config/tenants --tenant <tenant-id> --once

# 3. What Visma holds against what landed (the scripts read the same two settings as the pipeline).
VSV2_BIGQUERY_DATASET_STAGING=scratch_vsv2_rehearsal \
  .venv/bin/python scripts/verify_endpoint_counts.py --tenants <tenant-id>

# 4. Clean up.
bq rm -r -f -d aquatiq-datavarehus:scratch_vsv2_rehearsal
docker compose exec -T postgres psql -U visma_v2 -d postgres -c "DROP DATABASE visma_v2_rehearsal"
```

Read the result in three places: the last log line (`scheduler: nothing more is due passes=3`), the
state database (`vsv2.scheduler_pass`: three rows, `failed` empty; `vsv2.sync_run`: the
`<endpoint>:open_set` rows of the two invoice endpoints), and the count report.

Done on 2026-10-13 with Aquatiq Aps, the smallest active tenant: 29 minutes for the three passes
(hourly 15 endpoints and 484 records, daily 117 and 9,799, full 135 and 14,710), 0 failed endpoints,
267 heartbeat rows, both open-set runs succeeded; the count report is
`gates/endpoint_counts_2026-10-13_rehearsal_aps.md` (64 of 64 exact). The first pass of a large
tenant takes longer in proportion: Aquatiq AS alone has 586,405 general-ledger lines, so plan for
hours, not minutes, and measure it with one tenant before the first all-tenant start.

## 4d. Monitoring

`vsv2 monitor` says whether the pipeline is healthy right now, so a failed or stalled sync is not
silent. `vsv2 healthcheck` only proves that Postgres and Redis answer; it passes with a scheduler
that is stuck. The monitor reads the state database (`vsv2.scheduler_pass` and `vsv2.sync_run`) and
the tenant list in `config/tenants`, prints a report, optionally posts a summary to a chat webhook,
and exits with a code a cron job, a systemd timer, a Cloud Scheduler job or a healthcheck can act on.

It needs only `VSV2_DATABASE_URL`: no Visma credentials, no BigQuery, no Redis. It writes nothing
(the session and the transaction are read-only, and the role needs only `SELECT` on the two tables).
Ages are measured against the database's clock, which also stamps the passes, so the host that runs
the monitor may have any clock.

### What it checks

Per tenant in `config/tenants`. A `historical` tenant only runs the weekly full pass, so for it only
`freshness_full`, `failures_full` and `stuck_pass` apply.

| check | PROBLEM when | WARNING when |
|---|---|---|
| `freshness_hourly`, `_daily`, `_full` | the newest `succeeded` pass of the tier started more than the limit ago (hourly 180 min, daily 30 h, full 8 d; a succeeded retry counts) | no pass of the tier has succeeded yet and its first pass is younger than the limit; older than that is a PROBLEM. A tier with no pass row at all is judged from the tenant's first pass of any tier |
| `stuck_pass` | a pass is `running` and its `heartbeat_at` is older than 10 min: the scheduler died mid-pass (also for tenants that are not in the list) | |
| `failures_hourly`, `_daily`, `_full` | an endpoint is in `failed_names` of the last 3 finished passes of the tier in a row | the newest finished pass failed (its endpoints are listed with how many passes in a row each has failed) |
| `open_set_customer_invoices`, `open_set_supplier_invoices` (active tenants) | the newest finished `<endpoint>:open_set` run in `vsv2.sync_run` is older than 36 h; 3 runs in a row failed; no run, and the tenant's first hourly pass is older than 24 h | the newest run failed (fewer than 3 in a row) or ended `partial_success`; no run yet and the first hourly pass is younger than 24 h |
| `tenant_has_run` | the tenant has no pass row at all (the scheduler reads the tenant list only when it starts) | |
| `scheduler_ran` | `scheduler_pass` is empty: the scheduler has not run yet. One finding, not one per tenant | |
| `state_database` | the database cannot be read (exit code 2, see below) | |

Details that matter when reading a finding:

* A pass that is still `running` is not a verdict; the checks use the newest *finished* pass. An
  abandoned pass (the scheduler ended without finishing it) neither extends nor breaks an endpoint's
  run of failures. If it is the newest pass it is a failure without an endpoint to blame, counted in
  passes in a row.
* The scheduler retries a failed pass for just the failed endpoints after 15 min (hourly), 2 h
  (daily) and 12 h (full), so "3 in a row" is the pass and its two retries: about 30 min of failing
  for the hourly tier, 4 h for the daily one, 24 h for the full one.
* The open-set run needs BigQuery staging. A deployment without it never has runs, and the monitor
  will call that a problem after the first day.
* A tenant that is in the database but not in `config/tenants` is not checked, except for a stuck pass.
* Every limit is a flag. The finding names the limit it was judged against.

### Flags

| flag | default | |
|---|---|---|
| `--format text\|json` | `text` | the report on stdout |
| `--tenants-dir DIR` | `config/tenants` | the tenants that must be healthy; an unreadable or empty directory is exit 2, not an all-clear |
| `--webhook-url URL` | env `VSV2_MONITOR_WEBHOOK_URL` | post a summary when there is a problem (below). The URL is never printed, and `--help` does not show the value of the variable. Prefer the variable: a flag shows in the process list |
| `--hourly-max-age-mins N` | 180 | 3 times the hourly interval |
| `--daily-max-age-hours N` | 30 | |
| `--full-max-age-days N` | 8 | |
| `--open-set-max-age-hours N` | 36 | |
| `--stuck-heartbeat-mins N` | 10 | the scheduler's own limit for abandoning a pass; it does so at its next poll, so a stuck pass that persists means the scheduler is not running |
| `--repeat-failures N` | 3 | failures in a row that make a problem (passes and open-set runs) |
| `--strict` | off | a warning also exits 1 |

### Exit codes

| code | meaning |
|---|---|
| 0 | everything is OK, or there are warnings only |
| 1 | at least one PROBLEM; with `--strict`, also a warning |
| 2 | the monitor could not evaluate: the state database cannot be read (not reachable, no `VSV2_DATABASE_URL`, no `vsv2` schema), or the tenant directory is unreadable or empty. It says so and guesses nothing. Wrong flags exit 2 as well |

### The report

Text (default): the verdict, the counts, every problem and warning with its detail, then the tenants
with nothing to report.

```
vsv2 monitor: PROBLEM at 2026-10-13T08:15:03Z (database clock)
1 problem, 0 warnings, 11 ok; 2 tenants checked

PROBLEM  Aqua Pharma AS (ab0e5d06)  [freshness_hourly]
         last succeeded hourly pass started 5 h ago (2026-10-13T03:15:03Z), limit 3 h; latest hourly pass: succeeded, started 5 h ago

OK: Consult AS
```

`--format json` prints `{"summary": {...}, "findings": [...]}` with stable field names:

```json
{
  "summary": {"status": "problem", "exit_code": 1, "problems": 1, "warnings": 0, "ok": 11,
              "tenants": 2, "evaluated_at": "2026-10-13T08:15:03Z", "error": null},
  "findings": [
    {"tenant_id": "ab0e5d06-fa25-11ed-a0e0-0693d8a7c3dd", "tenant_name": "Aqua Pharma AS",
     "check": "freshness_hourly", "severity": "problem",
     "detail": "last succeeded hourly pass started 5 h ago (...), limit 3 h; ...",
     "observed_at": "2026-10-13T03:15:03Z"}
  ]
}
```

`findings` holds every check that ran, OK ones included. `severity` is `ok`, `warning` or `problem`;
`summary.status` is `ok`, `warning`, `problem` or `unreadable` (then `error` says why, with the
database URL masked, and `findings` holds one `state_database` problem). `tenant_id` and
`tenant_name` are `null` for a finding about the pipeline as a whole. `observed_at` is when the pass or
run the finding rests on began, `null` when there is none (`tenant_has_run`, `scheduler_ran`).
`exit_code` follows `--strict`. The `detail` text is composed by the monitor from the rows: it never
contains error text from the database, which is why it is safe to forward.

### Running it

```bash
docker compose run --rm vsv2 monitor                       # in the container, on the mounted config/tenants
docker compose run --rm vsv2 monitor --format json --strict
cargo run -p vsv2-cli -- monitor                           # from a checkout, VSV2_DATABASE_URL in .env
```

Set the webhook once, in `.env` (compose passes it to the container):
`VSV2_MONITOR_WEBHOOK_URL=https://hooks.slack.com/services/...`. Run the monitor **every 15 to 30
minutes**. It takes a second or two, and never longer than about 70 seconds (a database that does
not answer, plus the webhook's 10 s).

A role that can do nothing but read is enough, if you want one for it:

```sql
CREATE ROLE vsv2_monitor LOGIN PASSWORD '<password>';
GRANT USAGE ON SCHEMA vsv2 TO vsv2_monitor;
GRANT SELECT ON vsv2.scheduler_pass, vsv2.sync_run TO vsv2_monitor;
```

**cron** (every 20 minutes; the first line keeps a log, the second sends mail only when the exit
code is not 0, with `MAILTO` set):

```
*/20 * * * * cd /opt/visma_service_v2 && docker compose run --rm -T vsv2 monitor >> /var/log/vsv2-monitor.log 2>&1
*/20 * * * * cd /opt/visma_service_v2 && out=$(docker compose run --rm -T vsv2 monitor 2>&1) || echo "$out"
```

**systemd timer** (the unit fails on exit 1 or 2, which `OnFailure=` can turn into a notification;
the timer keeps firing):

```ini
# /etc/systemd/system/vsv2-monitor.service
[Unit]
Description=visma_service_v2 pipeline monitor
Requires=docker.service
After=docker.service

[Service]
Type=oneshot
WorkingDirectory=/opt/visma_service_v2
ExecStart=/usr/bin/docker compose run --rm -T vsv2 monitor

# /etc/systemd/system/vsv2-monitor.timer
[Unit]
Description=Run the vsv2 monitor every 20 minutes

[Timer]
OnCalendar=*:0/20
RandomizedDelaySec=30

[Install]
WantedBy=timers.target
```

`systemctl enable --now vsv2-monitor.timer`. **Cloud Scheduler**: run the image as a Cloud Run job
with the argument `monitor` and `VSV2_DATABASE_URL` (and the webhook) as secrets, started every 20
minutes; an execution that exits 1 or 2 is a failed execution, which Cloud Monitoring can alert on.
**Compose healthcheck**: a container of its own can use `test: ["CMD", "/usr/local/bin/vsv2",
"monitor"]` with `interval: 20m` and `timeout: 90s`; unhealthy then shows in `docker ps` and does
nothing else.

### The webhook

When there is at least one PROBLEM, and only then (never for OK, and not for warnings, also not under
`--strict`), the monitor makes one HTTP POST of `{"text": "<summary>"}`, compatible with Slack and
Teams incoming webhooks: the counts on the first line, then one line per problem (the first 15; the
rest as a count). `&`, `<` and `>` are escaped, because Slack reads them as markup. An unreadable
state database is posted too, as one `state_database` problem, without the reason.

```
vsv2 monitor: 2 problems, 1 warning (12 tenants)
- Aqua Pharma AS: freshness_hourly: last succeeded hourly pass started 5 h ago (...), limit 3 h; ...
- Consult AS: tenant_has_run: never ran: no scheduler pass is recorded for this tenant (...)
```

The timeout is 10 s and redirects are not followed. A webhook that fails (no answer, an error status,
a URL that is not a URL) changes nothing but a line on stderr, such as `the alert was not delivered:
the webhook answered HTTP 500`, and never the exit code. That line, the report and every error carry
no part of the URL, whose path is the credential.

**The command keeps no state and there is no migration.** A problem that persists is reported, and
posted, by every run until it is fixed: at a 20-minute interval that is 3 posts an hour. Nothing is
posted when a problem clears, and there is no recovery message.

### What it does not cover

* BigQuery. The freshness of the staging tables (`_sync_heartbeat`) is gate-b's job
  (`scripts/run_gates.py`); the monitor reads no BigQuery. It sees the scheduler's records, not what
  landed.
* A scheduler loop that is stuck between passes. Nothing is written to the state while the loop only
  polls, so a loop that stopped starting passes shows only when a tier's freshness limit runs out
  (up to 3 hours for the hourly tier). The heartbeat of a running pass comes from a task of its own, so
  a sync that hangs inside a live process keeps beating and is not "stuck": it shows as a pass that
  never finishes, again by the freshness limit.
* Recovery. No message when a problem clears; run it with `--format json` and compare, if you need
  to know.
* The monitor itself. If cron, the timer or the job stops, nothing says so; use the failure
  notification of whatever runs it, or a dead-man's-switch service.
* What the data says: the gates (counts, journal balance, assertions), the Dataform runs, Power BI.
  An endpoint that succeeds with no rows is a success to the monitor.

### Reading a finding

| finding | usually means | do |
|---|---|---|
| `scheduler_ran` | the scheduler never started, or the monitor reads another database | `docker compose up -d vsv2`; compare `VSV2_DATABASE_URL` with the scheduler's |
| `tenant_has_run` | the tenant file was added after the scheduler started | restart `vsv2` (tenants are read once, at start) |
| `freshness_*` | the scheduler is down or its passes keep failing | `docker compose ps vsv2` and `logs`; the `failures_*` finding of the same tier names the endpoints |
| `stuck_pass` | the container died mid-pass and nothing has restarted it | start `vsv2`: it abandons the pass at its next poll and runs the tier again |
| `failures_*` | one endpoint fails pass after pass | the log line `scheduler: endpoint failed` for it; `vsv2 sync --tenant <id> --endpoint <name>` |
| `open_set_*` | the reconciliation of § 4b did not run or failed | the `vsv2.sync_run` query of § 4b; `error` of the failed run |
| `state_database` (exit 2) | Postgres is down, the URL is wrong, or `vsv2 migrate` has not run | `docker compose ps postgres`; `vsv2 migrate` |

Checked on 2026-10-13 against the state database of the dress rehearsal (§ 4c), which held one tenant from
one round. Three runs, each with the exit code it gave:

* all 12 tenants in `config/tenants`: exit 1, 12 problems, 8 ok. Eleven tenants never ran
  (`tenant_has_run`), and the rehearsal tenant's last hourly pass was 7 h old (`freshness_hourly`), which
  is what a single round looks like a few hours later;
* the rehearsal tenant alone, from a directory that holds only its file: exit 1, for the same stale hourly pass;
* a database that does not exist: exit 2, `UNREADABLE`, and the name of the database is masked in the message.

To repeat it, run the rehearsal of § 4c again (it creates the state database), then give the monitor a
tenant directory with the tenant you ran:

```bash
mkdir -p /tmp/one-tenant && cp config/tenants/aquatiq_aps.json /tmp/one-tenant/
VSV2_DATABASE_URL=postgres://visma_v2:visma_v2@127.0.0.1:55433/visma_v2_rehearsal \
  cargo run -p vsv2-cli -- monitor --tenants-dir /tmp/one-tenant
```

## 5. Operational Invariants

1. **Tier 1 ≠ DataMart** — hourly operational endpoints use ERP REST only.
2. **Soft-delete only** — never hard-delete BigQuery rows. Staging rows carry `deleted_at`; a complete scan sets it on what Visma stopped returning (never more than max(50, 20 %) of a tenant's rows at once) and a returning record clears it.
3. **Bronze lineage** — every row carries `tenant_id`, `_source`, and `_synced_at` (RFC-3339 UTC), stamped before any normalizer runs.
4. **Config-validated** — `make validate-config` must pass on every PR.
5. **Tenant-scoped** — all state and BQ writes are keyed by `tenant_id`; live MERGE uses `tenant_id` plus the endpoint primary key.
6. **Validated before MERGE** — rows missing tenant/key/validation fields or
   conflicting with another row in the same run are quarantined and the
   checkpoint is not advanced.
7. **Replay-safe** — incremental fetches use overlap windows, and checkpoints
   never move backward when overlap returns old or empty data.

## 6. DataMart Cutover (currently gated)

1. Verify DataMart authentication with `vsv2 dm-ping` and resolve the company ID
   with `vsv2 dm-companies`.
2. Run the separate discovery/shadow comparison for the exact tenant and
   endpoint. Resolve business-key, field, and completeness gaps before promotion.
3. Keep `datamart.ready=false` and tenant overrides disabled until that endpoint
   passes the cutover gate. A normal sync falls back to ERP when DataMart is
   requested but not ready; it does not perform a dual-source shadow run.
4. Do not set `datamart.ready=true` for a pilot tenant yet: the current master
   flag routes *all* tenants for that endpoint to DataMart. Per-tenant production
   promotion needs its own explicit routing gate before a pilot cutover.
5. See `docs/source_of_truth.md` § 8 and
   `docs/layeSystem/Layer1/layer1_datamart_audit.md` for the remaining checks.

## 7. Troubleshooting

| Symptom | Check |
|---|---|
| 401 from ERP REST | Verify `VISMA_CLIENT_ID` / `VISMA_CLIENT_SECRET` / `VISMA_OAUTH_SCOPE` |
| 401 from DataMart | `VISMA_OAUTH_SCOPE_DATAMART` must be provisioned in Visma Developer Portal |
| 401 from Expense | `VISMA_OAUTH_SCOPE_EXPENSE` must be set and provisioned |
| `validate-config` fails Invariant #4 | A Tier 1 endpoint has `datamart.ready: true` — remove it |
| Normalizer strips lineage | Python loader re-stamps automatically — verify with `pytest tooling/tests/test_bq_loader.py` |
| Missing DataMart company id | Run `vsv2 dm-companies` and set `VSV2_AQUATIQ_DATAMART_COMPANY_ID` |
| Rate limit 429 | Lower the relevant `VSV2_RATELIMIT_*` variable |
| BQ writes skipped | `VSV2_BIGQUERY_PROJECT` and `VSV2_BIGQUERY_DATASET_STAGING` (or `..._BRONZE`) must both be set |
| `vsv2-bq-stage` not found | The image was built without the Python stage; `docker compose build vsv2` |
| `delete_skipped=N` in a sync log | A complete scan would have marked more than 20 % of a tenant's rows deleted; look at why the fetch came back short before using `--force-delete` |
| Scheduler idle although tiers are due | A `running` row in `scheduler_pass` with a fresh `heartbeat_at` is a live pass; one silent for 10 minutes is abandoned at the next poll and runs again |
| Rows in quarantine | Inspect `<bronze>.__vsv2_quarantine`; fix the endpoint key/DTO/source data before replaying |
| Duplicate rows in a run | Exact duplicates are dropped; same key with different payload is quarantined and blocks checkpoint advancement |

## 8. Related Docs

- [DECISIONS.md](DECISIONS.md) — locked architectural choices
- [architecture.md](architecture.md) — component overview
- [source_of_truth.md](source_of_truth.md) — endpoint → source routing contract
- [layeSystem/LayerSystem.md](layeSystem/LayerSystem.md) — layer architecture
