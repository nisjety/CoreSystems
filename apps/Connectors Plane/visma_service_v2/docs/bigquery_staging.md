# BigQuery staging: `prj-dw-dev.dw_staging_visma`

The landing layer of v2. It keeps what Visma returned, without typing it. Dataform builds
`visma_bronze` (typed, unioned over ERP and DataMart) from it, and silver and gold after that.
The dataset is in `europe-north2`, like bronze, silver and gold, so Dataform reads it without
crossing regions.

## Tables

One table per endpoint config, named after the endpoint (`accounts`, `dm_accounts`,
`general_ledger_transactions`, ...), created on the first sync, including endpoints that return
nothing. Every table has the same columns:

| Column | |
|---|---|
| `tenant_id` | Visma tenant |
| `record_key` | the endpoint's key, tenant first, joined by `\x1f` (`REPLACE(record_key, CHR(31), '\|')` to read it) |
| `source` | `erp_rest`, `datamart_graphql` or `expense_api` |
| `payload` | JSON: the record as fetched, plus `tenant_id`, `_source`, and where they apply `_partition_key` and `_scan_period` |
| `payload_hash` | SHA-256 of the payload without its paging block (below) |
| `first_seen_at` | when the record first arrived |
| `updated_at` | when the payload last changed (not when it was last fetched) |
| `last_run_id` | the sync run that wrote the row last |
| `deleted_at` | when Visma stopped returning the record (see Deletes); cleared when it returns. Read live rows with `deleted_at IS NULL` |

Clustered by `tenant_id, record_key`. `_synced_at` and `_page_index` are not in the payload: they
change on every fetch and say nothing about the record.

The ERP list endpoints put `metadata: {"totalCount": N, "maxPageSize": M}` in every row (55 tables,
2.2 million rows). It describes the query that returned the row, not the record, and one new
invoice raises `totalCount` in the row of every other invoice. It stays in the payload as fetched,
but it is left out of `payload_hash`, so it never makes a row look changed and `updated_at` keeps
meaning "the record changed". The value in a row is the count at the time the row was last written:
do not read it as a current total. A `metadata` with any other key in it is treated as data. Rows
written before this rule have the old hash and are rewritten once, by the next full pass.

```sql
SELECT JSON_VALUE(payload, '$.accountCD') AS account,
       JSON_VALUE(payload, '$.description') AS description
FROM `prj-dw-dev.dw_staging_visma.accounts`
WHERE tenant_id = '33f319b9-4d57-11ee-960c-025417856183' AND deleted_at IS NULL
```

## Writing

`vsv2 sync` writes here when `VSV2_BIGQUERY_PROJECT` and `VSV2_BIGQUERY_DATASET_STAGING` are set
(`.env.example`). The Rust executor starts `vsv2-bq-stage` (`tooling/.../bigquery/staging.py`),
which streams the records in chunks of 20,000 into a temporary table with an explicit schema and
then runs one MERGE on `(tenant_id, record_key)`. The MERGE writes only new rows, rows whose hash
changed and rows coming back from deleted, so a full refresh of an unchanged endpoint rewrites
nothing (243,276 rows: 65 s the first time, 51 s and 0 rows written the second). The
Python process peaked at 0.4 GiB for those 243,276 rows: only keys and hashes are held, not rows.

Why not the typed MERGE of v1's bronze layout (`vsv2-bq-merge`, still in the code): it needs
every target table to exist, and it autodetects the schema of each batch, so a field that is empty
in one batch is dropped and the MERGE fails when a later batch has it. With 135 sparse, nested
endpoints that is the wrong place to type the data.

A checkpoint in the state database (`visma_v2`) is advanced only after the write succeeded, so the
state describes what is in BigQuery. That database had no checkpoints when the first load started.

```bash
scripts/tenant_sweep.py --db visma_v2 --sink bigquery --include-pilot --tenants <ids, heaviest first>
scripts/verify_bq_staging.py --db vsv2_alltenants_20260930
```

## First load (2026-10-01 to 02)

All 12 tenants, every endpoint, through real syncs (`tenant_sweep.py --sink bigquery`):

* 135 tables, 1,220 tenant/endpoint pairs with rows, **4,604,194 live rows**. No duplicate key, no
  null payload, every payload's `tenant_id` matches its column, every key starts with the tenant,
  no temporary table left over. `ModulePM` 1400592 of Sense is three rows (202310, 202311, 202312).
* `scripts/verify_bq_staging.py` (`docs/bq_staging_verification.md`) against the local copies of
  the same fetches: 1,082 pairs equal, 137 with more rows in BigQuery (new since the copies were
  made: GL balances gained period 202610, journals gained postings), 1 with fewer
  (`purchase_receipt_transfers` of the pilot: a short-lived list, Visma returned 007979 and no
  longer 007967 or 007973; the pilot's general ledger matches key for key, 922 keys more in
  BigQuery). Delete reconciliation, below, is what retires such rows.
* `scripts/verify_bq_vs_visma.py` (`docs/bq_vs_visma_spotcheck.md`): 345 rows across nine endpoints
  and all tenants, read from Visma by key now: 345 identical, 0 gone.
* The state database (`visma_v2`) holds a checkpoint for what was loaded and 1,621 succeeded runs.

What went wrong and what it changed:

* The laptop slept repeatedly overnight. Syncs stalled, tokens expired in flight (401), requests
  timed out, and a BigQuery job that had lost its connection waited for 5.9 hours. Now: a 401 is
  retried once with a new token for the same scope (`OAuthClient::forget_token`), the ERP request
  timeout is 180 s (was 60 s; `customer_invoices` pages take up to 99 s), BigQuery load and MERGE
  jobs time out (15 and 30 minutes) and the Rust writer kills the Python process after an hour.
  `tenant_sweep.py` measures its own deadline on the monotonic clock: three syncs that had
  finished were reported as timeouts because the wall clock jumped.
* The pilot's general ledger (575,000 lines) failed three times on HTTP 500 "Execution Timeout
  Expired" at deep pages of the `lastModifiedDateTime` walk, and loaded in a few minutes per
  period (see the endpoint's notes). The default config is unchanged.

## Deletes

ERP never says a record is gone; it just stops returning it. DataMart does: a deleted record
comes back once with `deletedDatabaseRecord: true`. Both end in `deleted_at`.

* **DataMart tombstone**: the row is written with `deleted_at` set when it arrives, kept with its
  last payload, and cleared if the record is later returned live. `scripts/bq_backfill_tombstones.py`
  set it once for the 44,455 tombstoned `dm_*` rows loaded before the rule existed (idempotent).
* **Complete scan** (ERP, and DataMart full pass): when a sync fetched the whole endpoint for a
  tenant, every live row of that tenant that was not in the fetch gets `deleted_at = now`. "Whole"
  is strict: no date window, no watermark, no cursor, at least one partition, and every partition
  fetched without a filter (`is_complete_scan` in `executor.rs`). An incremental run, a windowed
  run or one with a failed partition never deletes anything. `vsv2 sync --full` and the scheduler's
  weekly pass are the complete scans.
* **Guard**: a complete scan may mark at most max(50, 20 % of the tenant's live rows) deleted. More
  than that is read as a broken fetch, not as deletions: nothing is marked and the log says
  `delete_skipped=<n>`. `vsv2-bq-stage --force-delete` overrides it for a real mass deletion.
* The other tenants' rows in the same table are never touched.

`scripts/check_bq_staging_live.py` runs 18 cases of this against BigQuery on a throwaway table
(insert, change, no-op, a record that disappears and returns, the guard, an empty scan,
tombstones). Live example: `purchase_receipt_transfers` of the pilot, `sync --full`: 007973 is no
longer returned and is marked deleted, 007912, 007943 and 007945 stay live.

## Scheduler

`vsv2 schedule` runs every tenant in `config/tenants/*.json` (`--tenant` repeated narrows it;
`--once` does one round and exits). Three tiers, each a list of endpoints from the endpoint
configs:

| Tier | Every | Endpoints | Scope |
|---|---|---|---|
| hourly | `VSV2_SCHEDULE_TIER1_SECS` (3600) | `tier: 1` (15 transaction endpoints) | incremental from the checkpoint |
| daily | `VSV2_SCHEDULE_TIER2_SECS` (86400) | `tier: 2` and `3` (journals, masters) | incremental from the checkpoint |
| full | `VSV2_SCHEDULE_TIER4_SECS` (604800) | every enabled endpoint | complete scan: no checkpoint, deletes reconciled |

Historical tenants get the full pass only. What is due comes from `vsv2.scheduler_pass`
(migration 0011), not from timers in memory: a restart, a sleeping laptop or a second instance does
not shift or repeat the schedule. A pass that finished with failures is retried for just the failed
endpoints, after 15 minutes (hourly), 2 hours (daily) or 12 hours (full); the retry does not
restart the interval. A running pass writes a heartbeat every minute; one that has been silent for
10 minutes belongs to a process that died (a restart, a crash) and is marked failed and
`abandoned` at the next poll. An abandoned pass does not count as the tier's last complete pass, so
the tier runs again at once rather than after its interval, and an abandoned retry keeps the
endpoint names it was retrying.
Per tenant one endpoint syncs at a time (an in-process lock plus a Redis lease that lives 5
minutes and is renewed with every heartbeat, so a dead process frees its tenants within minutes); up to
`VSV2_SCHEDULE_PARALLEL_TENANTS` (4) tenants run side by side. `VSV2_SCHEDULE_POLL_SECS` (60) is how
often it looks for due work.

Endpoints with `open_set: true` (`customer_invoices`, `supplier_invoices`) also get a daily open-set reconciliation after their hourly sync: `vsv2-bq-open-keys` (`bigquery/open_keys.py`, read only) lists the keys and the staged `balance` of a tenant's live rows with `status = 'Open'`, they are compared with Visma's own `status=Open` list (keys, and the `balance` of the list row), and the differences are fetched by key and staged like any other record, because a payment closes an invoice, or lowers its balance, without moving its `lastModifiedDateTime` (`DEPLOYMENT.md` § 4b).

Journals and the general ledger are read by financial period (`period_scan`). With
`period_scan.resume` (journal and general-ledger transactions) the periods are always the whole
range and the checkpoint only filters on `lastModifiedDateTime` inside each period, so the
incremental pass is one cheap request per period (pilot: 94 periods, 1,277 changed records, 44 s,
where the single lastModified walk took 15 to 40 minutes and failed on HTTP 500). The full pass
ignores the checkpoint. GL balances and project transactions have no `resume`: carried-forward
balances change without their own timestamp moving, so every pass walks all periods.

## Deployment

`docker compose up -d vsv2` runs the scheduler for all tenants (`docker-compose.yml`). The image
(`Dockerfile`) is built with the Python tooling, and the build fails if any `vsv2-bq-*` command
the orchestrator spawns does not start. Google credentials are mounted read-only at
`/run/secrets/gcp.json` and never baked in: by default the gcloud ADC file of whoever runs compose;
on a server set `VSV2_GCP_CREDENTIALS_FILE` to a service-account key the container user (uid 1001)
can read. The project and dataset default to `prj-dw-dev` and `dw_staging_visma`
(`VSV2_BIGQUERY_PROJECT`, `VSV2_BIGQUERY_DATASET_STAGING`), and the local Postgres bronze sink is
off. Migration 0011 must be applied by hand to a state database created before it
(`psql -f migrations/0011_scheduler_pass.sql`); compose only runs the migrations when the volume is
first created. The `vsv2-http` service has the same image, credentials and settings. The
scheduler's ERP request budget is its own (240 per minute shared by all tenants, `docker-compose.yml`):
the 60 per minute and 5,000 per day in `.env` would stretch the first full pass over days.

## Moving to another GCP project

Everything that names the project is configuration, not code: `VSV2_BIGQUERY_PROJECT` (and
`VSV2_BIGQUERY_DATASET_STAGING`, `VSV2_BIGQUERY_LOCATION`) in `.env`. The scripts that read or write
v2 staging follow the same settings (`scripts/_bq_target.py`); the v1 comparison scripts
(`v1_v2_*`, `tenant_key_diff`, `tenant_scan_compare`, `verify_derived_price_classes`) keep reading
v1's tables, which stay in the old project.

The writer creates the dataset (in `VSV2_BIGQUERY_LOCATION`, default `europe-north2`) and the
tables on first use, so a new project needs only the BigQuery API and a principal that may create
datasets once. Order matters, because of the checkpoints in the state database (`visma_v2`):

1. In the new project: BigQuery API on; the account the pipeline runs as needs
   `roles/bigquery.dataEditor` and `roles/bigquery.jobUser` (and `roles/bigquery.dataOwner` or
   `roles/bigquery.admin` for the first dataset). A server uses a service-account key
   (`VSV2_GCP_CREDENTIALS_FILE`); a laptop uses `gcloud auth application-default login`.
2. Copy the data, `scripts/bq_copy_staging.py --to <new project>`. It copies every table within
   the region, checks row counts and live rows, and can be run again. Do not switch first: the
   checkpoints say everything is already in BigQuery, so a switch to an empty project would only
   ever receive new changes. If a copy is not wanted, empty `vsv2.sync_checkpoint` and run a full
   pass instead (hours of requests to Visma).
3. Set `VSV2_BIGQUERY_PROJECT=<new project>` in `.env`, then `docker compose up -d vsv2`.
4. `scripts/check_bq_staging_live.py` and `scripts/verify_bq_staging.py` now run against the new
   project.

The v1 Dataform repository (`Datawarehouse` in `prj-dw-dev`, `apps/visma_service/Dataform`) is not
changed by the move; it keeps reading v1's bronze tables. The new project has its own Dataform
repository, `Visma` (see `## Dataform` below).

## Dataform

Bronze, silver, gold and the datamart are built from these tables by the Dataform project in
`dataform/` (README there), deployed as the `Visma` repository in this project. Bronze reads the
`payload` column and gives back the columns of v1's bronze tables; the check against v1 is in
`dataform_validation.md`.

## Not built yet

* History of versions: a changed record replaces the previous one. The payload hash tells that it
  changed, not what it was.
* A scheduler on a machine that stays up. The state lives in Postgres, so a laptop that sleeps only
  delays passes, but a server (the VPS has not been touched) is where it should run.
