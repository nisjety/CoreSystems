# Xero main GCP migration

## Current checkpoint (2026-10-16)

The target project is `aquatiq-datavarehus`, linked to billing account
`01C19F-28FE35-1B76F1`. Billing was enabled before the copy. The earlier
proposed project `prj-dw-main` was not used; do not run the old
`deploy/bootstrap-gcp-main` script because it still targets that project.

`aquatiq-datavarehus.dw_staging_xero` now contains a copy of
`prj-dw-dev.dw_staging_xero` in `europe-north2`: 51 physical tables and three
views. The views were recreated with references to the destination project.
Verification compared every object by row count and reported byte size (zero
mismatches, 477,201 rows and 131,228,931 bytes total), all 910 column
definitions (zero mismatches), and all three rewritten view definitions. The
dataset and tables have no labels and no default table expiration. The source
dataset was not changed. This is a snapshot; source writes after the copy are
not replicated.

The copied dataset contains the **older typed Xero staging contract**, including
legacy `v2_*` control tables. Those legacy objects are historical only. The
active path does not use the endpoint-layout baseline in
`prj-dw-dev.dw_staging_xero_main`; it writes the natural-name raw contract in
`aquatiq-datavarehus.dw_staging_xero` and projects compatible current typed
rows for the existing business models.

The chosen immediate source contract is the copied **typed** staging tables.
An isolated Dataform repository, `Xero` in `europe-north1`, now uses the
custom execution identity `xero-dataform@aquatiq-datavarehus.iam.gserviceaccount.com`.
It has read access to staging, write access only to the Xero output datasets,
and BigQuery job creation in this project. The new datasets
`dw_bronze_xero`, `dw_bronze_xero_assertions`, `dw_1_silver_xero`, and
`dw_1_silver_xero_assertions` are all in `europe-north2` without labels.
The isolated source is [Dataform/xero_warehouse/README.md](../Dataform/xero_warehouse/README.md).

The first controlled Dataform run and a repeat run both succeeded. Staging,
bronze, and silver matched at 886 accounts, 13,491 invoices, and 13,303
payments across seven tenants. Customer payments (`ACCRECPAYMENT`) numbered
3,020; 1,143 payments retained the Xero `DELETED` status. Payment fields and
deletion flags had zero staging-to-silver mismatches; 161 customer payments
retained their deleted marker. The bronze repeat run added no duplicates.
The silver models distinguish explicit deletion from records missing in
staging. The copied snapshot itself is still not original raw Xero history and
cannot prove deletion from absence; only the active raw generation carries
that source evidence. No release configuration or scheduled workflow has been
created.

### Seven-company Account detail validation (2026-10-16 Oslo)

Run `d6dfebdf-d006-49a7-9427-2b4e20d3097e` completed seven `accounts` list
outcomes and 886 `getAccount` direct-detail outcomes with no Xero API error.
Its manifest declares 1,772 records; pinned raw staging reconciled exactly to
seven list receipts / 886 account records and 886 direct-detail receipts /
886 account records. The full isolated Dataform workflow
`1792185191-4408ee52-d76b-4ae8-a5f7-503e80b713b9` then succeeded.

Silver reconciled every direct Account detail to its same-run collection row:
886 rows, seven tenants, and zero reconciliation failures. Gold
`xero_gold_finance.dim_account` exposes the same 886 accounts, each with direct
evidence and a successful reconciliation flag. This is raw-first evidence: a
direct detail can enrich and validate an Account but cannot create, remove, or
replace the lifecycle state established by the certified collection response.

The event loader batches ordinary raw writes with one transactional coordinator
and 80 request slots. It does **not** project catalogued `accounting_get_*`
events into the mutable `accounting_get_responses` convenience table: Eventarc
retries can repeat a request, while the raw history already preserves each
receipt and the Bronze/Silver/Gold graph reads that contract. The convenience
table may be rebuilt from pinned raw observations with
`etl.xero.fresh_backfill --current-projections` when required, but it is not a
warehouse authority. No raw, Bronze, Silver, or Gold row is deleted or
replaced by this policy.

### Warehouse semantic expansion (2026-10-16 Oslo)

The isolated Xero graph expanded to 579 actions and full workflow
`1792179945-2e8745e1-3dd6-44d6-9873-0bcbc08d1263` succeeded. The related
local Dataform project suite passed 49 tests before publication. This work uses
only Xero-native source semantics; it does not introduce Visma business units,
employees, departments, branches, warehouse, inventory, sales-order, or FX-rate
entities that Xero has not supplied.

`xero_endpoint_warehouse_coverage` separates raw delivery from warehouse model
scope. It records each tenant/operation's raw `operation_status`, its explicit
Bronze/Silver/Gold relation lists, and one contract state. The 182 declared
model relations are compile-checked through four dependency-group views of at
most 45 direct dependencies each, which keeps every Dataform action below the
50-dependency platform limit. This checkpoint is superseded by the direct-
detail contract closure below.

New conformed dimensions preserve 154 Xero organisation users across seven
tenants and 15 branding themes. User records are Xero access users, not
employees. Branding-theme keys resolve for all 5,989 populated references
across invoice, quote, purchase-order, and recurring-invoice outputs.

The payment-application facts use the canonical Payments feed, not
`invoice_payments`, whose historic nested rows lack fresh `synced_at` values.
They materialized 3,052 customer and 10,389 supplier applications. All 13,441
rows reconcile one-for-one to existing Gold payment facts with no source-amount
mismatches. The facts retain 70 customer and 122 supplier unresolved invoice
references and 166/990 deleted payment markers. Those states have NULL
additive amounts; the application-grain and Gold-safety assertions returned
zero failure rows.

The report model now includes business-facing non-additive snapshot facts:
8,713 Balance Sheet and Profit and Loss statement lines over 102 snapshots per
report type, plus 1,262 Bank Summary values. There are no orphan rows, missing
statement dates, additive flags, or report-snapshot safety failures. These are
source report outputs only: they must not be aggregated within their hierarchy
or metrics, across snapshots, or combined with general-ledger balances and
transaction facts. Statement-date parsing distinguishes Xero two- and
four-digit-year column headers before conformance to `dim_date`.

Every retained Gold document child now has a direct parent-reference assertion:
AR/AP invoice lines, credit-note lines and allocations, overpayment lines, bank
transaction lines, batch-payment details, recurring-template lines, quote lines,
and purchase-order lines must resolve to their retained parent. This preserves
lifecycle history without accepting orphaned business records.

Direct budget-detail coverage is now modeled separately from budget metadata.
The `getBudget` operation has one authoritative direct response for every
tenant. All seven current responses explicitly contain `BudgetLines = []`, so
the Bronze response observations contain seven zero-line rows and the Bronze,
Silver, and Gold account-period balance facts contain zero rows. This is kept
as an auditable source empty state: no zero budget amount is fabricated and no
source lifecycle row is removed. `xero_budget_balance_safety` returned zero
failure rows after the full 579-action workflow.

The fixed-asset Gold model is sourced from `getAssetById` detail responses and
reconciled against the `getAssets` inventory route by
`xero_gold_fixed_asset_safety`. The coverage contract now declares both routes
against the same Bronze, Silver, and Gold asset relations. Five tenants have
completed 655 detail responses with reconciliation evidence; two are explicitly
`not_attempted`, not failed or inferred complete.

`getProject` direct detail now has the same controlled treatment: 327 retained
direct responses across two tenants and two runs reconcile exactly to same-run
`getProjects` collection payloads for 181 distinct projects. Direct detail
enriches and validates Gold project outputs but cannot create a lifecycle state;
only the certified collection snapshots can mark a Project current or removed.

### Direct-detail contract closure (2026-10-17 Oslo)

The active coverage contract now declares 47 Gold operations across 186
compiled Bronze/Silver/Gold relations. All 150 documented Xero operations are
visible: 47 have Gold contracts, two Payment Services operations are explicitly
out of scope, and the remaining 101 are `not_modelled` because no authoritative
delivery has been recorded. All 300 authoritative tenant-operation deliveries
are Gold-covered; the count of authoritative rows outside a Gold contract is
zero.

`getContactGroup`, `getTask`, and `getTimeEntry` now follow the established
raw-first direct-detail pattern. The retained evidence currently reconciles one
contact group, six task details, and eight time-entry details to their same-run
collection observations with zero scalar mismatches. The contact-group direct
response reports four members while its same-run list payload reports zero;
this remains an explicit audit-only member-count mismatch. The list response
continues to own membership lifecycle.

Project task and time typed observations are projection-revisioned. A prior
incomplete projection remains as revision 1; the corrected revision 2 rows are
appended alongside it and are deterministically preferred by Silver and Gold.
This repaired three historical false task mismatches without modifying source
payloads or deleting typed history.

The new `xero_gold_core_reconciliation` manifest verifies row parity for 44
one-to-one Xero-native Silver-to-Gold relations across all seven tenants. It
materialized 308 explicit relation/tenant rows, including seven verified `0/0`
budget-balance rows, with zero mismatches and zero assertion failures. Split
customer/supplier Gold models remain protected by their existing domain grain
assertions until their filtered source contracts are added to this manifest.

## Active raw-first generation (2026-10-15 Oslo)

The active generation is `a3e6e7e5-04c1-4574-a63f-e0863ea9cef1`, activated at
`2026-10-15T23:19:31Z`. Objects are written below:

```text
gs://aquatiq-datavarehus-xero-raw/xero/a3e6e7e5-04c1-4574-a63f-e0863ea9cef1/
```

The `xero-staging-loader` Gen 2 function uses
`process_fresh_gcs_upload`. For every pinned `.meta.json` event in this exact
generation it first publishes immutable `raw_records`, `raw_collections`,
`raw_children`, and `ingest_receipts`, then projects compatible collection
pages into current typed tables. Catalogued direct Accounting GET responses are
kept raw-first because their authoritative model and reconciliation source is
the immutable receipt contract. The manifest writes `sync_runs` and
`sync_run_entities` through the raw transaction. Events outside the generation
are ignored before a BigQuery write.

A one-tenant Payments canary proved all layers: 40 source records, three raw
receipts, 40 typed payment keys, and 40 Silver payment keys. Those keys split
to 38 supplier and two customer Gold payments. Raw provenance and child-count
assertions all passed. This proves the path; it is not a historical-backfill
certificate or a claim that every catalogued endpoint has landed.

### Standard Accounting validation (2026-10-16 Oslo)

The first seven-tenant standard Accounting run
`cffa373d-1cdf-44dc-b2bb-f1a475a0f8e9` completed without a Xero API error:
189 tenant/endpoint outcomes, 276 source pages, and 4,604 source records.
`xero_raw_run_reconciliation` subsequently reported all 189 outcomes as
`complete`: 276 expected/received pages and 4,604 expected/received raw
records. The manifest is therefore backed by raw receipt and record evidence,
not merely by a successful CLI exit.

The full Xero Dataform workflow completed all 353 actions successfully after
the raw delivery drained. Key continuity checks found 544 raw Payments, 801 raw
Invoices, and 180 raw Bank Transactions matched by tenant/source ID in both
typed staging and Silver. Raw child lifecycle, receipt provenance, raw record
grain, finance, and Gold safety assertions passed in that workflow.

A repeat full workflow through the isolated Xero runner also succeeded after
the reporting-model hardening. Financial-report cells now preserve 2,876 Xero
percentage values as explicit non-additive numeric source values, and all
55,859 Silver report cell keys are unique and match Gold one-for-one. Endpoint
readiness is split into the current 27-operation Accounting warehouse contract
(ready for every tenant) and the full 148-operation in-scope catalog (not yet
complete), so the dashboard cannot confuse a proven business domain with full
API coverage.

This is a complete delivery/reconciliation result for the default 30-day
Accounting collection run, not an historical Xero backfill. It does not yet
cover data-dependent detail routes, attachment bytes, optional reports, or the
Assets, Bank Feeds, Finance, Identity, and Projects API families. Payment
Services remains intentionally excluded.

### Projects operational baseline (2026-10-16 Oslo)

The completed Projects baseline
`91fe3f5c-9fde-474a-a031-79237655d91e` covered seven Xero tenants and
completed with 376 request outcomes, 377 response pages, 231 source records,
and no Xero API errors. Raw staging reconciled every endpoint at page and
record grain: `projects_projects` 8/181, `projects_users` 7/43,
`projects_tasks` 181/3, and `projects_time` 181/4.

The bounded raw replay committed 2,891 pinned response sidecars in 29
idempotent batches. Run manifests are intentionally replayed separately: the
raw response backfill does not include `_manifests/xero`, while
`python3 -m etl.xero.fresh_backfill --generation <generation> --manifests-only`
restores `sync_runs` and `sync_run_entities` through their own receipt-atomic
transactions. The recovery run replayed all 11 active-generation manifests.
Both response and manifest replays verify pinned object generations and
checksums before BigQuery publication. A distinct manifest object that repeats
an existing logical `run_id` is rejected inside the publication transaction;
receipt idempotency alone is not sufficient for run-level evidence.

Projects Bronze certified all seven tenant runs. Five tenants had a valid empty
Project list, so the warehouse derives zero task/time request sets only from
that reconciled parent snapshot plus exactly one zero-record raw `/Projects`
receipt; it does not create raw rows for calls Xero did not need to make. The
complete Bronze, Silver, and Gold graph then materialized
181 current projects, 43 project users, three tasks, and four time entries.
The Gold operational snapshot contains 181 rows. It inherits resolved project
and contact keys from the conformed project dimension; its project, task, user,
and time-entry relationships are assertion-checked. Membership reconciliation
and Gold project safety assertions both succeeded.

## Fiscal-period contract (2026-10-16 Oslo)

Xero's Organisation response exposes `FinancialYearEndDay` and
`FinancialYearEndMonth`, but it does not expose a native journal posting-period
field or the Visma AP/AR/GL period-closure controls. The Xero model therefore
keeps the shared date dimension calendar-only and derives a tenant-specific
management fiscal calendar in Silver and Gold.

The seven connected tenants currently have only month-end fiscal configurations:
five close in December and two close on 30 June. The model generates ordinary
calendar-month periods, then labels them against each tenant's fiscal year. For
the June year-end tenants, 1--30 June 2026 is FY2026 period 12 and 1--31 July
2026 is FY2027 period 1. It does not create rolling 30th-to-30th monthly
periods.

Live validation after the managed Dataform rebuild found 601 tenant fiscal
periods. All 244,303 Gold GL lines and all 244,303
`xero_datamart.finance_activity` rows resolve to the single explicit basis
`calendar_month_from_organisation_financial_year_end`; none has an unresolved
or non-derived fiscal-period key. The fiscal safety workflow
`1792161909-c42131df-f40b-4e5a-b950-db6778c7c440` and finance-mart safety
workflow `1792162048-76a076bd-b9ff-4a2b-842d-47d19928f746` succeeded.

`dim_company_fiscal_configuration_history` retains all source observations:
28 configuration versions across seven tenants, with seven current versions.
No tenant has yet changed its fiscal configuration. Because Xero supplies an
observed configuration version rather than an effective date for restating
historical accounting periods, the fiscal safety assertion fails if a tenant
changes this configuration. A finance-approved migration policy is required
before accepting that change. It also fails for a non-month-end configuration
until its period-boundary policy is explicitly designed.

`bridge_company_date_fiscal_period` is a protected append-only mapping from a
tenant and calendar date to that baseline fiscal period. It prevents a changed
Organisation setting from relabelling already mapped historical dates. Once a
configuration change is observed, the bridge will not map new dates until the
required migration policy is implemented; this produces a visible unresolved
period rather than an unapproved restatement.

Gold GL and the finance activity mart expose `financial_period_basis` and
`is_financial_period_derived`. They must be used as a management reporting
calendar, not as proof of native Xero posting-period or close-control state.

## Current report snapshots (2026-10-16 Oslo)

The bounded seven-tenant report run
`1cbd6e4b-3862-402d-a804-945b73e82b47` fetched the six supported current-report
types (Profit and Loss, Balance Sheet, Trial Balance, Bank Summary, Budget
Summary, and Executive Summary) for 42 tenant/endpoint outcomes. It completed
with 42 source pages, 42 snapshots, and zero Xero API errors. Raw reconciliation
then confirmed all 42 outcomes complete with exactly 42 expected/received pages
and records before a full 353-action Dataform run succeeded.

The endpoint readiness summary now records 34 authoritative operations per
tenant: 27 current Accounting collections, direct budget detail, and the six
report operations. It certifies two coherent source cohorts: 28 standard
Accounting/detail operations must come from one successful run per tenant, and
the six reports must come from one successful reporting run per tenant. A tenant
is current-accounting ready only when both cohorts are complete; success evidence
from unrelated runs is not combined. The
two contact-scoped aged reports remain data-dependent and are not treated as
complete without explicit contact discovery. The full 148-operation in-scope
catalog remains separate and incomplete; 115 operations per tenant are clearly
marked as not yet attempted.

## Payments canary (2026-10-15 Oslo)

The new project now has a private, uniform-access, public-access-prevented
`europe-north2` bucket, `gs://aquatiq-datavarehus-xero-raw`. Cloud Storage
`OBJECT_FINALIZE` notifications publish to `xero-raw-finalized`. The Gen 2
`xero-staging-loader` function in `europe-north1` consumes that topic and
writes immutable raw history before updating the existing typed
`dw_staging_xero` contract. Its dedicated service account has object read
access to this bucket, writer access to this staging dataset, BigQuery job
creation, and invocation access on this Cloud Run service. The function is
private, limited to one instance with one concurrent request. The trigger
retries failures and has a dead-letter topic and review subscription. Raw
objects remain available for controlled replay.

A one-tenant, `payments`-only, 30-day PKCE sync fetched 20 records across two
pages. A manual batch load proved the raw-to-typed parser. After enabling the
notification, another bounded sync initially hit an Eventarc-to-Cloud Run
invoker IAM delay; granting the trigger identity `roles/run.invoker` on this
service resolved it. Subsequent raw uploads loaded automatically. Two run
manifests reached `dw_staging_xero.sync_runs`. A Dataform invocation from the
validated Xero compilation succeeded with all 137 actions.

Observed counts after the event-driven test: typed staging payments 13,304
(13,303 before the canary), bronze payment observations 13,343, silver current
payments 13,304, and gold supplier payments 10,247 (10,246 before). The latest
payment sync timestamp reached `2026-10-14 22:10:02 UTC`. At that checkpoint,
the other six organizations still had only the copied snapshot.

A second organization's 30-day payments test fetched 134 records in three
pages. The event-driven loader refreshed all 134 typed rows, including 35
customer receipts. Staging, silver, and gold each showed those 35 customer
payments with the same new sync timestamp (`2026-10-14 22:13:43 UTC`); the
subsequent Dataform invocation again passed all 137 actions. Once all pages
finished, staging and silver each held 13,324 current payments, bronze held
13,477 typed observations, and gold held 3,021 customer and 10,266 supplier
payments. The second canary added 20 current payments, one of them a customer
receipt. At this historical checkpoint, five organizations still had only the
copied snapshot. A later seven-tenant 30-day run is recorded below; neither
run is a complete historical backfill.
The only extractor used here was the local CLI with
the shared PKCE store and explicit `GCP_PROJECT`, `GCS_BUCKET`, and
`XERO_STAGING_DATASET`; the VPS configuration still points to the old project.

Raw object paths in this typed canary are `raw/xero/...`. Do not source the
old `.env.current` or `.env.fresh` during new-project tests: both target
`prj-dw-dev`, and their ingestion generation would route to the incompatible
endpoint-layout loader. Do not run the obsolete `deploy/bootstrap-gcp-main`.

The typed staging table upserts current rows. The raw-history path now captures
each delivered source response before that projection; Dataform materializes
those immutable receipts, record versions, collection versions, and child
versions into Bronze. Typed models remain a compatibility source for Silver and
Gold until raw coverage and reconciliation prove each business-domain switch.

## Remaining cutover gates

1. Continue the copied typed contract for immediate continuity. Do not point
   the endpoint-layout loader at these typed tables.
2. Add durable alerting and an operational replay procedure for the event-driven
   staging loader. Eventarc retry and a dead-letter topic now exist, but
   neither replaces alerting or a recovery drill. A recovery must replay both
   response sidecars and signed run manifests; raw-only replay cannot certify a
   warehouse run.
3. Move the extractor/VPS configuration to this project with a dedicated
   writer identity and shared PKCE token store. Schedule only after tenant and
   endpoint validation; do not reuse `prj-dw-dev` generation values or route
   endpoint-layout data into these typed tables.
4. Expand the isolated Xero Dataform project beyond its currently modeled entities,
   with source-specific freshness, grain, status, and reconciliation checks.
   Keep tenant ID as the organization key. Do not copy old bronze/silver objects.
5. Complete historical and optional API-family extraction, then reconcile
   raw pages, staging, bronze, silver, and gold by tenant and entity. Validate
   deletion/supersession markers and amounts before scheduling a baseline.
   Keep Payment Services out of scope.

Until those gates are complete, the new project has a working local
extraction/event-loaded pipeline, not a continuously scheduled full Xero sync.

## Local Docker validation

The local-only Compose override `docker-compose.local-main.yml` targets
`aquatiq-datavarehus` and the copied typed `dw_staging_xero` contract. It
mounts the existing owner-only PKCE connection store and the operator's Google
ADC file at runtime; neither credential is included in the image. Set
`HOST_UID=$(id -u)` and `HOST_GID=$(id -g)` before each Compose command so the
container can read and rotate the host-owned connection file. The BFF stays on
`http://localhost:5002`, and the one-shot `local-sync` service uses the same
PKCE store and destination. Do not use the base `xero` service for this test:
it still reads `.env` and its legacy Custom Connection configuration.

```bash
HOST_UID=$(id -u) HOST_GID=$(id -g) docker compose \
  -f docker-compose.yml -f docker-compose.local-main.yml up -d --build onboarding
HOST_UID=$(id -u) HOST_GID=$(id -g) docker compose \
  -f docker-compose.yml -f docker-compose.local-main.yml run --rm --no-deps \
  local-sync healthcheck --check-bucket
HOST_UID=$(id -u) HOST_GID=$(id -g) docker compose \
  -f docker-compose.yml -f docker-compose.local-main.yml run --rm --no-deps \
  local-sync sync --window-days 30 --max-concurrent 1
```

`healthcheck --check-bucket` verifies token refresh for each connected tenant
and reads bucket metadata. The raw-upload canary and runs below also proved
object-write permission and the event-driven staging path. The file
store uses a persistent private lock file to coordinate BFF and CLI refreshes
across local processes; the production GCS token store remains a separate
deployment decision. No schedule or complete historical backfill is implied
by these commands.

### October 15 validation result

The local BFF passed HTTP health checks at `http://localhost:8080` during the
October 15 validation. The current local Compose override uses the registered
`http://localhost:5002/api/xero/callback` redirect and serves host port 5002;
unauthenticated connection access returned 401. The Docker
`healthcheck --check-bucket` refreshed all seven connected organizations and
read the target bucket. A payments canary also proved raw object upload and
typed staging ingestion. The local 30-day default sync (`--window-days 30
--max-concurrent 1`) completed 189 tenant/entity outcomes across seven
organizations: 276 raw pages, 4,485 records, and no Xero API errors (run
`23b3a126-67c8-417c-804d-9ec5e72cd9be`). The run manifest and all outcomes
landed in `dw_staging_xero`; counts across the 23 nonempty typed entity tables
reconciled to 4,485 records at the run timestamp. The three default entities
without typed parsers (expense claims, prepayments, receipts) returned zero
records in this run.

The initial raw-object burst hit Cloud Run 429 responses because the loader
allows one in-flight request. Retry is now enabled, with a Pub/Sub dead-letter
topic `xero-staging-loader-dead-letter` and review subscription
`xero-staging-loader-dead-letter-review`. Both the Eventarc backlog and
dead-letter backlog must be checked after each run; an uploaded raw object is
not evidence that its typed row landed. The loader's child-table merge now
records `synced_at` so replay of an older page cannot overwrite a newer child
row. An invoice-page replay under the new loader revision added the timestamp
column and verified 108 invoice lines and 12 invoice payments for that page.

Dataform compilation reported no errors. Workflow invocation
`1792019047-abd3ac11-4af9-4f70-b5b3-03c87d88b9fe` succeeded with all 137
actions. Staging and silver both held 13,421 current payments and 13,750
invoices; gold held 3,049 customer and 10,335 supplier payments, plus 3,730
customer and 10,020 supplier invoices. Gold payment categories do not cover
37 other payment types, so those two gold payment counts are not expected to
sum to all staged payments.

A key-and-field reconciliation found no unmatched rows for those 13,421
payments or 13,750 invoices between typed staging and silver. Payment amount,
status, type, and deleted flag had zero mismatches; invoice total, paid, due,
status, and type had zero mismatches. This validates the compared fields for
the current rows, not the completeness of Xero history.

This test covers the default 27 Accounting collections in a 30-day modified
window. It does **not** certify all 150 catalogued GET operations, optional
reports, attachments, Finance/Bankfeeds/Identity/Projects/Assets APIs, or a
complete historical backfill. No production schedule or VPS cutover was made.

An additional unfiltered master-data run (`--full --max-concurrent 1`) covered
all seven organizations and all nine master entities: 63 tenant/entity
outcomes, 89 raw pages, 5,602 records, and zero Xero API errors (run
`a3df43e2-5ff4-4089-9aad-745d86d2ace9`). Its BigQuery manifest had the
same 63 outcomes and 5,602 records. Once the loader queue drained, refreshed
typed staging rows across the nine entity tables summed to exactly 5,602;
neither the Eventarc nor dead-letter subscription had a backlog. This is a
complete master-data pull for those nine entities, not a transactional-history
backfill.

The completed child-page replay published 147 current-run sidecars through the
new loader revision. Twelve child tables with nonempty current-run data gained
`synced_at`; the remaining optional/empty child tables will evolve when they
receive rows. No replay event remained queued or dead-lettered. Dataform
workflow invocation `1792021661-874e7322-7cdb-4249-ac37-9020b976abb4`
then succeeded on all 137 actions. Staging and silver each held 886 accounts,
2,206 contacts, 13,421 payments, and 13,750 invoices. Account/contact keys,
names, and statuses had no mismatches between staging and silver.

## Gold expansion (2026-10-15 Oslo)

The isolated Xero Dataform project now has `xero_gold_sales` and
`xero_gold_procurement` in `europe-north2` alongside finance/common. Eight
silver and nine gold models add item, quote, purchase-order, bank-transaction,
bank-transfer, and role-neutral contact coverage. A complete Dev workflow
invocation `1792096686-7b37a7ae-b6b9-4f9d-85f1-158e1ff8d671` succeeded
on all 190 actions, including new grain and measure-safety assertions. Gold
counts match typed staging: 2,206 contacts; 2,178 items; 1,319 quotes and 3,868 lines; 1,079 purchase
orders and 2,413 lines; 4,854 bank transactions and 5,022 lines; and 84 bank
transfers. One quote and three purchase orders have ambiguous child sets;
their header and line-level additive measures are NULL while source rows remain
visible. Header and child subtotals must also reconcile. Role-specific quote
and order contact keys are nullable for prospects without a customer or
supplier role; all present contacts can join to `xero_gold_common.dim_contact`.
These models are Xero-specific business interpretations, not a claim that
Xero has Visma sales-order, inventory-movement, or BU semantics. The larger
Visma gold catalog also includes entities not modeled or not available from
the current Xero typed source. Dataform remains unscheduled.

## Raw loader concurrency (2026-10-16 Oslo)

The Gen 2 `xero-staging-loader` revision `xero-staging-loader-00006-hok` is
active with 1 CPU, 1 GiB memory, one configured instance, and one concurrent
request. The raw writer commits the receipt, records, collections and children
inside one BigQuery transaction protected by a global `raw_control` fence.
Four simultaneous Eventarc deliveries could therefore conflict on that fence;
Pub/Sub retries prevented data loss, but the resulting 500/429 churn was not
the desired operating mode.

The active deployment serializes handler requests to match the transaction
contract while retaining Eventarc retry and the dead-letter topic. The
source-controlled deploy helper requires an explicit reviewed isolated source
directory and rejects the checkout root. The 42-response report run drained
and reconciled completely through this path. Equal `synced_at` observations
across pages remain an ordering limitation; complete business-version history
continues to rely on raw page-aware Bronze history rather than typed current
tables alone.

## Provenance and Trial Balance Baseline (2026-10-16 Oslo)

The current `main` raw generation is proven across the three Xero business
families that have completed fresh source pulls. Accounting run
`dd588dd2-d6b8-464d-8830-6b606e8c2831` reconciled 203 manifest outcomes, 284
pages, and 4,583 records. Assets run
`368aa681-39d5-47f5-a866-0ebb1b3c3bb2` reconciled 690 outcomes, 693 pages,
and 1,323 records. Projects run
`8a681b6f-9598-4059-92c2-ed8b4b442512` reconciled 564 outcomes, 565 pages,
and 419 records. All 251 resulting tenant-endpoint contracts were complete
with no source errors.

Detail APIs produce several manifest outcomes for one tenant and endpoint.
`xero_raw_run_reconciliation` therefore aggregates manifest outcomes before
joining raw receipts and canonical raw records. Its grain is
`run_id + tenant_id + endpoint`, exposes `outcome_count`, and requires exact
aggregate page and record equality. It does not pretend the current manifest
format carries a per-detail-request join key.

Raw history remains append-only. A historical replay created 320 exact
duplicate raw-record versions (640 physical rows) in an earlier Project run.
`xero_raw_record_source_duplicates` retains and marks those source rows;
`xero_raw_record_history_canonical` deterministically selects one row per
immutable version for downstream models. The duplicate-conflict assertion
fails if rows sharing a version disagree semantically. No raw object, receipt,
or business history was deleted or overwritten.

Xero now has the account-period balance capability expected from the Visma
standard, using the Xero-native Trial Balance report rather than invented
ledger balance data. Silver and Gold `fact_general_ledger_balance` retain
report snapshot and account grain, native source Debit/Credit/YTD Debit/YTD
Credit values, source balance date, functional currency, and derived tenant
fiscal period. Measures are additive only within one validated source snapshot
and explicitly non-additive across snapshots. The 102 retained source
snapshots produced 5,881 distinct account rows in both layers; all account,
company, and fiscal references resolved, and the row-shape, balance, and
Silver-to-Gold assertions returned zero failures. Full isolated Xero workflow
`1792171246-e765425f-c3e8-4cec-9b44-457a4cc4936b` succeeded after these
changes.

## Business Consumption Marts (2026-10-17 Oslo)

`xero_datamart` now exposes four Xero-native consumption marts: existing
functional-currency `finance_activity`, plus quote-grain `sales_pipeline`,
purchase-order-grain `procurement_commitments`, and project-grain
`project_operations`. They follow the Visma consumption-model discipline while
preserving the Xero contract rather than copying Visma-only semantics:

- `sales_pipeline` is not revenue, a sales order, or a forecast. It retains the
  source lifecycle/value for audit and provides a separate effective amount.
  A source-open quote is not effectively open after its expiry date at the
  source observation timestamp.
- `procurement_commitments` is not booked AP cost. It exposes only the Xero
  purchase-order commitment measure when its parent/line set is safe.
- `project_operations` is not project P&L. Xero's current contract does not
  provide project-linked accounting costs/revenue allocation, margin, or an
  authoritative reporting-FX rate; its P&L and NOK values remain explicitly
  unavailable.

The individual mart materialization invocation
`1792189525-b8bf1f79-9396-4ef8-bcc9-6b9c5effa930` succeeded. The dedicated
`xero_business_mart_safety` assertion invocation
`1792189603-6a171beb-ecd1-41d2-8a20-9d1b4a4797f8` then succeeded with zero
violations. Native-grain reconciliation confirmed 1,322 quote/pipeline rows,
1,080 purchase-order/commitment rows, and 181 project/operations rows, each
with zero missing/extra keys and zero lifecycle/measure mismatches. The
pipeline mart retained 266 expired rows for history but exposed no effective
pipeline amount for any of them. No NOK or project-P&L value was populated.

An earlier invocation that included the mart's full dependency closure failed
while rebuilding busy append-only Project Bronze tables due to BigQuery's
per-table update rate limit. This did not affect the completed mart data. The
documented operating rule is now to run dependent mart targets directly against
already-certified Gold sources, then execute their dedicated assertion; run a
full dependency workflow only when upstream Bronze/Silver/Gold sources are
being refreshed.

## Account Classification Observation History (2026-10-17 Oslo)

The Xero chart of accounts now has an explicit append-only classification
history in Silver and Gold. Every retained `xero_accounts` observation carries
its native Xero account class, native fallback classification, mapping method,
and observation-time interval. The older staged `bs_pl`/`fsli_1` labels are
intentionally not adopted: their compatibility columns are emitted as NULL so a
fresh Xero capture has no implicit dependency on legacy mappings. The current
account dimension points to exactly one current classification history row. A
safety assertion rejects duplicate current rows, backward intervals, or
divergence between that pointer and the current account classification.

This is provenance governance, not an invented group-chart policy. The interval
documents when the warehouse saw a source classification; Xero does not supply
a finance-approved source-effective date for a retrospective reporting mapping.
Existing GL is therefore not retroactively reclassified. A later controlled
Norwegian/group-chart policy must be separately approved, versioned, and joined
by its declared effective date.

The account history materialization invocation
`1792190105-13bec5ef-1024-4b07-8b3c-cda392c5caf6` and safety invocation
`1792190186-8ccc96b9-3306-4e13-9a14-b995267bf7b1` both succeeded. The retained
history has 2,852 source observations for 886 accounts, exactly 886 current
pointers, zero backward intervals, and zero current-pointer mismatches.
The hardened invocation `1792191032-fe65f742-43dc-4eb7-a8b4-7effe359858a`
also succeeded: all 886 current accounts now use only the native account-class
fallback and zero retained history rows adopt legacy staged mapping values.

## GL Tracking Segment Bridge (2026-10-17 Oslo)

`xero_gold_finance.bridge_general_ledger_tracking_option` is the safe Xero
counterpart to a reporting-segment bridge. Its grain is one populated source
tracking slot (`1` or `2`) per journal line. It resolves the source
tenant/category/option-name to a Xero tracking option only when there is one
candidate; it retains unresolved or ambiguous states and has no financial
measure. It must therefore be used as a filter/grouping relationship, not as a
financial allocation or a way to sum a journal line twice.

The materialization and safety invocation
`1792190473-2ae1c2be-3169-4eb7-a403-7e892293ac6b` succeeded. It reconciled
81,929 populated source slots to 81,929 bridge rows, with zero missing/extra,
unresolved, ambiguous, or additive-measure rows. This deliberately does not
create a Xero business-unit, department, project-P&L, or NOK FX model.

## Native FX Evidence Coverage (2026-10-17 Oslo)

`xero_gold_finance.fact_source_exchange_rate_observation` now records native
Xero rate evidence at `tenant_id + source_entity_type + source_entity_id`
grain for customer/supplier invoices, customer/supplier credit notes,
customer/supplier payments, and bank transactions. It retains currency, base
currency when supplied, source rate, source amount, bank amount when supplied,
and observation time. The source amounts are explicitly non-additive evidence,
because documents, payments and bank transactions can represent overlapping
economic activity. It deliberately does not infer rate direction, forward
fill a daily currency pair, calculate NOK, or overwrite accounting amounts.

The materialization and safety invocation
`1792190658-5013fd9f-56ff-4071-aa35-392855e22319` succeeded. The current
evidence contains 32,430 source entities. All 845 cross-currency documents and
payments have a native source rate; the fact exposes zero direction-known and
zero NOK values. This is a readiness/quality baseline only. An authoritative
daily FX source and approved direction/date policy remain required before
`amount_nok` can be populated anywhere in the Xero warehouse.

## Local PKCE and aged-report validation (2026-10-18 Oslo)

The CoreSystem local Compose overlay now binds the onboarding BFF to
`127.0.0.1:5002` and advertises the registered Xero redirect
`http://localhost:5002/api/xero/callback`. The login route returns that exact
redirect with PKCE S256; the local server refreshed all seven existing tenant
grants without exposing access or refresh tokens. This validates the local
callback configuration and existing grants, not a new interactive consent.

The one-tenant invoice/payment canary `af3ef9cb-46bd-42b7-adeb-342b12fede80`
returned 12 invoices and 4 payments with no Xero errors. The full Accounting
run `f69def37-6de9-44a6-b6f2-6169b1bdbaa2` then returned 29 endpoint
outcomes, 42 pages, and 576 records. The aged-contact run
`aaf5c3f4-0ee9-450a-b36b-94fc66316be7` discovered 61 contacts in a second
tenant and fetched both report types for every contact: 123 outcomes, 124
pages, and 183 records. Run-scoped, pinned-object replay reconciled both
larger runs exactly against manifest page and record counts. The loader did
return Cloud Run 429 responses during these bursts because it has one instance;
the source objects remain in GCS, but automatic delivery at this fan-out is
not yet reliable without replay.

Dataform invocation `1792299492-5958bbc4-e01d-47a2-a225-c6eab89acb91`
succeeded after a coverage-model correction. Endpoint coverage now selects
the latest complete standard Accounting, tenant-wide reporting, and
aged-contact cohorts independently. A Contacts discovery inside an aged run
can no longer replace the standard cohort's Contacts observation and falsely
mark standard Accounting not ready. The latest attempt is retained in separate
health fields; the current summary has zero failed latest tenant-operations.
All seven tenants have standard Accounting
release state `ready`; the aged-report tenant and the earlier proven tenant
have aging portfolio state `ready`. The new aged run has 61 manifested requests
and 61 source snapshots for each report, and Gold contains 288 receivable and
2,559 payable amount rows for that tenant, all with parse-valid amounts.

At this earlier checkpoint, five tenants had no contact-scoped aged AR/AP
run. Their then-current coverage state was `not_attempted`, not a Dataform
transformation failure. The later source-to-Gold gap check below records
their completed and reconciled runs. Separately, 97 documented GET
operations per tenant have no Gold contract; many Accounting detail, document,
history, and attachment routes plus Finance and Bank Feeds are not attempted.
`ready` here certifies the current modeled cohort and reconciled delivery, not
a complete historical backfill of every Xero endpoint. A zero-contact aged
tenant also needs explicit empty-parent outcome handling before its two report
routes can be certified without report calls.
The `xero_datamart.finance_reporting_readiness` control remains gated for all
seven tenants by approved group-chart mappings and a fresh external NOK FX
source; Xero payloads alone cannot satisfy those governance inputs.

## Source-to-Gold gap check (2026-10-18 Oslo)

After local PKCE reconnection, the seven-tenant three-day Accounting run
`5cf59d33-0ca7-44bf-9419-12fbf1406cda` completed 203 outcomes, 221 pages,
and 787 records with no Xero errors. Run-scoped replay reconciled all 221
pages and 787 raw rows in staging and Bronze. The same-day six-report run
`c2d9fddc-89f0-4892-b761-559c4cc2379e` reconciled 42/42 report pages;
all seven modeled Accounting cohorts then reported `ready` and
`fresh_within_24_hours` after Dataform execution.

An unwindowed seven-tenant sweep of expense claims, prepayments, receipts,
and budgets (`c487180c-8e34-4818-a41a-68469f663c4b`) reconciled 35/35
pages and 22/22 records. The standard incremental run had zero receipts, but
the sweep found eight older receipts in two tenants. Dataform now has eight
matching Bronze, Silver, and Gold receipt rows. All eight source receipts
have empty `LineItems`, explaining the zero-row receipt-line facts. Expense
claims and prepayments returned zero records in every connected tenant, so
their empty facts reflect current source evidence, not a transform failure.
Each tenant returned a budget header, but every list and direct budget
response had an empty `BudgetLines` array; the budget-balance fact is
therefore empty by source evidence. These observations are scoped to the
connected organisations and queried endpoints, not a claim that such data
cannot exist in other Xero organisations.

A bounded invoice-detail canary
`916b732b-8aa4-4a54-a604-916c48d6e79d` returned 11 list invoices and
11 direct responses. Seven direct responses contained one embedded attachment
each, while the list source exposed only `HasAttachments`. A separate direct
`getInvoice` canary also returned one attachment without a parent metadata
tag. The isolated Dataform project now validates singleton invoice identity,
complete array shape, attachment IDs, and the `HasAttachments` flag before
inferring absence. It retains the raw response history, exposes detailed
metadata in Silver, and publishes only identifiers, MIME type, size, and
current/removed state in Gold. Five additional one-invoice detail canaries
were replayed and materialized; the latest workflow has 13 matching
Bronze/Silver/Gold attachment rows. File bytes are not downloaded; the dedicated attachment-list routes
are still separate, unattempted coverage.

The bounded aged-contact run `1d6dfa64-e57d-4434-8237-9df2e38c7ca5`
covered two more tenants. Its 520 outcomes, 524 pages, and 777 records
reconcile exactly after pinned-object replay. Three subsequent single-tenant
runs (`73104b1e-150c-4ad5-9ec0-9429d6c97135`,
`7eeee842-4056-46c6-b066-a944b8f872ee`, and
`8f60de5f-06f9-420a-9ef3-04ca3d4ba4a6`) reconciled 460/460,
629/629, and 1,979/1,979 pages respectively. The earlier combined
attempt had a GCS upload failure and is not counted as complete. The local
uploader was rebuilt with bounded create-only retries and pinned-object
validation. Dataform invocation
`1792341838-3e90767b-5fe5-4799-a87a-fef9e722da3c` succeeded, leaving
all seven tenants' modeled Accounting and aged-contact portfolios `ready`.

The current catalog declares Gold contracts for 52 of 150 documented GET
operations; two Payment Services routes remain deliberately out of scope.
The 96 operations without a Gold contract are 11 Accounting attachment-list,
27 detail, 26 document-byte/PDF, 16 history, and three optional report routes,
plus four Bank Feeds, eight Finance, and one Identity route. This is an API
coverage backlog, not 96 missing business facts: every currently
authoritative operation has a declared Gold contract, but most of those
side routes have never been attempted. Finance, Bank Feeds, and Identity
require the dedicated restricted raw bucket, which is not configured in the
local container. They must not be sent to the ordinary Accounting bucket.

The seven-tenant finance reporting gate remains separate from Xero source
completeness. All 886 current accounts have Xero-native financial classes,
but none has a finance-approved group-chart mapping. The external FX feed is
nine days old and the NOK conversion policy/rate coverage is incomplete.
Neither Norwegian group-chart mappings nor an authoritative NOK rate series
can be inferred safely from Xero's transactional payloads.

### Full invoice and payment sweep

The seven-tenant open sweep `8f4fae2b-6ab1-4607-946c-e6c9851a2e28`
returned 18,067 invoices and 17,709 payments across 14 successful
tenant-endpoint outcomes. Its 377 GCS pages and 35,776 records reconcile
exactly to both staging receipts/raw rows and the Bronze run reconciliation.
Pinned-object projection replay refreshed the typed staging tables to the
same 18,067 invoice and 17,709 payment identities. No typed row was inferred
from a missing raw response.

Before the sweep, the Gold currency audit contained 192 unresolved payments
(70 customer, 122 supplier) referencing 185 absent typed invoice headers.
All 192 payment payloads and all 185 invoice headers were returned in the
open sweep with explicit Xero currency codes. Dataform invocation
`1792343040-2c6e03c4-e988-4387-8e91-10b2f7e65d6f` succeeded: the
currency audit is now empty, with zero observed header-versus-payment
currency conflicts and zero payment/invoice type mismatches. Gold contains
4,093 customer payments (3,887 reportable) and 13,573 supplier payments
(12,207 reportable). The remaining nonreportable statuses are retained,
including Xero-deleted rows; they are not silently removed.

The payment model also retains Xero's `Invoice.CurrencyCode` from the latest
raw Payment response. It uses this only when the raw and typed payment
observations align, and rejects malformed or conflicting currencies. Gold
exposes the currency source and raw/typed mismatch flags; it does not use a
payment amount or Xero exchange rate to guess a missing invoice currency.

The currently empty materialized facts are source-evidenced: zero expense
claims and prepayments in seven unwindowed tenant sweeps, no `BudgetLines`
in seven budget list and direct-detail responses, and no `LineItems` in the
eight returned receipts. They are not evidence that those entity types can
never exist in other organisations. All 52 current authoritative GET
operations are either `succeeded` or source-proven `not_applicable` for
each tenant. Another 96 documented GET side routes per tenant remain
`not_attempted`, and two Payment Services routes remain out of scope; full
catalog completeness must not be inferred from the Accounting release state.
Finance reporting remains blocked for all seven tenants by group-chart
mapping approval and a current authoritative NOK FX feed.

### Source-to-Gold line and payment reconciliation (2026-10-18 Oslo)

The unwindowed credit-note run `6c6dd86e-6e09-46a6-b1f2-b0e2b5d6322a`
returned 496 headers over 16 pages with no API errors. It recovered five
credit-note parents referenced by non-invoice payments. Silver and Gold now
retain 43 non-invoice payments (32 supplier credit-note, 10 supplier
overpayment, one customer credit-note payment); 36 are reportable, seven are
Xero-deleted. All 449 credit-note allocations were observed with their parent
and have resolved invoice, type, and currency references. These live payloads
nest the parent under `Invoice`; other documented payment shapes and refund
types still need an observed-payload validation before they become reportable.
Dataform invocation `1792343617-c0ab6c58-34a6-46d5-99dc-8ae693d998c9`
succeeded.

The full invoice sweep exposed seven older typed invoice lines absent from
complete current raw `LineItems` arrays across four invoices. They remain in
Silver and Gold with `is_removed_from_source = TRUE` and NULL additive Gold
measures; no row was deleted. The 31,802 current lines match raw IDs and
financial/classification values. The Silver invoice-line ambiguity audit has
zero rows, and no removed invoice line is reportable. The stricter raw-parent
and value-alignment workflow
`1792344633-9c126f27-900f-4ad7-9ce7-28aa9b24dc71` succeeded.

The seven-tenant quote/purchase-order open sweep
`3f4bd8bc-b050-41f1-ae0b-90c641069cb9` returned 1,329 quotes and
1,080 purchase orders: 14 successful outcomes, 43 pages and 2,409 records.
All 43 receipts and 2,409 immutable raw records reconciled to the manifest.
Pinned-object replay rebuilt the typed current projections. Every current
raw parent had an explicit `LineItems` array, a matching typed observation,
and the same line count. The typed merge still retained two older quote lines
and four older purchase-order lines absent from these raw arrays. Silver and
Gold now mark those six rows removed, retain their source values, and exclude
them from additive Gold pipeline/commitment measures. Current raw-versus-typed
counts are 3,917 quote lines and 2,413 purchase-order lines, with zero
ambiguous or value-mismatched current rows. Dataform invocation
`1792345402-f1c589cb-0fb5-4b78-bf26-d5c19747d83c` and the new
commercial-line reconciliation assertion succeeded.

At this checkpoint, all 364 authoritative tenant-operation rows (52 GET
operations across seven tenants) have declared Gold contracts. This is not
the complete 150-operation API catalog: 96 side operations per tenant remain
unattempted/unmodelled, and two Payment Services operations are intentionally
out of scope. The only empty materialized Gold finance business facts are
expense claims, prepayments and their children, budget balances, and receipt
lines. The seven-tenant unwindowed source sweep returned no expense claims or
prepayments, seven budget responses with empty `BudgetLines`, and eight
receipts with empty `LineItems`; the eight receipt headers are present in
Gold. Both Gold currency and unresolved-dimension audits are empty. The
remaining group-chart and NOK conversion gates require governed mappings and
an authoritative external FX policy/feed, not invented Xero values.
