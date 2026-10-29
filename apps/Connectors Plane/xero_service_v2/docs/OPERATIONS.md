# Operations

The sync CLI is a **one-shot raw extraction job**: each invocation fetches Xero
pages, writes them verbatim to GCS, then exits. It does not own the downstream
warehouse or a sync checkpoint database. PKCE credential storage and the
optional shared Redis quota coordinator are separate from raw landing.
For the active main baseline, operating it means using the pinned
`aquatiq-datavarehus` environment, reconciling each source run before its
Dataform transformation, and running the isolated `Xero` Dataform repository.
The BFF and MCP service are separate long-running processes. The active main
baseline is deliberately **not scheduled** and has no historical backfill yet.

## AI/MCP service

`xero-mcp` is a separate long-running service, not a new CLI sync mode or an
LLM host. It shares Xero PKCE/Custom Connection authentication and the raw
sync library. It supports stdio and bearer-protected Streamable HTTP on
`127.0.0.1:5003/mcp`, with extraction and sensitive operations disabled by
default. See [XERO_MCP.md](XERO_MCP.md) for deployment, tool/resource/prompt
contracts, bounded results, and private raw extraction. CoreSystem integration
is a separate adapter task described in
[XERO_MODEL_PLANE_MAPPING.md](XERO_MODEL_PLANE_MAPPING.md).

## Environment variables

| Var                                 | Required | Default            | Notes                                                |
|-------------------------------------|----------|--------------------|------------------------------------------------------|
| `XERO_AUTH_MODE`                     | no       | `custom`           | `custom` for client credentials or `pkce` for stored onboarding connections |
| `XERO_CONNECTION_TYPE`              | no       | `custom`           | Validated only in custom mode; must be `custom` |
| `XERO_ORG_N_CLIENT_ID` / `_CLIENT_SECRET` / `_TENANT_ID` / `_NAME` | yes (per org) | — | numbered custom-connection blocks, N = 1, 2, … |
| `XERO_CC_CLIENT_ID` / `_CLIENT_SECRET` / `_TENANT_ID` / `_TENANT_NAME` | alt | — | single-org form (instead of `XERO_ORG_1_*`) |
| `XERO_CUSTOM_CONNECTION_ORG_INDEX`  | no       | `1`                | which numbered org is "primary"                      |
| `XERO_PKCE_SCOPES`                  | no       | BFF read-only set  | PKCE onboarding scopes; includes `accounting.budgets.read` |
| `XERO_CONNECTION_STORE_BACKEND`     | PKCE     | `file`             | Set `gcs` in production on both BFF and CLI; `file` is local-only |
| `XERO_CONNECTION_STORE_GCS_BUCKET`  | GCS token store | — | Dedicated private bucket, not either raw-data bucket |
| `XERO_CONNECTION_STORE_GCS_OBJECT`  | no       | `xero/pkce/connections.json` | Same object path on BFF and CLI |
| `XERO_CONNECTION_STORE_KMS_KEY`     | optional | bucket default | Explicit Cloud KMS key for credential object writes |
| `GCS_BUCKET`                        | yes¹     | —                  | destination bucket (per env: `xero-raw-dev` / `xero-raw-prod`) |
| `XERO_STAGING_DATASET`               | no       | `dw_staging_xero` | Ordinary Xero BigQuery staging target; valid dataset identifier required |
| `GCS_PREFIX`                        | no       | `raw/xero`         | object key prefix; must not start with `_manifests`  |
| `XERO_SENSITIVE_GCS_BUCKET`         | sensitive APIs | — | Dedicated restricted bucket; must match `GCS_BUCKET` for Bank Feeds, Finance or Identity |
| `GCP_PROJECT`                       | extended ETL | — | Explicit BigQuery project for extended API payloads |
| `XERO_SENSITIVE_DATASET`            | sensitive ETL | — | Restricted staging dataset, separate from `staging_xero` |
| `XERO_DOCUMENT_GCS_BUCKET`          | binary Accounting GET | — | Separate restricted PDF/attachment bucket; differs from `GCS_BUCKET` |
| `XERO_DOCUMENT_DATASET`             | document ETL | — | Must be `staging_xero_documents` for document-bucket loader |
| `GOOGLE_APPLICATION_CREDENTIALS`    | yes¹     | —                  | path to the GCS service-account JSON                 |
| `SYNC_WINDOW_DAYS`                  | no       | `3`                | incremental modified-window look-back                |
| `SYNC_ROLLING_FULL_TIGHT_DAYS`      | no       | `30`               | `--full-with-window` (weekly) window size            |
| `SYNC_ROLLING_FULL_WIDE_DAYS`       | no       | `90`               | rolling-full (wide, monthly) window size             |
| `SYNC_MAX_CONCURRENT`               | no       | `6`                | max concurrent (tenant, entity) tasks; clamped to 1..=6 |
| `XERO_DETAIL_SPOOL_MAX_BYTES`        | no       | `536870912`        | Positive byte budget for collection + detail staging per expanded entity |
| `XERO_DETAIL_SPOOL_DIR`              | no       | OS temp directory | Existing restricted, encrypted volume for temporary raw financial data |
| `XERO_REDIS_URL`                     | shared quota | unset          | Opt-in Redis quota backend; non-loopback TCP requires verified `rediss://` TLS |
| `XERO_REDIS_APP_KEY`                 | with Redis | unset            | Same non-secret app cohort on every worker sharing the Xero app and Redis database; ASCII letters/digits/underscore/hyphen, max 80 |
| `RUST_LOG`                          | no       | `info,xero_cli=info,…` | tracing-subscriber filter                        |

¹ `GCS_BUCKET` and `GOOGLE_APPLICATION_CREDENTIALS` are required for a real
upload. A `--dry-run` writes to local disk and needs neither.

## GCP Xero staging

### Active project baseline

The active Xero warehouse is `aquatiq-datavarehus` in `europe-north2`. Tenant
ID is the Xero organization/company key; it is never a Visma-style business
unit. The raw writer target is `dw_staging_xero`, with Bronze in
`dw_bronze_xero`, Silver in `dw_1_silver_xero`, and tenant-scoped Gold marts in
the `xero_gold_*` datasets.

The active raw generation is
`xero/a3e6e7e5-04c1-4574-a63f-e0863ea9cef1/`, activated at
`2026-10-15T23:19:31Z`, in `gs://aquatiq-datavarehus-xero-raw`. Cloud Storage
notifications publish to `xero-raw-finalized`; the private Gen 2
`xero-staging-loader` function uses `process_fresh_gcs_upload` with
`XERO_INGESTION_NAMESPACE=main` and `XERO_STAGING_LAYOUT=warehouse`.

The raw notification is infrastructure, not an ad-hoc console setting. Before
the first source run after a bucket or topic change, use the source-controlled
provisioner. It grants only the Cloud Storage service agent publisher access
and maintains exactly one unfiltered `OBJECT_FINALIZE` JSON notification for
the main raw bucket; it never modifies other topics or projects.

```bash
bash deploy/provision-xero-raw-notification --preview .env.main
bash deploy/provision-xero-raw-notification --apply .env.main
```

Every active-generation sidecar is validated and committed to immutable raw
receipt, record, collection, and child history before the compatible typed
current projection is updated. Events from other generations or buckets are
rejected before a BigQuery write. The raw history is never overwritten or
deleted. Child lifecycle is classified as current, superseded, removed from a
complete later parent, source-deleted, or unverified; positional child keys are
never called deleted from absence.

Use the private `.env.main` configuration for local validation. It must name
only `aquatiq-datavarehus`, `aquatiq-datavarehus-xero-raw`,
`dw_staging_xero`, and the active generation. A source run is complete only
after its manifest, tenant/endpoint outcomes, receipts, raw records, typed
projection, and relevant Dataform assertions reconcile. Payment Services is
intentionally out of warehouse scope. There is no production schedule or full
historical backfill yet.

### Contact-scoped aged reports

Run Xero aged receivables and payables as a bounded contact fan-out, never as a
generic report entity. The command first lands the complete Contacts response
for each tenant, then requests one AR and one AP report for every discovered
contact. It refuses to fan out when discovery is partial or above the configured
limit.

```bash
bash deploy/xero-fresh sync-aged-contact-reports \
  --tenant TENANT_UUID \
  --max-contacts-per-tenant 1000 \
  --max-report-requests 2000
```

`sync_run_entities.request_key` records the `contact:<uuid>` request identity.
Dataform marks a report run complete only when contact discovery, successful
keyed requests, and keyed report snapshots agree exactly. Do not sum values
across source headers or turn an unlabelled `Due` value into an assumed
currency. A header such as `Due USD` is the only currency proof for that
report measure.

The validated standard Accounting run
`cffa373d-1cdf-44dc-b2bb-f1a475a0f8e9` has 189 complete tenant/endpoint
outcomes, 276 received pages, and 4,604 received raw records. The full `Xero`
Dataform graph succeeded with 353 actions after that delivery. This is a
baseline operational proof, not a promise that optional or data-dependent API
routes have been extracted.

Run transformations only through the isolated Xero runner:

```bash
python3 Dataform/xero_warehouse/run_workflow.py --full --wait
```

It pins the GCP project, region, repository and workspace, and rejects
foreign-project action targets. Do not use the shared Dataform workflow helper:
its legacy defaults point to the mixed development warehouse.

## Archived development guidance (do not execute)

Everything below this heading preserves the earlier `prj-dw-dev` and
endpoint-layout investigation for traceability. It is **not** an active
configuration for `aquatiq-datavarehus`, including any references to
`dw_staging_xero_main`, `.env.current`, `.env.fresh`, `prj-dw-dev` buckets,
the shared Dataform helper, cron installation, or legacy container image paths.
Do not use it to schedule, deploy, backfill, or redirect the active Xero
pipeline. The active contracts are the section above,
[`XERO_GCP_MAIN_MIGRATION.md`](XERO_GCP_MAIN_MIGRATION.md), and
[`Dataform/xero_warehouse/README.md`](../Dataform/xero_warehouse/README.md).

### Historical endpoint-layout baseline (inactive)

The new main-line Xero pipeline is still being built in `prj-dw-dev`. On
2026-10-09, the older `prj-dw-dev.dw_staging_xero` dataset was copied to
`aquatiq-datavarehus.dw_staging_xero` as a development snapshot: 51 tables,
3 views, and 477,201 rows at verification, with no destination labels. The
copied views reference the new project. This did not redirect any writer or
move the new endpoint-layout pipeline. A separate typed-source Xero Dataform
repository and first bronze/silver models now run in `aquatiq-datavarehus`;
see [XERO_GCP_MAIN_MIGRATION.md](XERO_GCP_MAIN_MIGRATION.md) for scope and
remaining live-cutover gates. Existing `dw_staging_xero` natural-name
tables still receive the older pipeline's data, so the new endpoint baseline
uses separate datasets instead of overwriting or importing those rows:

| Dataset | Purpose |
| --- | --- |
| `dw_staging_xero_main` | Raw endpoint observations and receipt/run metadata |
| `dw_bronze_xero` | Protected, insert-only Dataform endpoint tables |
| `dw_bronze_xero_assertions` | Xero-only grain assertions |

All three datasets are in `europe-north2`. Their tables/views have no labels.
The staging dataset has 35 endpoint tables named for the API endpoints, plus
`accounting_get_responses` and six receipt, child, and run-control tables.
Dataform `main` has 35 `xero_<endpoint>` bronze actions and 35 assertions;
Payment Services and the Bills alias are excluded. The bronze actions read only
the corresponding new staging tables, preserve raw JSON and lineage, and never
full-refresh. They do not represent a silver model or a complete 150-route
warehouse. Xero tenant ID, not a Visma-style BU, is the organisation key.

The active raw prefix is
`xero/abf15bca-8a62-4e2b-b650-b2a6cdc19196/`, with activation time
`2026-10-06T22:09:02Z`. Storage notification 3 publishes that prefix to
`xero-main-raw-finalize`; the dedicated `xero-main-to-staging` Gen2 function
uses `process_fresh_gcs_upload`, `XERO_INGESTION_NAMESPACE=main`,
`XERO_STAGING_LAYOUT=endpoint`, and `XERO_STAGING_DATASET=dw_staging_xero_main`.
The prior raw notifications and loaders remain intact. Events outside this
generation cannot write to the new staging dataset.

Provisioning and Dataform compilation/invocations have succeeded, but the new
staging and bronze business tables are still **empty**. The staging `control`
table has one operational fence row; it is not Xero data. Do not interpret the
older `dw_staging_xero` counts as current-baseline data. A real fresh Xero fetch
is pending PKCE client-secret rotation and private credential update. After
rotation, create a private `.env.current` from `.env.current.example`, build the
current CLI, and run a bounded customer Payments fetch first:

```bash
cargo build -p xero-cli
XERO_FRESH_ENV=.env.current bash deploy/xero-fresh sync --no-window --entity payments --max-concurrent 1
```

Check the raw sidecar and run manifest under the active prefix, the new
function's delivery logs, the matching `ingest_receipts` and `sync_run_entities`
in `dw_staging_xero_main`, and row counts in `payments` and
`dw_bronze_xero.xero_payments`. Run the matching protected Dataform incremental
action after staging delivery, then its grain assertion. Expand to the other
Accounting endpoints only after these checks pass. A zero count is valid only
with a completed manifest and matching receipt, never from an empty table alone.
No old Xero data is copied into this pipeline; historical observations are
retained and later lifecycle projections should mark superseded/deleted state
without replacing or deleting raw records.

### Previous Xero staging

The old `v2` child/collection tables and views, and their dependent quality
view, were removed after confirming zero business rows. Notification
configuration 2 was disabled, so this retired generation no longer delivers
new objects. Other old `v2_*` objects remain for now; they are not part of the
current pipeline. `etl.bootstrap_fresh` now defaults to the new endpoint layout
and `dw_staging_xero_main`, preventing an ordinary bootstrap from recreating
the retired objects. The remainder of this subsection is historical context.

The older development warehouse is `prj-dw-dev` in `europe-north2`. Ordinary
Xero raw payloads land in `gs://prj-dw-dev-raw`, and the event-driven
`xero-raw-to-staging` Cloud Function writes typed rows to
`prj-dw-dev.dw_staging_xero`. This is the dedicated ordinary Xero staging
dataset; do not use the older numbered staging datasets for new writes.

Dataset metadata follows the shared integration convention:

| Dataset | Platform | Class | Location | Labels |
| --- | --- | --- | --- | --- |
| `dw_staging_xero` | BigQuery | Default | `europe-north2` | `source=xero_v2`, `layer=staging` |

Existing unprefixed tables are historical and are not the fresh v2 baseline.
The October 6 baseline uses only the isolated `v2_` staging contract below.
Do not import older tables/raw objects, repair legacy rows, or activate ODS or
Silver as part of this baseline. Restricted Bank Feeds, Finance, Identity, and
document data remains outside the ordinary dataset.

### Fresh v2 baseline

Use `.env.fresh.example` as the configuration contract. The private local
`.env.fresh` selects generation `ba1eb730-e742-4dfc-bfd9-edbb7bddcf91`, activated
at `2026-10-06T13:41:24Z`. A generation is a baseline, not a per-run identifier.
The Rust generation sink preserves response bytes and writes only below
`v2/<generation>/raw/xero/` and `v2/<generation>/_manifests/xero/`. The old
`raw/xero/` notification cannot see these objects.

`xero-v2-fresh-finalize` previously received notification configuration 2 for
this whole generation prefix, including manifests; that notification has been
removed. The historical dedicated function entrypoint is
`process_fresh_gcs_upload`; never deploy fresh ingestion with the legacy entrypoint.
It requires `XERO_INGESTION_GENERATION`, `XERO_FRESH_START_AT`, `GCP_PROJECT`,
`GCS_BUCKET`, and `XERO_STAGING_DATASET`. Missing/invalid baseline settings fail
closed. Mismatched namespaces/generations and pre-activation observations never
write either fresh or legacy rows.

The reader pins the event's sidecar generation and the sidecar's data-object
generation and CRC32C. Payloads, child positions, and an ingest receipt publish
in one BigQuery transaction, serialized by an operational-only control fence.
Retransmitted events cannot duplicate a committed observation. Business history
has no UPDATE, DELETE, replacement, or expiration.

| Staging object | Role |
| --- | --- |
| `v2_record_versions` | Every fresh source-record occurrence with complete JSON payload, source revision, and raw provenance. |
| `v2_collection_versions` | Explicit child-array observations, including `child_count=0` for `[]`. Missing/null arrays are unknown, not empty. |
| `v2_child_versions` | Every original array position, including blanks and duplicate/missing native IDs. |
| `v2_ingest_receipts` | Pinned source objects and expected/observed counts, including successful empty responses and binary pointers. |
| `v2_sync_runs`, `v2_sync_run_entities` | Immutable manifests and per-tenant endpoint outcomes. |
| `endpoint_catalog`, `endpoint_coverage` | 150-operation inventory and conservative evidence, not a claim that all were extracted. |
| `v2_quality_issues` | Duplicate identities, orphan children, raw-array count mismatches, conflicting revisions, and receipt-count mismatches. |

`v2_records`, `v2_collections`, and `v2_children` expose lifecycle markers without
changing stored payloads. `is_superseded` marks older observations; `source_deleted`
requires an endpoint-specific documented deletion status. Child removal from a
complete parent array is separately marked `is_removed` with
`deletion_reason=missing_from_complete_parent`, not asserted as source deletion.
Filtered/incremental collection absence never marks a parent deleted. Older
arrivals cannot reactivate newer records. Conflicting equal-order payloads are
excluded from current projections rather than arbitrarily promoted.

Do not run the retired baseline's bootstrap or `.env.fresh` sync commands. For
the current endpoint layout, use `.env.current.example` and the procedure in
"Current-project Xero baseline" above. Do not report aggregate
unprefixed warehouse counts as the fresh v2 baseline. JSON staging retains raw
fields; typed dimensional/fact transformations are deliberately out of scope.

Coverage verification requires a matching generation/run/tenant manifest, all
pages, declared termination, and matching source/staging counts. A direct GET
response is only `response_verified`, not exhaustive parent fan-out. Unknown
termination is incomplete; missing evidence is not an empty dataset. Verification
remains blocked when quality issues exist, a scope/entitlement is unavailable,
or the endpoint has not been freshly attempted.

The fresh loader is deployed as `xero-v2-fresh-to-staging` in
`europe-north1`, revision `xero-v2-fresh-to-staging-00001-zoy`, with retries
enabled, two maximum instances, and concurrency one. Its pinned deployed source
matches the local transformation, ingestion, writer, and handler modules. On
October 6 at 17:25 UTC, an old-namespace Pub/Sub probe returned HTTP 200 and
logged `Skipped non-baseline input; no v2 or legacy writes`. This checks the
isolation gate, not delivery of an actual Xero response.

Credentialed fresh extraction is paused until the exposed PKCE client secret
is rotated and the private configuration updated. Customer Payments read access
and Payment Services are separate: the latter's `paymentservices` scope was
rejected by authorization for this app. Do not treat that rejection as a failure
of the customer Payments endpoint or try to repair it through token refresh.

Offline tests block ADC, real Google clients, and Google API networking before
collection. Legacy live tests require explicit `XERO_RUN_LIVE_ETL_TESTS=1` and
an explicit `XERO_ETL_TEST_PROJECT` / `XERO_ETL_TEST_DATASET=xero_test_<name>`;
business datasets are rejected. Do not use the legacy live tests to certify v2.

The following legacy loader details are retained only as operational history:

The function is Pub/Sub-triggered through `xero-raw-finalize` because the raw
bucket is in `europe-north2` and the function runs in `europe-north1`. Its
runtime environment must explicitly set:

```text
GCP_PROJECT=prj-dw-dev
GCS_BUCKET=prj-dw-dev-raw
XERO_STAGING_DATASET=dw_staging_xero
```

The loader source is deployed from the repository root with `main.py` as its
entrypoint. `.gcloudignore` intentionally excludes connection stores, `.env`
files, local raw output, Dataform files, tests, and build artifacts. To prove a
new revision before scheduling extraction, publish the attributes from one
existing `.meta.json` sidecar to `xero-raw-finalize`; the loader validates the
sidecar and performs an idempotent BigQuery MERGE.

Dataform Xero declarations and all ordinary Xero downstream references use
`dw_staging_xero`. Sensitive Bank Feeds, Finance, Identity, and document
datasets remain separate and are not enabled by this ordinary staging setup.

Custom Connection scopes are granted in the Xero Developer Portal, not by an
`XERO_SCOPES` environment variable. PKCE scope changes require tenant
reauthorization; existing refresh tokens do not gain new permissions. A sync
with any entity error or a hit page cap now exits nonzero after writing its
manifest. Budget and contact-group syncs fetch each listed record's detail
document, which adds API calls proportional to the number of records.
Payment Services and Branding Theme Payment Services are intentionally outside
the warehouse contract. Do not request the write-capable `paymentservices`
scope; the CLI rejects those routes. Coverage reports `out_of_scope` when they
have no evidence and `policy_violation` if an excluded route is ever observed.
The `PaymentServices` collection embedded in a Branding Theme is also removed
before the v2 BigQuery staging payload is written.

## CLI surface

```text
xero sync [flags]      Fetch raw Xero pages and land them in GCS (or local disk)
xero sync-extended     Opt-in Assets / Projects / Bank Feeds / Identity / Finance raw sync
xero sync-attachments  Fetch Accounting attachment metadata for explicit or discovered parents
xero sync-accounting-get  Fetch one pinned Accounting GET operation per selected tenant
xero healthcheck       Mint a token per tenant; --check-bucket also probes GCS
```

Mode-selecting flags (precedence top→bottom): `--backfill FROM:TO`, `--reports`
(+ `--as-of`), `--where <expr>` / `--no-window`, `--business-from` /
`--business-to`, `--full-with-window`, `--full`, else incremental. Plus
`--window-days`, `--chunk-months`, `--entity <csv>`, `--tenant <csv>`,
`--max-concurrent`, `--dry-run`, `--local-dir`.

For historical Accounting reports, select only compatible entities and pass
explicit report parameters. Without these flags the daily report schedule
remains as-of or month-to-date:

```bash
xero sync --reports --entity report_profit_and_loss \
  --report-from 2025-01-01 --report-to 2025-12-31 --report-periods 3
xero sync --reports --entity report_balance_sheet \
  --as-of 2025-12-31 --report-periods 3
```

`--report-from` and `--report-to` must be paired and apply only to
ProfitAndLoss or BankSummary. `--report-periods` (1-12) applies only to
ProfitAndLoss or BalanceSheet. The query signature is retained in raw report
metadata. TrialBalance does not accept `periods` in Xero's current Accounting
OpenAPI specification.

## Accounting UUID details

`xero sync --expand-details` follows the documented `/{Resource}/{UUID}`
routes for Accounting entities selected by `--entity`. It is opt-in because a
large collection can require one additional Xero call per record. The default
limit is 1,000 IDs per entity; exceeding `--max-detail-records` marks the
entity failed in the manifest and makes the CLI exit nonzero. Run a bounded
dry-run first, then set a limit appropriate to the tenant and quota budget:

```bash
xero sync --entity invoices --expand-details --max-detail-records 500 \
  --dry-run --local-dir <restricted-local-dir>
```

This currently substitutes full detail envelopes for collection pages within
that entity's raw run. It adds access to 22 Accounting GET path shapes beyond
Budget and ContactGroup details, but does **not** yet retain both list and
detail envelopes independently or prove field-level warehouse completeness.
Do not use it as a certified full-collection deletion signal. Alternate-ID,
history, binary attachment and PDF routes require the direct catalog command
and explicit parameters; see
[`XERO_ENDPOINT_PARITY.md`](XERO_ENDPOINT_PARITY.md).

## Direct Accounting GET warehouse landing

The read-only `sync-accounting-get` command covers all 126 GET operation IDs
in the pinned Accounting OpenAPI catalog. The command validates path, query,
and header names/required values against that catalog, and each invocation
writes one response per selected tenant plus a run manifest. Examples:

```bash
xero sync-accounting-get --operation getInvoiceReminders --tenant <tenant-uuid>
xero sync-accounting-get --operation getInvoiceHistory \
  --path-param InvoiceID=<invoice-uuid> --tenant <tenant-uuid>
xero sync-accounting-get --operation getInvoiceAttachmentById \
  --path-param InvoiceID=<invoice-uuid> --path-param AttachmentID=<attachment-uuid> \
  --header contentType=application/pdf --tenant <tenant-uuid>
```

For binary routes, configure `XERO_DOCUMENT_GCS_BUCKET` as a *different*,
private bucket. Restrict object read to document operators. Create the BigQuery
dataset `staging_xero_documents` and set
`XERO_DOCUMENT_DATASET=staging_xero_documents` on the document loader.
Configure that bucket's OBJECT_FINALIZE notification to the loader Pub/Sub
topic (or deploy a separate loader instance with both bucket variables set).
The loader accepts binary sidecars only from that bucket, never downloads
binary object bytes, and writes only metadata and a raw-object pointer to
`staging_xero_documents.accounting_get_responses`. JSON responses stay in the
ordinary raw bucket and land in `staging_xero.accounting_get_responses`. Create
datasets and grant permissions before production runs. The command still uses
`GCS_BUCKET` for JSON, so it is required even for binary runs; `--dry-run`
writes to local disk.

This is a direct operation runner, **not a scheduled tenant-wide extraction**.
Supply valid IDs, dates, filters, and required scopes/entitlements. Responses
over 8 MiB are represented by a GCS pointer rather than BigQuery JSON text;
the fetch itself is capped at 50 MiB. A 304 response creates only a manifest
outcome. Do not treat a missing row as a deletion signal. Live validation of
all 126 routes and complete fan-out remain outstanding.

## Accounting attachments

`sync-attachments` is an opt-in Accounting attachment metadata lookup. It
uses the normal Accounting rate limiter and requires the
`accounting.attachments.read` scope (reauthorize existing PKCE connections
after adding it). Custom Connections need the scope granted in the Xero
Developer Portal. Use one supported parent type per run:

```bash
xero sync-attachments --entity invoices \
  --parent-id 5f3c9a2e-0000-0000-0000-000000000000 --tenant <tenant-uuid>
xero sync-attachments --entity invoices --discover \
  --max-scan-pages 20 --max-parents 100 --tenant <tenant-uuid>
xero sync-attachments --entity invoices --discover --include-unflagged \
  --max-scan-pages 20 --max-parents 500 --tenant <tenant-uuid>
xero sync-attachments --entity quotes --discover --include-unflagged \
  --max-scan-pages 20 --max-parents 100 --tenant <tenant-uuid>
xero sync-attachments --entity accounts --discover --tenant <tenant-uuid>
```

Discovery supports Accounts, BankTransfers, BankTransactions, Contacts,
CreditNotes, Invoices, ManualJournals, PurchaseOrders and Quotes. Accounts
and BankTransfers use a single unpaginated collection request; the other
types scan a complete paginated parent collection. By default, discovery
looks up parents with `HasAttachments=true`. Quote list responses omit that
field, so Quote discovery requires `--include-unflagged` and checks every
scanned parent. A page or parent limit hit fails the run rather than treating
a partial scan as complete.
Defaults are 20 scan pages and 100 selected parents. `--include-unflagged`
looks up every scanned parent, including those now reporting no attachments;
use it for deletion reconciliation, with a sufficient parent cap and an
explicit quota budget.

The command requires `GCS_PREFIX=raw/xero`. It stores each original
`Attachments` JSON list and sidecar under the `attachment_metadata` endpoint,
with `x-parent-endpoint` and `x-parent-id` identifying the source document.
The loader MERGEs typed rows into `staging_xero.attachment_metadata` and
appends every complete parent-list observation, including empty lists, to
`staging_xero.attachment_list_snapshots`. `dw_1_silver_xero.dim_attachment`
uses each observed parent's latest complete list to expose currently listed
attachments. Staging metadata remains historical last-observed data; an empty
list removes the parent's attachments from Silver, not from staging. Run-history
outcomes include the parent type and UUID for per-parent troubleshooting.
Missed loader events can be replayed with
`python3 -m etl.backfill_gcs attachment_metadata`. This flow does not download
binary content. Every scanned page and selected parent costs an API call;
check the tenant's daily allowance before scheduling broad sweeps. Deletions
are reflected only for parents rechecked after the change. Deploy the updated
loader and complete one attachment sync or backfill before running the Silver
model, so both attachment staging tables exist. Verify an empty parent list
creates a snapshot with `attachment_count=0` and removes that parent's rows
from `dim_attachment` on the next Dataform run.

## Bounded bulk extraction

Accounting and extended collection syncs now pull one raw response at a time,
await its sink upload, and only then fetch the next page. The token provider is
consulted before each request, allowing PKCE refresh during long runs. Raw
collection and report responses are capped at 50 MiB, including chunked bodies.
Interactive MCP reads retain their stricter 5 MiB limit. Compatibility APIs
that return `Vec<RawPage>` still collect pages; do not use those as streaming APIs.

Late source, refresh, or sink failures produce an error outcome with the pages
and records already committed by the sink. Extended membership snapshots are
published only after a complete successful list, never after a page cap or
late failure. Raw body and metadata writes are not a transaction; reconcile any
partial object/sidecar pair after an upload failure. There are no new persisted
checkpoints or watermarks: reruns append new raw objects and downstream loaders
deduplicate them.

Detail expansion must keep the original collection until every requested
detail succeeds. It stages exact bytes in temporary directories (Unix `0700`,
files `0600`), then replays either the full detail set or the original collection
one page at a time. A detail failure retains collection data but marks the
entity incomplete; it does not silently report success. The combined staging
budget is 512 MiB per expanded entity by default. Budget and ContactGroup
mandatory detail fan-out is capped at 10,000 unique IDs unless an explicit
detail limit overrides it; opt-in `--expand-details` retains its 1,000-ID CLI
default. Extended membership/fan-out ID sets are capped at 100,000 per list.
Exceeding a bound fails the entity rather than certifying a partial list.

Temporary files are removed on normal completion and task cancellation.
Abrupt termination or a host crash can leave private spool directories behind.
Use an encrypted, access-restricted `XERO_DETAIL_SPOOL_DIR` in production and
remove abandoned `xero-detail-*` directories only after confirming their worker
is no longer running. The spool contains source financial data, not diagnostics.

## Shared Xero quotas

Without Redis configuration, the native client enforces process-local tenant
concurrency/minute/daily limits and an app-minute budget. To coordinate multiple
workers, set both `XERO_REDIS_URL` and `XERO_REDIS_APP_KEY` before startup on
every CLI/MCP worker sharing that app. Changing these settings requires restart.
Use the same Redis database and cohort; separate Xero apps should use separate
cohorts. Older v1 clients and unconfigured workers are outside this coordinator.

Redis uses atomic server-time reservations for tenant and app minute budgets,
observed daily allowance, shared 429 cooldowns and at most five tenant request
leases. Successful responses update `X-MinLimit-Remaining`,
`X-DayLimit-Remaining` and `X-AppMinLimit-Remaining`. Daily exhaustion admits
bounded recovery probes rather than resetting to an invented daily allowance.
Startup validates configuration/connectivity; backend errors fail closed with
sanitized errors, not a fallback to uncoordinated requests.

Non-loopback TCP requires certificate-verified TLS; plaintext is allowed only
for literal loopback addresses or a local Unix socket. Store the authenticated
URL in a secret manager. Use a private network, dedicated Redis ACL/key
namespace and a persistence/retention policy appropriate to quota metadata.
The coordinator stores tenant identifiers/counters, not tokens or response
payloads. Request leases expire after 120 seconds (the HTTP timeout is 60
seconds); abandoned leases recover, but a worker frozen beyond the lease TTL
is not covered by a strict distributed in-flight guarantee. A Redis outage
must be resolved before extraction resumes.

## Extended Xero APIs

The endpoint catalog, data contract, security boundary, first-run gates and
known gaps are in [`XERO_EXTENDED_RUNBOOK.md`](XERO_EXTENDED_RUNBOOK.md).
`sync-extended` requires `GCS_PREFIX=raw/xero`, the path the ETL reader scans.

`sync-extended` is deliberately separate from the Accounting schedule. Select
families explicitly; a missing scope or partner entitlement fails that request
and is recorded in the run manifest without changing Accounting ingestion.
Existing PKCE connections must be reauthorized after adding scopes. Custom
Connection scopes are granted in the Xero Developer Portal.
Run Assets/Projects separately from Bank Feeds/Finance/Identity. For the latter,
point `GCS_BUCKET` at a dedicated bucket with restricted IAM and set
`XERO_SENSITIVE_GCS_BUCKET` to that same bucket. Deploy a separate Cloud
Function instance with matching `GCS_BUCKET`, explicit `GCP_PROJECT`, and
`XERO_SENSITIVE_DATASET`. Create `staging_xero_sensitive` and
`dw_1_silver_xero_sensitive` with restricted IAM before loading or modeling
sensitive data. PKCE Identity `/connections` is app-wide; omit `--tenant` for
that family in PKCE mode.

| Family | GET scope(s) | Raw path segment | Access note |
|---|---|---|---|
| Assets | `assets.read` | `1.0` | Three required statuses are fetched separately; asset details, types and settings are included. |
| Projects | `projects.read` | `2.0` | Projects, users, per-project tasks and time; detail GETs are included. |
| Bank Feeds | `bankfeeds` | `1.0` | This scope is write-capable and Xero restricts the API to approved integrations. |
| Identity | bearer token | `identity` | PKCE `/connections` is fetched once for the app; Custom Connections are fetched per tenant. |
| Finance | `finance.cashvalidation.read`, `finance.statements.read`, optionally `finance.bankstatementsplus.read` | `1.0` | Partner access is required; BankStatementsPlus also needs bank account UUIDs. |

Examples:

```bash
xero sync-extended --family assets,projects --tenant <tenant-uuid> --dry-run
xero sync-extended --family finance --from 2026-09-01 --to 2026-09-30 \
  --bank-account-id <bank-account-uuid> --tenant <tenant-uuid>
xero sync-extended --family identity
```

`--no-details` skips per-ID GETs to reduce API calls, but may omit fields that
Xero only returns in detail responses. Finance requires both `--from` and
`--to`; rerun each required period for historical snapshots. Other families
currently perform full collection pulls. The Bank Feeds API is distinct from
Accounting `BankTransactions` and should not be requested merely to read normal
bank transactions.

Deploy the updated GCS Cloud Function before the first extended sync. It
populates `staging_xero.extended_records` for Assets/Projects and restricted
`staging_xero_sensitive.extended_records` for Bank Feeds/Finance/Identity,
both with full `payload_json` (grain: tenant, endpoint, record ID). Backfill with
`python3 -m etl.backfill_gcs <endpoint>`; the endpoint catalog is in
`etl/xero/extended.py`. Complete collection membership also lands in
`extended_list_snapshots`; the current-state Silver models require these
snapshots and same-run staging headers, so replay the relevant
`*_list_snapshots` endpoints before enabling them. The new Silver models are
disabled by default. After
the staging tables exist, enable Assets/Projects with Dataform variable
`enable_xero_extended=true`; enable Bank Feeds/Finance only in the restricted
workflow with `enable_xero_sensitive=true`. Keep the sensitive Silver dataset
restricted too. Raw GCS objects remain
the historical source of truth; staging holds
the latest current record or latest response for each requested Finance period.
The per-tenant evidence matrix and opt-in nightly exception command are in
[`XERO_EXTENDED_RUNBOOK.md`](XERO_EXTENDED_RUNBOOK.md#nightly-evidence-and-exceptions).

## Run history

Manifest sidecar events now populate `staging_xero.sync_runs` and
`staging_xero.sync_run_entities` (grain `run_id, outcome_index`). Sensitive
family runs use the corresponding restricted dataset. These tables contain
status, counts and termination reasons, not raw payloads or error strings; the
full manifest remains in its source bucket. After deploying the updated Cloud
Function, replay older manifests once for each bucket:

```bash
GCS_BUCKET=<ordinary-bucket> GCP_PROJECT=<project> \
  python3 -m etl.backfill_manifests
GCS_BUCKET=<restricted-bucket> XERO_SENSITIVE_GCS_BUCKET=<restricted-bucket> \
  XERO_SENSITIVE_DATASET=staging_xero_sensitive GCP_PROJECT=<project> \
  python3 -m etl.backfill_manifests
```

Replay is idempotent. A loader failure leaves the original GCS manifest intact
for retry. The client caps concurrent calls at five per tenant and stops when
the observed daily allowance reaches zero; the run then records an entity
error and exits nonzero.

## V1 parity decisions

V2 carries forward V1's useful operational ideas as typed report overrides,
header-aware quota budgeting, and queryable history derived from durable run
manifests. It does not copy V1's sync executor or attachment download helper.
The latter issues binary requests outside V1's shared rate limiter. V2 now
supports bounded parent discovery and deletion-aware current metadata, but
binary content ingestion and size/access controls remain before V2 can claim
attachment parity. Custom Connections do not use refresh tokens.

## Production PKCE connection store

Set `XERO_CONNECTION_STORE_BACKEND=gcs` and identical
`XERO_CONNECTION_STORE_GCS_BUCKET`, `XERO_CONNECTION_STORE_GCS_OBJECT`, and
optional `XERO_CONNECTION_STORE_KMS_KEY` values on the onboarding BFF and sync
CLI. Use a dedicated bucket with uniform bucket-level access, public access
prevention, restricted IAM, audit logging, and object versioning **disabled**:
older object generations contain superseded refresh tokens. Also review the
[bucket soft-delete policy](https://docs.cloud.google.com/storage/docs/soft-delete):
GCS enables it by default and can retain overwritten objects even when
versioning is off. Disable it for this credential bucket if your recovery
policy permits; otherwise restrict soft-deleted-object list/restore access and
include retained generations in token incident response. The BFF and CLI
identities need read/write access to that bucket; grant no general raw-data
readers access. The application rejects a token-store bucket equal to either
configured raw-data bucket. Do not log or export the JSON object.

The store uses GCS generation preconditions for every mutation and keeps an
expiring refresh lease in that same object, so a stale worker cannot commit a
rotated token. One Xero authorization may cover several tenants; the BFF
publishes those tenant records atomically and refresh rotation updates the
whole grant. The file backend remains for single-process local development,
not multi-instance production. An existing local file is **not** automatically
migrated; reconnect tenants through the BFF before switching the sync CLI to
GCS. Test one tenant's connect, refresh, restart, and second-worker access
before scheduling production PKCE jobs. This GCS path has not yet been
validated against a live bucket or Xero tenant.

## Layered sync cadence

A single rolling pull does not keep the bucket complete on its own (hard deletes
never re-enter a modified window; rare linked changes don't bump a parent's
`UpdatedDateUTC`). Run **layered** jobs at different cadences — each lands the
same append-only GCS layout, tagged via the `x-sync-type` object metadata.

| Layer                  | Cadence  | Command                                                        | Catches |
|------------------------|----------|---------------------------------------------------------------|---------|
| incremental            | every 4h | `xero sync --window-days 3`                                    | churn, closes, voids |
| open-sweep             | daily    | `xero sync --where 'Status=="AUTHORISED"' --no-window`         | open items unchanged > 3d |
| master                 | daily    | `xero sync --full`                                             | master data (cheap, full) |
| reports                | daily    | `xero sync --reports --as-of <today>`                         | point-in-time snapshots |
| rolling-full (tight)   | weekly   | `xero sync --full-with-window`                                | recent hard-deletes, drift |
| rolling-full (wide)    | monthly  | `xero sync --business-from <today-90d> --business-to <today>` | drift in days 31–90 |
| backfill               | once     | `xero sync --backfill 2020-01-01:<today> --chunk-months 1`    | full history |

The ready-to-install schedule is in
[`deploy/crontab.example`](../deploy/crontab.example). Install it with:

```bash
# edit XERO_BIN / ENV_FILE / LOG_DIR at the top of the file first
crontab deploy/crontab.example
crontab -l        # confirm
```

## One-time backfill

Run the backfill **once** when onboarding a bucket — not on a schedule (it is
expensive and would exhaust the per-tenant rate-limit budget). Pin it to one
tenant at a time:

```bash
xero sync --tenant <tenant-uuid> \
    --backfill 2020-01-01:2026-06-17 \
    --chunk-months 1
```

The range is expanded into business-date chunks (oldest→newest) and run as one
sub-run per chunk; the per-chunk manifests are merged into a single run manifest.

### Accounting detail rollout

The nine detail-derived staging tables must exist before Dataform executes their
silver models. Run the idempotent schema script first; it pins IDs, account
codes, budget amounts, and dates to the intended BigQuery types rather than
relying on first-file autodetection:

```bash
bq query --project_id=prj-dw-dev --use_legacy_sql=false \
  < etl/sql/create_xero_detail_staging.sql
```

Deploy the updated raw sync and ETL/Cloud Function, then run a fresh sync for
`budgets`, `contact_groups`, `batch_payments`, `linked_transactions`, and
`repeating_invoices` for each tenant. Historical
budget/group list responses lack the child details, so a GCS backfill alone
cannot recover their balances or membership. After the fresh sync, run:

```bash
python3 -m etl.backfill_gcs budgets contact_groups batch_payments linked_transactions repeating_invoices
cd Dataform
dataform compile --quiet
```

Build the nine dependent `dw_1_silver_xero` models after staging is populated.
Compare header and current child counts per tenant; child models join only the
header's latest `synced_at` snapshot, so a group, batch, or template with zero
current children should produce none.
An older Pub/Sub event or backfill can no longer overwrite a newer staged
header when `synced_at` is present.

The latest inspected run across all seven tenants contained linked transactions
and repeating invoices, but zero expense claims, prepayments, or receipts.
Those three still land raw responses in GCS; their unparsed-endpoint warning
should prompt payload-led parser work when non-empty records first appear.

## Live validation

Before wiring the scheduler, validate against a real environment in two steps.

1. **Dry-run to local disk** — proves auth + the object/metadata layout without
   touching GCS:

   ```bash
   xero sync --reports --as-of 2026-06-17 --dry-run --local-dir ./out
   find ./out -type f            # inspect the raw layout + .meta.json sidecars
   ```

2. **Real incremental window against the dev bucket** — proves the GCS upload
   path end-to-end for a single tenant:

   ```bash
   # .env must have GCS_BUCKET=xero-raw-dev + GOOGLE_APPLICATION_CREDENTIALS set
   xero healthcheck --check-bucket          # token mint + bucket auth
   xero sync --window-days 3 --tenant <tenant-uuid>
   ```

   Then confirm objects landed:

   ```bash
   gsutil ls -r gs://xero-raw-dev/raw/xero/<tenant-uuid>/2.0/ | head
   gsutil ls    gs://xero-raw-dev/_manifests/xero/$(date -u +%Y-%m-%d)/
   ```

## Production deploy

Build and push the one-shot job image, then have the host scheduler invoke it.

```bash
docker build -t gcr.io/prj-dw-dev/xero-service-v2:0.1.0 .
docker push     gcr.io/prj-dw-dev/xero-service-v2:0.1.0
```

The container's entrypoint is the `xero` binary; pass the CLI args at run time:

```bash
docker run --rm \
  --env-file /etc/xero/xero.env \
  -v /secrets/gcs-sa.json:/secrets/gcs-sa.json:ro \
  gcr.io/prj-dw-dev/xero-service-v2:0.1.0 \
  sync --window-days 3
```

Schedule it with whatever the host provides:

- **VPS / bare host** — cron via [`deploy/crontab.example`](../deploy/crontab.example)
  (host binary or `docker run`).
- **GCP** — a **Cloud Run Job** (not a Service) per layer, triggered by **Cloud
  Scheduler** on the cadence above. The job's service account needs
  `roles/storage.objectCreator` and `roles/storage.objectViewer` on the raw
  bucket. A lost upload response is recovered by reading object metadata and
  verifying the generation, content type, size, CRC32C, SHA-256, and provenance
  before publishing its sidecar. Creator-only access cannot complete recovery.

The image is non-root (uid 10001 `xero`) and uses `tini` as PID 1 so SIGTERM
cancels the job cleanly. It exits 0 on success, non-zero if any layer-level check
fails (per-entity errors are isolated and recorded in the manifest, not fatal).

## GCS bucket setup (one-time per environment)

```bash
# bucket per environment
gsutil mb -l europe-north1 gs://xero-raw-dev

# runtime SA needs object-create and object-view for idempotent upload recovery
gsutil iam ch serviceAccount:xero-runner@prj-dw-dev.iam.gserviceaccount.com:objectCreator gs://xero-raw-dev
gsutil iam ch serviceAccount:xero-runner@prj-dw-dev.iam.gserviceaccount.com:objectViewer  gs://xero-raw-dev
```

`healthcheck --check-bucket` additionally calls `buckets.get`; grant
`storage.buckets.get` when that optional probe is used. Scope raw-object read
access to this bucket and keep the separate sensitive/document buckets on
their own identities and policies.

Date-partitioned object keys make lifecycle rules trivial — e.g. transition raw
objects to Coldline after 90 days. Manifests live under `_manifests/xero/…`,
outside `GCS_PREFIX`.

## Monitoring (what to watch)

- **Job exit code** per layer (cron/Cloud Scheduler should alert on non-zero).
- **Per-entity errors in the run manifest** (`_manifests/xero/<date>/<run_id>.json`,
  `entities[].error`) — one entity failing does not abort the run.
- **`x-record-count` / page counts** trending to zero where you expect data, or
  spiking unexpectedly.
- **Xero rate-limit headers** in the tracing logs (`info`/`debug`) — daily
  5000-call cap, 60/min per tenant. Stagger cron minutes so layers don't collide.
- **Object freshness in GCS** — most recent `run_date` partition per endpoint
  should advance on schedule.

## Common ops

```bash
# Prove auth for every configured tenant (and bucket wiring):
xero healthcheck --check-bucket

# Re-run a single layer for one tenant after a failure (safe — append-only):
xero sync --window-days 3 --tenant <tenant-uuid>

# Re-pull a specific entity only:
xero sync --window-days 7 --entity invoices,payments --tenant <tenant-uuid>

# Inspect the latest manifest:
gsutil cat gs://xero-raw-dev/_manifests/xero/$(date -u +%Y-%m-%d)/*.json | head
```

## Troubleshooting

| Symptom                                           | Likely cause / fix                                            |
|---------------------------------------------------|--------------------------------------------------------------|
| `config error — check your .env file`             | Missing `XERO_ORG_N_*` / `XERO_CC_*`; custom connection not configured |
| `GCS config error (set GCS_BUCKET)`               | `GCS_BUCKET` unset for a real run (dry-run does not need it)  |
| `failed to build GCS sink …`                      | `GOOGLE_APPLICATION_CREDENTIALS` path wrong or SA lacks bucket access |
| `GCS_PREFIX must not start with '_manifests'`     | Manifests are reserved; choose another prefix (default `raw/xero`) |
| `requested tenant not configured: <id>`           | `--tenant` value isn't among the configured `XERO_ORG_N_*` tenants |
| `report entity X is only valid with --reports`    | Pass report entities only under `--reports`                   |
| `401` on a report/entity                          | Custom Connection grant can't reach it (Xero platform constraint); drop it |
| 429 / daily-limit storms                          | Too many layers overlapping; stagger cron minutes, lower `SYNC_MAX_CONCURRENT` |
