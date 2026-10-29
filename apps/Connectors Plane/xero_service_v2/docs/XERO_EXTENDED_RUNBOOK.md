# Extended Xero API ingestion

Status: implemented but **not deployed or verified against a live Xero tenant**.
The Accounting schedule remains unchanged. This runbook covers the opt-in
Assets, Projects, Bank Feeds, Identity, and Finance path added in September 2026.
It does **not** claim coverage of every GET operation in Xero's specifications.

## Contract and coverage

`xero sync-extended` makes read-only HTTP GET requests, lands each response body
unchanged in GCS (or a local dry-run directory), and writes an `x-*` JSON
sidecar and a per-run manifest. `etl/cloud_function/main.py` (or
`etl/backfill_gcs.py`) validates the sidecar/path pair, extracts records, and
MERGEs the **full** record into `extended_records.payload_json`. The merge key
is `(tenant_id, endpoint, record_id)`. Raw objects are the historical source;
BigQuery staging is latest-known state per key, not an append-only event table.
For complete collection GETs, the CLI also writes a membership object to the
family's `*_list_snapshots` endpoint, even when the response is empty. The
loader appends that evidence to `extended_list_snapshots` in the same security
domain. A page-cap stop, duplicate ID, failed request, or failed snapshot
write makes the entity fail; that request does not certify a complete list.
Manifest sidecars are loaded separately into `sync_runs` and
`sync_run_entities` in the same security domain as that run's raw bucket.
The warehouse path is `raw/xero/{tenant}/{version}/{endpoint}/{date}/{file}`;
`sync-extended` requires `GCS_PREFIX=raw/xero` because the loader scans that
layout. Each data file has a same-name `.meta.json` sidecar.
Finance records include their requested date/period in the key so snapshots for
different periods coexist. The initial Silver models cover only a subset of
these endpoints and are disabled until the corresponding Dataform variable is
enabled. No new Gold models are included.

| Layer | Grain | Completeness contract |
|---|---|---|
| Raw API page | GCS object path | Immutable response body plus sidecar; page metadata is not a deletion signal. |
| List evidence | `(tenant_id, snapshot_id)` | One append-only membership per successful list request, including empty `record_ids`. Source endpoint, request key, run ID and count must agree. |
| Extended staging | `(tenant_id, endpoint, record_id)` | Last loaded header/detail. Projects task/time IDs are prefixed by parent project UUID. Disappeared IDs remain here. |
| Current extended Silver | `(tenant_id, endpoint, record_id)` | Membership from the latest complete family run, joined to a header loaded from the same run. Failed outcomes, missing or duplicate list scopes, and missing Projects child lists exclude a run. |
| Finance staging | `(tenant_id, endpoint, period-or-request-key[:native-id])` | Dated request snapshot, not a full ledger or deletion-aware collection. Filter by period before aggregation. |
| Run history | `(run_id, outcome_index)` | Manifest-derived outcomes and counts; no payload or error text in BigQuery. |

Current Silver omits IDs absent from certified membership. Staging does not
delete them, so raw and staging can still be audited. Missing page loads cause
a count gap instead of silently filling from an older staging header. A
successful `--no-details` run can be complete for membership while still
lacking detail-only fields.

| Family | API base URL | Required OAuth permission | Implemented GETs (staging endpoint) |
|---|---|---|---|
| Assets | `https://api.xero.com/assets.xro/1.0` | `assets.read` | `/Assets?status=DRAFT/REGISTERED/DISPOSED` (`assets_assets`), `/Assets/{id}` (`assets_asset_details`), `/AssetTypes` (`assets_asset_types`), `/Settings` (`assets_settings`) |
| Projects | `https://api.xero.com/projects.xro/2.0` | `projects.read` | `/Projects` (`projects_projects`), `/Projects/{id}` (`projects_project_details`), `/ProjectsUsers` (`projects_users`), `/Projects/{id}/Tasks` (`projects_tasks`), `/Projects/{id}/Tasks/{taskId}` (`projects_task_details`), `/Projects/{id}/Time` (`projects_time`), `/Projects/{id}/Time/{timeEntryId}` (`projects_time_details`) |
| Bank Feeds | `https://api.xero.com/bankfeeds.xro/1.0` | `bankfeeds` | `/FeedConnections` (`bankfeeds_feed_connections`), `/FeedConnections/{id}` (`bankfeeds_feed_connection_details`), `/Statements` (`bankfeeds_statements`), `/Statements/{id}` (`bankfeeds_statement_details`) |
| Identity | `https://api.xero.com` | bearer token for `/connections` | `/connections` (`identity_connections`) |
| Finance | `https://api.xero.com/finance.xro/1.0` | `finance.cashvalidation.read`, `finance.statements.read`; `finance.bankstatementsplus.read` when requested | `/CashValidation` (`finance_cash_validation`), `/FinancialStatements/BalanceSheet` (`finance_balance_sheet`), `/FinancialStatements/Cashflow` (`finance_cashflow`), `/FinancialStatements/ProfitAndLoss` (`finance_profit_and_loss`), `/FinancialStatements/TrialBalance` (`finance_trial_balance`), `/FinancialStatements/contacts/revenue` (`finance_revenue_by_contact`), `/FinancialStatements/contacts/expense` (`finance_expense_by_contact`), `/BankStatementsPlus/statements` (`finance_bank_statements_plus`) |

Catalog source: `core/crates/xero-sync/src/extended.rs`; response shapes and
record IDs: `etl/xero/extended.py`. Compare changes against Xero's official
[Assets](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/master/xero_assets.yaml),
[Projects](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/master/xero-projects.yaml),
[Bank Feeds](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/master/xero_bankfeeds.yaml),
[Identity](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/master/xero-identity.yaml), and
[Finance](https://raw.githubusercontent.com/XeroAPI/Xero-OpenAPI/master/xero-finance.yaml)
OpenAPI files. Accounting remains covered separately by
[`OPERATIONS.md`](OPERATIONS.md). The `bankfeeds` scope is write-capable even
though this connector only performs GETs; Xero restricts Bank Feeds access.
Finance requires Xero partner access. These are entitlement gates, not
something a code change can bypass.

## Security and storage

- Assets/Projects: ordinary raw bucket -> `staging_xero.extended_records` and
  `staging_xero.extended_list_snapshots` -> `dw_1_silver_xero` models with
  `enable_xero_extended=true`.
- Bank Feeds/Finance/Identity: a **separate restricted bucket** ->
  `staging_xero_sensitive.extended_records` and
  `staging_xero_sensitive.extended_list_snapshots` ->
  `dw_1_silver_xero_sensitive` models with `enable_xero_sensitive=true`.
  Set `GCS_BUCKET` and `XERO_SENSITIVE_GCS_BUCKET` to the same restricted bucket
  for those CLI runs. Set `GCP_PROJECT` and
  `XERO_SENSITIVE_DATASET=staging_xero_sensitive` on its loader. The CLI and
  ETL both reject a mismatched sensitive target. The CLI also rejects a command
  that mixes ordinary and sensitive families; use separate invocations.
- Give the sensitive sync identity write access only to its bucket. Give the
  sensitive loader read access only to that bucket and BigQuery write access
  only to its restricted staging dataset. Grant Dataform access to the
  restricted staging/Silver datasets only in its restricted workflow. Review
  inherited project-level IAM before treating either dataset as private.
- The Cloud Function is configured for **one** `GCS_BUCKET`; provision a
  separate instance and trigger/topic for the restricted bucket. Confirm the
  event's bucket name is checked at runtime. Keep raw object retention,
  encryption, audit logging and deletion policy consistent with finance data.
- PKCE `/connections` is app-wide and stored under tenant `_application`;
  omit `--tenant` for Identity in PKCE mode. Custom Connections run per
  configured tenant. Do not join app-wide connection records to an org as if
  they belonged to one tenant.
- Dry-run files contain real data. On Unix the sink writes files `0600` and
  containing directories `0700`; put `--local-dir` on a restricted volume and
  remove it according to your retention policy after review. Do not attach
  payloads to tickets or paste them into logs.

## Rollout gates

1. **Access approval:** enable only the required scopes in the Xero app.
   Custom Connection scopes are granted in the Developer Portal; PKCE tenants
   must reauthorize after scope changes. Verify Finance partner and Bank Feeds
   approval separately. Start with one non-production tenant and one family.
2. **Provisioning:** create both datasets in the intended project with the
   correct location and restricted IAM; provision the restricted bucket and
   its separate notification/loader. Deploy the updated loader **before**
   writing any extended sidecar. Set explicit `GCP_PROJECT`; do not rely on
   the Accounting loader's defaults.
3. **Offline probe:** run one family into an access-controlled local directory,
   then validate the raw/sidecar contract. The validator reports only counts
   and reasons, never payloads or native record IDs:

   ```bash
   xero sync-extended --family assets --tenant <tenant-uuid> \
     --dry-run --local-dir <restricted-local-dir>
   .venv/bin/python -m etl.validate_extended <restricted-local-dir>
   ```

   A nonzero CLI exit or any validator error stops rollout. Inspect the
   manifest for failed or truncated requests. `--no-details` is a cheaper
   smoke test but is **not** a complete data-quality test.
4. **One live tenant:** point the sync job at the correct bucket, run only
   Assets/Projects first, then check object/sidecar pairs, Cloud Function
   success logs and staging counts. Run sensitive families separately after
   IAM review. Finance requires both `--from` and `--to`; add
   `--bank-account-id <uuid>` only when BankStatementsPlus permission is
   granted. Run Identity without `--tenant` for PKCE.
5. **Backfill and model:** if the loader was deployed after raw objects were
   written, replay **one endpoint at a time** with
   `python3 -m etl.backfill_gcs <endpoint>` and the appropriate `GCS_BUCKET`.
   Run it once per bucket; the script scans only its configured bucket.
   Replay `assets_list_snapshots`, `projects_list_snapshots`,
   `bankfeeds_list_snapshots`, and `identity_list_snapshots` in their respective
   buckets as well as the normal response endpoints. Replay manifests with
   `python3 -m etl.backfill_manifests`. Older raw pages **without** list
   snapshots cannot establish deletion-aware completeness; start a fresh sync.
   Older Projects task/time pages without `x-request-key` cannot be assigned a
   safe project-scoped ID; do not backfill them into the new staging grain.
   Enable Assets/Projects Silver only after staging and membership are loaded.
   Enable sensitive models only in the restricted Dataform workflow.
   Run `dataform compile --quiet --vars=enable_xero_extended=true` (or
   `enable_xero_sensitive=true`) from the `Dataform/` directory; compilation
   is not a BigQuery execution.
   
   **Projects quota policy:** the Projects fan-out makes one Tasks and one Time
   collection request per project, plus optional project/task/time detail
   requests. Start a new tenant with `xero sync-extended --family projects
   --no-details` to establish a complete, certifiable collection baseline.
   Hydrate details in a separate quota window and keep them as enrichment; do
   not let a detail-heavy run prevent the baseline membership snapshot from
   completing. A manifest containing any failed or truncated outcome is never
   eligible to establish current Projects state or Gold facts.
6. **Schedule last:** after several successful comparisons, add explicit
   per-family jobs with bounded cadence and alert on nonzero exits, manifest
   errors, page-cap termination, missing sidecars, loader failures and stale
   `synced_at`. Do not place the sensitive jobs in the ordinary Accounting
   crontab or bucket. Leave all new jobs opt-in until entitlements and data
   reconciliation are confirmed.

## Nightly evidence and exceptions

The read-only reporter builds a tenant/endpoint matrix from manifest outcomes,
staging key counts, list membership, and selected Silver header/current counts.
Collection checks are **exact** only for the implemented complete-list
endpoints; Accounting's incremental/windowed raw counts and Finance period
snapshots are **advisory**. The report never emits payloads or native IDs.
`no_data` means a successful zero-record/no-op request (for complete
collections, every expected list snapshot must also be present).
`pending_current_model` means Dataform has not caught up or a load is missing;
`exact_gap` is a same-run count mismatch. `failed_run`, `missing_run`, `stale`,
`duplicate_keys`, and `pending_staging` are exceptions. Empty current
collections may have no Silver row; membership evidence distinguishes them
from an incomplete load.

After the appropriate loader and Dataform actions finish, schedule one report
per security domain. Configure `--expect` for tenants and endpoints that must
run; otherwise an endpoint that has **never** run cannot be reported missing.
Use a freshness threshold appropriate to each schedule (weekly endpoints need
a separate invocation from daily endpoints):

```bash
python3 -m etl.xero.quality_report --project <project-id> \
  --dataset staging_xero --max-age-hours 36 \
  --expect <tenant-uuid>:assets_assets \
  --expect <tenant-uuid>:projects_projects --fail-on-exceptions
python3 -m etl.xero.quality_report --project <project-id> \
  --dataset staging_xero_sensitive --max-age-hours 36 \
  --expect <tenant-uuid>:bankfeeds_statements --fail-on-exceptions
```

Exit code `2` indicates exceptions; send the JSON to restricted monitoring
and alert on that exit. Run the sensitive invocation under the restricted
BigQuery identity and do not copy its output into ordinary logs. This is a
read-only report, not a scheduler or automatic repair. A passing report does
not establish field-level Finance accuracy or live Xero API parity.

For a basic staging check, substitute the approved project ID and run in the
appropriate dataset. An empty result after a nonempty raw pull is a loader
failure, not evidence that Xero returned no data.

```sql
SELECT tenant_id, endpoint, COUNT(*) AS records, MAX(synced_at) AS latest_sync
FROM `<project>.staging_xero.extended_records`
GROUP BY tenant_id, endpoint
ORDER BY tenant_id, endpoint;
```

For sensitive data, query only from an authorized account and substitute
`staging_xero_sensitive`. The merge key should be unique:

```sql
SELECT tenant_id, endpoint, record_id, COUNT(*) AS duplicates
FROM `<project>.staging_xero.extended_records`
GROUP BY tenant_id, endpoint, record_id
HAVING COUNT(*) > 1;
```

## Current limitations and next engineering work

1. The new families are full collection pulls, with optional detail fan-out;
   they do not yet have incremental cursors. Disappeared IDs remain in staging
   history, but complete-list current Silver filters them out. The current
   models have not yet been exercised against a live warehouse.
2. Only five initial Silver models exist (fixed asset, project, project time
   entry, bankfeed statement, finance snapshot). The remaining response types
   are available in raw/staging JSON but not typed warehouse tables. Identity
   has no Silver model. Build these from **observed** tenant payloads, with
   field-level schema and reconciliation tests, before exposing them to BI.
3. Finance is period-snapshot based, not an automatic historical backfill.
   Define approved daily/monthly periods and retain the request key; rerun
   periods when Xero restates figures. BankStatementsPlus requires explicit
   account UUIDs. Do not interpret one snapshot as an all-time ledger.
4. No live Xero entitlement, tenant response, Cloud Function deployment or
   BigQuery load was exercised by the code tests. The first live rollout must
   validate response shapes, pagination, quota behavior, warehouse counts and
   restricted IAM before routine scheduling.

Rollback: stop the new family schedule and disable the two Dataform variables.
Leave Accounting running. Do not delete raw objects or staging tables as an
automatic rollback; preserve them for diagnosis under the correct access
controls. If a sensitive object reached the wrong bucket or dataset, treat it
as an access-control incident and follow the organization's data-handling
procedure.
