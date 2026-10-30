# Xero Dataform warehouse

This is the isolated Dataform project for `aquatiq-datavarehus`. It does not
share the Visma-oriented `Dataform/workflow_settings.yaml` or the older Xero
models in that mixed workspace.

- Dataform repository: `Xero`, region `europe-north1`, workspace `Dev`.
- BigQuery location: `europe-north2`.
- Source: `dw_staging_xero`. It contains the copied typed current snapshot and
  the active natural-name raw-history contract for the `main` Xero generation.
- Bronze: seven protected incremental typed header observations, 38 current typed
  views, and immutable raw record, collection, child, and receipt history in
  `dw_bronze_xero`.
- Silver: company, organisation user, account, contact, branding theme, item,
  quote/line, purchase order/line, invoice/line, payment and payment-to-invoice
  application, credit-note/line/allocation, bank transaction/line, bank transfer,
  journal-line, native budget account-period balance, and non-additive
  report-snapshot, and invoice-reminder configuration models in `dw_1_silver_xero`.
- Gold: finance, cash, and VAT-control facts/dimensions in `xero_gold_finance`, company,
  organisation user, branding-theme and role-neutral contact dimensions in
  `xero_gold_common`, item and quote facts in `xero_gold_sales`, and purchase
  order facts in `xero_gold_procurement`. Native budget balances are conformed
  in `xero_gold_finance` without combining their distinct Xero source measures.
  Invoice-reminder configuration is a source setting, not a receivables measure,
  and retains explicit empty-state evidence.
  Organisation actions are exposed as an integration-connection capability bridge;
  they are not organisation-wide policy or human-user entitlements.
- Open items: `fact_accounts_receivable_open_item` and
  `fact_accounts_payable_open_item` retain invoice/bill debit balances and
  unapplied credit-note and overpayment balances as separate signed documents.
  They use the observation date only for document ageing. They are not Xero
  aged-report snapshots and cannot certify a tenant-wide close position.
- Contact aging reports: `fact_contact_aging_report_run`,
  `fact_contact_aging_report_line`, and `fact_contact_aging_report_amount`
  preserve Xero's contact-scoped AR/AP report evidence. Gold exposes separate
  AR/AP report facts and `*_aging_by_contact` marts. A tenant report run is
  complete only when contact discovery, keyed source requests, and keyed report
  snapshots agree. A header such as `Due USD` proves `USD`; unnamed measures
  stay currency-unidentified and are never converted or silently combined.
- Data marts: finance activity, sales pipeline, procurement commitments,
  project operations, native-currency working-capital open items, VAT activity
  control, daily cash movement, and non-additive cash-position snapshots in
  `xero_datamart`.
- Assertions: `dw_bronze_xero_assertions` and `dw_1_silver_xero_assertions`.
- Execution identity: `xero-dataform@aquatiq-datavarehus.iam.gserviceaccount.com`.

The current release status, tenant-level source-to-Gold readiness gate, and
operating commands are documented in
[`docs/XERO_GCP_MAIN_MIGRATION.md`](../../docs/XERO_GCP_MAIN_MIGRATION.md).

The incremental bronze tables preserve observed typed rows by tenant, record,
and version hash. `payment_versions` marks older observations as superseded;
`fact_payment` and `fact_customer_payment` keep Xero `DELETED` rows with an
`is_deleted` flag. Account `ARCHIVED` and invoice `DELETED`/`VOIDED` remain
visible with distinct flags. Silver also marks rows absent from the current
typed staging snapshot as `is_missing_from_staging`, without claiming Xero
deleted them. Source-key and output-grain assertions detect duplicate or
missing identifiers. The tenant ID is the organization key, not a
Visma-style business unit.

The typed bronze views expose only the current copied staging snapshot. The
raw-history Bronze models are intentionally separate from those business views:

| Bronze model | Source table | Purpose |
| --- | --- | --- |
| `xero_raw_record_history` | `raw_records` | Immutable original response payloads, lifecycle values, run and receipt provenance. |
| `xero_raw_record_source_duplicates` | Raw record history | Audit-only marker for exact or conflicting physical replay duplicates; it never deletes source evidence. |
| `xero_raw_record_history_canonical` | Raw record history plus duplicate marker | One deterministic record per immutable source version; this is the raw record relation used by downstream Bronze, Silver, and Gold models. |
| `xero_raw_collection_history` | `raw_collections` | Explicit observed child-array membership and cardinality. |
| `xero_raw_child_history` | `raw_children` | Immutable original child payloads, including source position fallback keys. |
| `xero_raw_receipt_history` | `ingest_receipts` | Pinned raw-object identity, response status, expected and observed counts. |
| `xero_raw_record_current` | Raw record history | Latest observed source record only; it does not infer deletion from absence. |
| `xero_raw_collection_current` | Raw collection history plus current parent | Explicit child collection on the latest observed parent; an omitted collection remains unknown, not empty. |
| `xero_raw_child_current` | Raw child history plus current collection | Current, superseded, removed, and source-deleted child lifecycle without deleting historical child rows. |
| `xero_raw_ingestion_health` | Raw receipts and records | Tenant/endpoint delivery evidence and freshness facts. |
| `xero_raw_run_reconciliation` | Manifest outcomes, raw receipts, and raw records | Per-run completion evidence; an outcome remains `awaiting_raw_delivery` until page and record counts reconcile. |
| `audit_direct_response_projection_versions` | `xero_raw_direct_response_history` plus pinned receipt history | Silver audit of every raw direct-GET response. It exposes the request descriptor, GCS object pointer, HTTP metadata, and raw payload provenance; raw receipts remain the authority. |

`xero_current_accounting_warehouse_release_readiness` is the tenant-facing
release contract. It keeps base Accounting, source freshness, and the optional
contact-scoped AR/AP aging portfolio as distinct states: a tenant may be
source-complete for base Accounting while aging reports are not yet certified.
`source_freshness_state` records observed source age at materialization;
`is_fresh_within_default_48_hours` is a visible default, not an implicit
finance close policy. `aging_portfolio_release_state` requires both Xero aged
report endpoints to share a coherent authoritative run with raw reconciliation
and Gold contracts.

The raw-history assertions reject duplicate source identities, raw rows without
their receipt, mismatched receipt record counts, mismatched child collection
counts, and impossible child lifecycle states. A child is only marked
`removed` when a later parent response explicitly contains that child array,
the historical child has a stable native ID, and that ID is absent. Positional
fallback keys remain historical or unverified rather than being falsely marked
deleted. `raw_records`, `raw_collections`, `raw_children`, `ingest_receipts`,
and `raw_control` are staging infrastructure, not business tables. They have
natural Xero names and no dataset labels.

Direct lookup routes supplement, but never replace, list-controlled lifecycle.
`getAccount`, `getProject`, `getContactGroup`, `getTask`, and `getTimeEntry`
retain a direct-detail Bronze observation and a same-run Silver reconciliation.
Contact-group member counts
are specifically audit-only: the direct response can expose a different member
array from `getContactGroups`, so Gold reports the disagreement but uses only
the list route for the membership bridge.

Typed Project task and time-entry observations carry a projection schema
revision. When the typed projection grows, the corrected revision is appended
beside earlier observations of the same immutable raw version; reconciliation
chooses the latest projection revision and keeps the older row auditable.

Project observation tables use `onSchemaChange: "FAIL"`. They are protected,
append-only history tables, so a Project projection change must be applied as
one explicit, reviewed BigQuery migration before Dataform is deployed. This
avoids Dataform issuing one `ALTER TABLE` for each new field during a workflow
and hitting BigQuery's per-table metadata update quota. The initial migration
for the current Project projection adds all required columns in one statement
per table; it never rewrites, replaces, or deletes historical observations.
It is versioned as `migrations/20261017_project_observation_schema.sql` and is
uploaded to the Dataform repository for audit; execute it with BigQuery before
releasing a projection that adds a column.

`xero_raw_run_reconciliation` is intentionally at
`run_id + tenant_id + endpoint` grain. A detail API can create many manifest
outcomes for one endpoint, while the durable receipt contract currently has no
per-outcome correlation key. The model aggregates those outcomes before it
compares exact pages and records to receipts and canonical raw records; it
exposes `outcome_count` so this scope is explicit.

## Endpoint-to-warehouse contract coverage

`xero_endpoint_warehouse_coverage` and its tenant summary answer two separate
questions: whether Xero delivered an operation and how far that operation is
declared through the warehouse. `is_authoritative` remains a raw-delivery
fact. `warehouse_contract_state` independently records `not_modelled`,
`raw_only`, `bronze`, `silver`, `gold`, or `out_of_scope`, and the model lists
the exact Bronze, Silver and Gold relations. An authoritative response is never
presented as a completed business data product by accident.

As of October 18, the contract declares 52 Gold operations, all authoritative
for each of seven tenants (364 tenant-operation rows). No delivered operation
is presented as raw-only or unmodelled. The two Payment Services operations
remain explicitly out of warehouse scope. The remaining 96 documented API
operations are visible as `not_modelled`, not silently treated as absent or
complete. Most of those are supplementary detail, history, attachment, binary
document, gated Finance/Bank Feeds, or Identity routes rather than a missing
copy of a core accounting fact.

`not_applicable` is a constrained completeness result, not a synonym for an
empty or missing latest request. `xero_asset_run_certification` requires one
successful, fully reconciled `DRAFT`, `REGISTERED`, and `DISPOSED` asset list
snapshot in the same run before an empty asset inventory can suppress
`getAssetById`. Project child routes reuse `xero_project_run_certification`,
which verifies the complete parent inventory, per-project child request sets,
snapshots, membership, and raw observations in one run. Endpoint coverage
retains `not_applicable_evidence_run_id`,
`not_applicable_parent_endpoint`, and `not_applicable_evidence_type` so an
empty parent cannot be confused with an untested or failed child route.

Raw history began with the active main generation
`a3e6e7e5-04c1-4574-a63f-e0863ea9cef1`; it never imports, replaces, or deletes
copied staging data. The Cloud Function commits a pinned raw receipt, records,
collections, and children before updating compatible collection typed current
projections. Catalogued direct Accounting GET responses remain raw-first: their
Dataform models reconcile pinned raw evidence to same-run collection state, and
they are not treated as mutable current entities. The first standard Accounting run reconciled all 189 tenant/endpoint
outcomes, 276 pages, and 4,604 raw records. Silver and Gold continue to read
the compatible typed contract while raw coverage and tenant-level reconciliation
accumulate for the remaining business domains.

## Business crosswalk

| Norwegian concept | Visma production | Xero model | Xero interpretation |
| --- | --- | --- | --- |
| Selskap | `visma_gold_common.dim_company` | `xero_gold_common.dim_company` | One Xero tenant/organization; no BU segment. |
| Kontoplan | `visma_gold_finance.dim_account` | `xero_gold_finance.dim_account` | Xero account ID/code and native account class. A future Norwegian/group-chart mapping must be governed separately; legacy staged labels are not adopted. |
| Rapporteringssegment | Visma BU/subaccount bridge | `xero_gold_finance.bridge_general_ledger_tracking_option` | One row per populated Xero GL tracking slot. It is a generic tracking relationship, not a BU, department, project allocation, or additive financial fact. |
| Budsjett | Budget dimension and account-period fact | `xero_gold_finance.dim_budget` and `fact_budget_balance` | Budget metadata comes from the list route; direct budget detail preserves native `Amount` and `UnitAmount` separately per account and period. |
| Kunde / leverandor | `dim_customer` / `dim_supplier` | Same names in `xero_gold_finance` | One contact can have both roles. |
| Kontakt / prospekt | Contact master | `xero_gold_common.dim_contact` | All staged Xero contacts; role-specific keys appear only when the contact has that role. |
| Kundefaktura / leverandorfaktura | `fact_customer_invoice` / `fact_supplier_invoice` | Same names in `xero_gold_finance` | `ACCREC` / `ACCPAY` document types. |
| Apen kunde- / leverandorpost | Current AR/AP and ageing outputs | Current open-item facts, report-aged AR/AP facts, `xero_datamart.working_capital_open_items`, and `*_aging_by_contact` | Document-current balances remain distinct from Xero aged-report snapshots. Contact report data is certifiable only with complete keyed request coverage; source header currencies are preserved and unlabelled measures are not converted. |
| MVA-kontroll | Tax activity by source line | `xero_gold_finance.fact_vat_activity` and `xero_datamart.vat_activity_control` | Control and audit output only, grouped by tenant, fiscal period, Xero tax classification, and native currency. It excludes lifecycle-incomplete lines from totals but retains them in Gold; it is not a statutory MVA-melding. |
| Fakturalinje | Invoice-line facts | Customer and supplier invoice lines | `LineItemID` is the key; inclusive tax basis is normalized to net. |
| Innbetaling / utbetaling | Payment facts | Customer and supplier payment facts | `ACCRECPAYMENT` / `ACCPAYPAYMENT`; bank amount stays separate. |
| Kreditnota | Credit-note facts | Customer and supplier credit-note header/line/allocation facts | `ACCRECCREDIT` / `ACCPAYCREDIT`; no assumed sign reversal. Allocations link credits to invoices. |
| Hovedbok | `fact_general_ledger` | `xero_gold_finance.fact_general_ledger` | Xero `Journals` signed `NetAmount`: positive debit, negative credit. |
| Vare/tjeneste | Item dimension | `xero_gold_sales.dim_item` | Xero item master; not inventory movements. |
| Tilbud | Sales pipeline | `xero_datamart.sales_pipeline` | Xero quotes, not Visma sales orders or booked revenue. The mart separates source status/value from the effective non-expired pipeline measure. |
| Innkjopsordre | Purchase commitments | `xero_datamart.procurement_commitments` | Open `AUTHORISED` commitments, not supplier invoices or booked costs. |
| Prosjektleveranse | Project operations | `xero_datamart.project_operations` | Xero project delivery, task, time and billing-workflow measures; not project P&L, cost allocation, margin, or a Visma project dimension. |
| Banktransaksjon | External cash movement | `xero_gold_finance.fact_bank_transaction`, `fact_bank_transaction_line`, and `xero_datamart.cash_movement_daily` | Only authorised `SPEND`/`RECEIVE` contribute signed external cash. Daily movement remains separate per native currency and is never a bank-balance calculation. |
| Bankoverforing | Internal transfer | `xero_gold_finance.fact_bank_transfer` | Source amount retained; no external cash measure. |
| Bankposisjon | Bank Summary source snapshot | `xero_gold_finance.fact_bank_balance_snapshot` and `xero_datamart.cash_position_snapshot` | Exact source report-cell grain. Values are non-additive within and across snapshots and must never be added to daily movement. |
| Xero-bruker | Organisation user directory | `xero_gold_common.dim_user` | Xero access user, not an employee, department or BU record. |
| Fakturaoppgjor | Payment-to-invoice application | Customer/supplier invoice payment application facts | One payment maps to one native invoice reference; deleted and unresolved links remain visible and non-additive. |
| Fakturaoppsett | Branding theme | `xero_gold_common.dim_branding_theme` | Native Xero document template; not a payment-service integration. |
| Rapportoyeblikksbilde | Balance Sheet, P&L, Trial Balance, Bank Summary, Budget Summary and Executive Summary snapshot facts | Financial report snapshot/cell, financial statement line, bank balance snapshot facts | Explicitly non-additive source report outputs; never sum hierarchy or metric values together, and never mix them with GL totals. |

These Xero outputs do not depend on Visma tables or workbook mappings.
`company_key` is a fingerprint of `tenant_id`; document and account keys are
tenant-scoped. Xero tracking categories are retained, not interpreted as
Visma's Avdeling or BU without a tenant-specific mapping. Physical fields
remain English `snake_case`; the crosswalk supplies Norwegian business terms.

## Finance conformance controls

`dw_1_silver_xero.audit_account_mapping_coverage` shows, by tenant and mapping
method, whether an account has only a Xero-native class or a governed reporting
mapping. `xero_gold_finance.audit_account_mapping_readiness` deliberately
releases a group-chart mapping only when every active account has a
finance-governed mapping. Native `AccountClass` remains useful source context;
it is never relabelled as an approved Norwegian or group chart.

`xero_gold_finance.audit_nok_conversion_readiness` records whether a tenant has
cross-currency source observations, an external market-rate reference, and a
finance-approved conversion policy. The normalized daily bridge reads the
shared `dw_staging_currency.HistoryratesPowerBI` feed and retains its source
date and fill-forward state. Invalid/non-positive rates are excluded; overlapping
effective-dated mapping or policy rows remain visibly ambiguous and cannot be
selected as an approved value. The bridge itself is not an approval to convert
financial facts, so `amount_nok` stays unavailable until Finance defines the
endpoint-specific policy. `xero_datamart.finance_reporting_readiness` combines
these independent controls for downstream consumers: it distinguishes
source-native reporting from finance-approved group-chart and NOK reporting.

To move this control from `pending` to `ready`, add two governed inputs:

1. Finance-approved rows in `xero_reference.xero_account_reporting_mapping`,
   keyed by `(tenant_id, account_id)` and effective-dated with mapping
   source/version and approved FSLI and financial flags.
2. Finance-approved rows in `xero_reference.xero_nok_conversion_policy` for
   the normalized daily `currency -> NOK` bridge. The policy explicitly covers
   rate fill-forward treatment; the bridge carries rate date and source. The
   Visma model is the implementation pattern, but its mappings and rates are
   not copied into Xero.

When populated, `amount_in_currency` is the Xero document amount. Gold invoice `amount`,
payment `payment_amount_base`, and NOK amounts are intentionally NULL until
currency-rate and bank-amount reconciliation is proven per tenant. Journal
debit/credit is the native signed `NetAmount` split, with organization
`base_currency` as functional currency. Manual journals are already in
`Journals` and are not unioned again.

Gold `source_amount_in_currency` and `source_payment_amount` retain Xero's
typed source amounts even for deleted, voided, draft, or unresolved records.
Additive `amount_in_currency` and `payment_amount` are populated only when
`is_reportable` is true. For invoices/credits this means `AUTHORISED` or
`PAID`; invoice lines also require an unambiguous child set. Payments require
`AUTHORISED` and a known invoice currency. The seven-tenant unwindowed invoice
and payment sweep resolved the 192 payment currencies missing immediately
before that run; `xero_gold_finance.audit_unresolved_payment_currency` is now
empty. Forty-three payment rows have non-invoice Xero payment types and live in
the separate `fact_non_invoice_payment` contract; 36 are reportable. Do not
sum `source_*` fields for financial reporting.

After the full invoice sweep, seven older typed invoice lines were absent from
complete current raw `LineItems` arrays. Silver and Gold retain them with
`is_removed_from_source`; their additive measures are NULL. The current raw
set has 31,802 lines, all aligned with typed values. The Silver invoice-line
ambiguity audit and Gold unresolved-dimension-key audit are empty. Absence
from a sparse or stale source response never proves a line was removed.

Credit-note lines and allocations are modeled separately from headers. Line
measures require the staged child count and net subtotal to reconcile to the
credit-note header; a mismatch leaves source rows visible with additive
measures NULL. Allocation measures require a reportable credit, a linked
authorised/paid invoice of the matching customer/supplier type, matching
non-null source currencies, a known non-deleted allocation state, and an
allocation observed in the same source pull as its credit-note header. The
fresh seven-tenant credit-note sweep returned 496 headers (113 customer,
383 supplier). All 449 currently staged allocations were observed with their
parent credit note; none has an unresolved invoice, currency mismatch, or
invoice-type mismatch. Xero omitted `IsDeleted` on the copied allocations,
so a NULL source flag is treated as
not deleted; unparseable non-NULL values are flagged and excluded. Source
amounts retain their original sign, without invented NOK conversion. A
same-count, same-subtotal stale line set remains possible without a shared
original-response identifier.

The older October 15 copied-snapshot counts are historical and must not be
used as the current source inventory.

Quotes and purchase orders retain all statuses, including `DELETED`. Quote
pipeline amounts are populated only for `SENT`/`ACCEPTED` rows; purchase
commitment amounts only for `AUTHORISED` rows. Line-level amounts additionally
require an unambiguous child set. Header and line measures require both the
declared line count and non-null line net subtotal to reconcile to the header
within 0.02 in source currency. Description-only lines with NULL `LineAmount`
remain visible with NULL line measure when the other lines reconcile. The
full quote/purchase-order sweep found two older quote lines and four older
purchase-order lines absent from complete current raw arrays. All six are
retained with `is_removed_from_source` and NULL additive measures. Current
raw-versus-typed line counts and values reconcile: 3,917 quote lines and
2,413 purchase-order lines, with zero ambiguous current line sets. The bank
transaction cash measure excludes transfers, overpayments, and deleted transactions; it
does not infer currency for bank transfers. The 170 bank-transaction rows
flagged with missing subtotals are transfer-side records, not external cash
flows. These are source-specific
interpretations, not one-to-one Visma fact equivalents.

The initial gold expansion completed successfully in Dataform workspace
`Xero/Dev` after the contact and child-set review. Counts matching typed
staging were 2,206 contacts, 2,178 items, 1,319 quotes and 3,868 quote lines,
1,079 purchase orders and 2,413 lines, 4,854 bank transactions and 5,022
lines, and 84 bank transfers. `gold_expansion_grain` and
`gold_expansion_measure_safety` passed. These counts establish modeled
current-snapshot parity, not complete Xero endpoint coverage or historical
reconciliation.

Bronze `version_id` hashes the entire typed observation, including sync
timestamps. It identifies observations, **not** distinct Xero business
changes; `payment_versions.is_superseded` likewise means a later observation
exists. Do not interpret repeated syncs as multiple business changes.

## Projects operational model

Xero Projects now follows the same Bronze, Silver, and Gold ownership model as
the production Visma warehouse, while remaining honest about the different
source contract:

| Layer | Xero Projects model | Rule |
| --- | --- | --- |
| Bronze | `xero_project_*_observations`, list snapshots, list memberships, and run certification | Every response and membership observation is append-only and pinned to its raw receipt, run, version, and tenant. Empty child lists remain explicit snapshots rather than inferred absence. |
| Silver | `dim_project`, `dim_project_user`, `dim_project_task`, `fact_project_time_entry` | Reads only fully certified Project runs. Current entities are marked `is_current`; entities missing from a later complete snapshot are retained with `is_removed`, `removed_in_run_id`, and source observation history. |
| Gold common | `xero_gold_common.dim_project` | Conformed project master with company and contact keys. A tenant is the company boundary; no business unit is invented. |
| Gold operations | Project user/task dimensions, time-entry fact, and `fact_project_operational_snapshot` | Supports delivery, task, time, source billing summaries, and invoicing backlog analysis. The snapshot inherits the resolved project/contact keys; project, task, user, and time-entry relationships are assertion-checked. Gold keeps lifecycle markers and every source Money value retains its own currency unit; native duration stays in minutes. |

The certification gate deliberately keeps a quota-truncated or otherwise
partial run out of Silver and Gold. The validated seven-tenant Projects
baseline `91fe3f5c-9fde-474a-a031-79237655d91e` completed with 376 source
outcomes, 377 response pages, 231 source records, and no failed or partial
requests. It certified seven current tenant runs and materialized 181 projects,
43 users, three tasks, and four time entries consistently through Bronze,
Silver, and Gold. Five tenants had a verified empty Project list; their zero
task and time collections are certified only as derived empty-state evidence,
never as fabricated raw child responses.

A baseline run must complete with `sync_runs.status = 'succeeded'`, an
`ended_on_empty_page` outcome for every required request, and exact membership
counts for `projects_projects`, `projects_users`, `projects_tasks`, and
`projects_time`. Use `xero sync-extended --family projects --no-details` as the
daily-safe baseline. Project/task/time detail routes are enrichment; they are
not needed for the certified operational model and should be scheduled
separately under Xero's rate limit.

`getProject` direct detail is now an append-only Bronze observation stream and
is reconciled in Silver against the same-run `getProjects` collection payload.
The current raw history contains 327 direct-detail observations across two
tenants and two runs, representing 181 distinct projects; every observation has
an exact same-run collection counterpart. Gold project outputs expose
`has_direct_detail_evidence` and `is_direct_detail_reconciled`. A direct lookup
never marks a Project current, removed, or deleted: only a fully certified
collection snapshot can change lifecycle state.

Unlike Visma, Xero Projects does not provide project-linked ledger
transactions, cost allocation, project budgets, branch, business unit, or
margin source data. `fact_project_operational_snapshot` is therefore not a
project P&L and contains no invented revenue, cost, income, margin, or NOK
conversion measure. A later accounting-side project-reference mapping would
need its own documented reconciliation before any financial project fact is
created.

This is **not yet** a complete original-API raw history. Earlier versions
cannot be recovered from the copied, upserted staging tables. A record missing
from a later typed extract cannot be interpreted as a Xero deletion. The active
raw generation retains every observed source payload and receipt from its first
fresh delivery onward, but it cannot recreate source history before that point.
The business models do not claim coverage of all 150 documented Xero GET
operations.

## Current business-model coverage

The current graph contains 629 actions: 249 tables and 380 assertions. It
follows the Visma production shape while keeping Xero-native
semantics and source grains:

| Layer | Current Xero coverage | Measure and lifecycle rule |
| --- | --- | --- |
| Bronze | Versioned typed observations, child snapshots, raw receipt/history contract, and tenant/endpoint coverage facts | Observations are retained; absence never means deletion. |
| Silver | Company, organisation users, fiscal periods, contacts, contact addresses, contact phones, contact groups/memberships, branding themes, chart of accounts plus account-classification observation history, currency, tax rates and tax activity, tracking, budget metadata and account-period balance detail, AR/AP, cash, payment applications, GL, sales, procurement, batch payments, overpayments, linked transactions, repeating invoice templates, report snapshots/cells, statement lines, and bank-summary values | One Xero tenant is one company. Fiscal periods are derived from the organisation's source fiscal year-end configuration; no artificial business-unit dimension is created. |
| Gold common | Company, organisation user, branding theme, fiscal period, contact, contact address, contact phone, contact group/membership, currency, conformed date, tracking category, and tracking option dimensions | Every business key is tenant-scoped. The shared date dimension stays calendar-only; fiscal periods are tenant-specific. |
| Gold finance | AR/AP invoices, invoice lines, payments, payment applications, credit notes, credit allocations, overpayments, GL, GL balances, GL-to-tracking-option bridge, bank transactions/transfers, batch payments, linked transactions, recurring templates, tax rates and VAT activity, account-classification observation history, budget metadata and account-period balance detail, and Xero report snapshots/cells/statement lines/bank values | Only explicitly reportable operational rows receive additive measures; source values remain visible on all rows. Report snapshots are explicitly non-additive within their hierarchy or metrics and across snapshots. |
| Gold sales/procurement | Items, quotes/lines, and purchase orders/lines | Quotes are pipeline, purchase orders are commitments, and neither is treated as booked revenue. |
| Data marts | `xero_datamart.finance_activity`, `sales_pipeline`, `procurement_commitments`, `project_operations`, `working_capital_open_items`, `vat_activity_control`, `cash_movement_daily`, and `cash_position_snapshot` | Each has an explicit Xero-native source grain and a relation-level safety assertion. Finance is journal-line grain; pipeline is quote grain; commitments are purchase-order grain; project operations are project grain. VAT control is reportable document-line grain, cash movement is daily external bank-transaction grain, and cash positions retain non-additive Bank Summary cell grain. |

The initial validated copied snapshot contained 30 currency rows, 118 tax-rate
rows, 9 tracking categories, 64 tracking options, 7 budget headers, 612
financial-report snapshots, and 55,859 report cells. All 2,876 percentage
cells retain their parsed numeric source value and are explicitly tagged as
`percentage`; report values remain non-additive within their hierarchy or metrics and across snapshots. Budget headers do not
currently contain budget-line values. The seven current direct budget-detail
responses each preserve `BudgetLines = []`, so `fact_budget_balance` has zero
rows and no budget amount is fabricated. Report cells preserve the original
string and an independently safe parsed numeric value when possible; report
snapshots and cells are flagged non-additive. Batch-payment header and detail
facts are also explicitly non-additive to prevent double counting.
Statement dates distinguish Xero's two- and four-digit year headers before
parsing, then must resolve through the shared date dimension.

The contact reference layer currently contains 3,792 source addresses, 347
phone numbers, 1 contact group, and 4 group memberships. It is the Xero
equivalent of a contact/location and segmentation reference layer, not a copy
of Visma location or business-unit semantics. Address, phone, and membership
rows retain `is_contact_unresolved`; membership rows also retain
`is_group_unresolved`. A missing copied parent is visible through these flags
instead of being removed.

`xero_gold_reference_data_safety` verifies Silver-to-Gold count parity,
tracking-option/category and branding-theme references, future line-level tracking references,
budget-detail flags, report-cell/snapshot relationships, and numeric-value
flags. `xero_budget_balance_safety` verifies budget-detail source parity,
budget/account references, period validity, and measure semantics without
removing lifecycle rows. `xero_gold_extended_finance_safety` verifies expanded
finance grains and measure protections. `xero_gold_date_reference_safety` verifies every populated
Gold fact date key resolves to the shared `dim_date`. The date spine is derived
from observed Xero dates with a one-year reporting runway on both sides and is
calendar-only. `dim_financial_period` derives tenant-specific calendar-month
fiscal periods from Xero's `FinancialYearEndDay` and `FinancialYearEndMonth`
organisation settings when the configured fiscal year-end is the last day of
its month. A non-month-end configuration fails the safety assertion until a
finance-approved period-boundary policy is added. The model intentionally omits
Visma-style AP/AR/GL closure fields because Xero does not expose them.
`xero_gold_document_parent_reference_safety` verifies that every retained Gold
document line, allocation, batch detail, recurring-template line, quote line,
and purchase-order line resolves to its retained Gold parent.
`xero_gold_core_reconciliation` adds a warehouse-wide tenant-scoped control for
45 one-to-one Xero-native Silver-to-Gold relations across common, finance,
sales, procurement, and Projects. It emits one explicit row for every relation
and known tenant, including `0/0` source-complete states, and
`xero_gold_core_reconciliation_safety` blocks a workflow on a row-count
mismatch. Split customer/supplier outputs keep their domain-specific grain
assertions until their filtered source contracts are declared explicitly.
`bridge_company_date_fiscal_period` is append-only and protected: it freezes
the original derived fiscal mapping for each tenant date rather than allowing a
later Organisation setting to relabel historical GL. `dim_financial_period`
groups that immutable bridge.
Every observed organisation configuration version is retained in
`dim_company_fiscal_configuration_history`; Xero does not provide a source
effective date for a retrospective fiscal-period restatement. A configuration
change therefore fails `xero_fiscal_period_safety` until a finance-approved
migration policy is added. The assertion also checks valid fiscal
configuration, contiguous periods, and that every posted GL line resolves to
an explicitly derived tenant fiscal period. Gold GL and the finance activity
mart expose `financial_period_basis` and `is_financial_period_derived`, so
consumers do not confuse this management calendar with a native posting-period
or close-control field.

`xero_gold_contact_reference_safety` verifies Silver-to-Gold row parity for
contact addresses, phones, groups, and memberships, and verifies every
non-flagged membership group key resolves. Contact-parent gaps remain a visible
source-quality state rather than a failed or discarded relationship.

`xero_datamart.finance_activity` is the Xero counterpart to Visma's finance BI
consumption layer. It enriches each posted Xero journal line with a conformed
calendar date, tenant fiscal-period key/year/month/quarter, company, account
classification, and neutral tracking-option keys. Its fiscal fields are marked
`is_fiscal_period_derived` because they are calculated from the source
organisation configuration rather than returned as closed ledger periods. Its
additive measures are only `debit_amount`, `credit_amount`, and
`amount_functional_currency`/`signed_net_amount`, all in the tenant's Xero
functional currency. `amount_nok` is deliberately NULL and
`is_nok_amount_available` false until an authoritative daily FX source is
loaded. The finance-mart assertion checks GL row parity, account resolution,
tracking resolution, and the NOK safety rule.

The first managed run produced 244,091 finance-activity rows across AUD, CAD,
GBP, and NZD functional-currency tenants. It had zero unresolved account keys,
zero unresolved first or second tracking references, and zero populated NOK
amounts, as required by the safety assertion.

`dim_account_classification_history` retains every Xero account observation's
native account class, derived native fallback, mapping method, and
observation-time interval. It intentionally emits the legacy `source_bs_pl`
and `source_fsli_1` compatibility columns as NULL: fresh Xero capture does not
adopt old staged mapping labels. The current `dim_account` points to exactly
one current history row. Its intervals say when this warehouse observed a
classification, not when an unprovided finance policy made it effective; no
historic GL fact is reclassified on that basis. The history safety assertion
blocks duplicate current classifications, backward observation intervals, and
any mismatch between the current account dimension and its retained history.

`bridge_general_ledger_tracking_option` normalizes the two source tracking
slots on a journal line into one source-slot relation. It resolves a native
tracking option only when one tenant/category/option-name candidate exists;
unresolved and ambiguous source slots remain visible with their flags. It has
no monetary measure and `is_financial_measure_additive = false`, so it must be
used to filter or group a finance fact by one selected tracking category rather
than to sum a finance fact across both bridge positions. Xero tracking stays
tenant-defined: this bridge does not introduce a Visma-style BU, department,
or project allocation.

`fact_source_exchange_rate_observation` records the native rate evidence that
is already present on Xero documents, payments, and bank transactions. It
exposes source/base currencies, rate presence, cross-currency coverage, and
the source amounts without guessing rate direction, creating a daily
currency-pair rate, or calculating NOK. `is_rate_direction_known` and
`is_nok_amount_available` are consequently false. A dedicated rate-direction
and date-policy review plus an authoritative daily FX source remain required
before any group-currency reporting measure is published. Its source amounts
are evidence only and `is_financial_measure_additive` is false, since the fact
mixes document, payment, and bank-transaction event types.

`xero_datamart.sales_pipeline` is the Xero quote-level consumption model. It
retains `is_source_open_pipeline` and
`source_pipeline_amount_in_currency` for audit, but exposes
`is_effective_open_pipeline` and `pipeline_amount_in_currency` for business
aggregation. A quote is effectively open only when its source lifecycle is
open and its `expiry_date` is not before the observed `synced_at` date. Expired
quotes therefore remain visible but cannot be counted as live pipeline. The
mart has no forecast, booked-revenue, or NOK amount.

`xero_datamart.procurement_commitments` is one row per Xero purchase order. It
keeps the source lifecycle and bill marker, and exposes the Xero-supported
open commitment amount only when the source parent/line set is unambiguous.
It is not an AP-cost or supplier-invoice model, and it does not convert amounts
to NOK.

`xero_datamart.project_operations` is one row per retained project operational
snapshot. It makes native task, time, delivery and billing-workflow measures
easy to consume, while keeping source lifecycle and direct-detail
reconciliation evidence. Xero does not provide project-linked ledger cost,
revenue allocation, margin, or an authoritative FX source in this contract;
`is_project_pnl_available` and `is_nok_amount_available` are consequently
false and the corresponding values are NULL. `xero_business_mart_safety`
asserts one-to-one source grain, lifecycle, amount availability, and those
explicit unavailable-measure rules for all three non-finance marts.

The current typed/raw baseline has authoritative coverage evidence for all 34
operations in the current Accounting and reporting warehouse contract: 27
standard Accounting collections, direct budget detail, and six current report
operations. All 34 are authoritative per tenant across seven tenants. Readiness
is intentionally certified as two coherent cohorts rather than by mixing stale
successes from unrelated runs: the 28 standard Accounting/detail operations
must each come from one successful run per tenant, and the six reports must each
come from one successful reporting run per tenant. A tenant is current-accounting
ready only when both cohort checks are true.
`xero_endpoint_coverage_summary` now distinguishes
`is_ready_for_current_accounting_warehouse` from
`is_ready_for_full_catalog`: the first is true only after all 34 required
Accounting/reporting operations have valid coherent evidence, while the latter remains
false until all 148 in-scope catalog operations are complete; 115 remain
unattempted. Payment Services is deliberately excluded. This is therefore a
robust Accounting and current-reporting business layer, not a claim that every
Xero endpoint or all historical changes are already available. The next source
milestones are data-dependent detail routes,
optional reports and the remaining Xero API families, each reconciled by tenant
and endpoint before its domain reads raw-derived source data.

The latest full workflow
`1792179945-2e8745e1-3dd6-44d6-9873-0bcbc08d1263` succeeded with the 579-action
graph. `getBudget` has seven authoritative direct-detail responses, one for
each tenant, but each currently contains `BudgetLines = []`. Bronze therefore
retains seven explicit zero-line responses while Bronze, Silver, and Gold
budget-balance facts correctly contain zero rows. The balance safety assertion
returned zero failures; this is a source-complete empty-state result, not a
missing model or an inferred zero-valued budget.

`xero_endpoint_warehouse_coverage` declares 182 unique Bronze, Silver, and Gold
model relations across the warehouse contract. The compile-time dependency
validation is grouped into four generated views of no more than 45 direct model
dependencies each, so a renamed or removed contract model fails Dataform
compilation without exceeding BigQuery Dataform's 50-dependency action limit.

## Develop and publish

```bash
PYTHONPATH=. uv run python Dataform/xero_warehouse/pull_from_gcp.py --apply --overwrite
PYTHONPATH=. uv run --extra dev pytest -q Dataform/xero_warehouse/tests/test_project.py
dataform compile Dataform/xero_warehouse --quiet --timeout 60s
PYTHONPATH=. uv run python Dataform/xero_warehouse/sync_to_gcp.py
PYTHONPATH=. uv run python Dataform/xero_warehouse/sync_to_gcp.py --apply --message "Describe change"
PYTHONPATH=. uv run python Dataform/xero_warehouse/run_workflow.py --full --wait
PYTHONPATH=. uv run python Dataform/xero_warehouse/run_workflow.py \
  --target aquatiq-datavarehus.dw_1_silver_xero_assertions.xero_gold_reference_data_safety \
  --include-dependencies --wait
PYTHONPATH=. uv run python Dataform/xero_warehouse/run_workflow.py \
  --target aquatiq-datavarehus.xero_datamart.sales_pipeline \
  --target aquatiq-datavarehus.xero_datamart.procurement_commitments \
  --target aquatiq-datavarehus.xero_datamart.project_operations --wait
PYTHONPATH=. uv run python Dataform/xero_warehouse/run_workflow.py \
  --target aquatiq-datavarehus.dw_1_silver_xero_assertions.xero_business_mart_safety --wait
```

`run_workflow.py` has the Xero project, repository, region, and workspace
compiled into it. It rejects unqualified and foreign-project targets and checks
every compilation and workflow resource returned by the Dataform API. Do not
use the shared `Dataform/scripts/dataform-workflow-invoke.sh` for Xero: its
legacy defaults belong to the mixed `prj-dw-dev` warehouse.

Run a full workflow when Bronze/Silver sources have changed. For a downstream
mart-only change, run its target without `--include-dependencies` and then its
safety assertion: those marts read already-certified Gold relations. Pulling
their full dependency closure can needlessly rebuild high-write raw-history
Bronze tables and exhaust BigQuery's per-table update quota. This is an
execution-scope rule, not permission to bypass the safety assertion.

The sync command uploads only this project's settings, README, and definitions.
It never removes remote files and does not publish the mixed Visma project.
Compile and reconcile changed models before creating any schedule. The
initial run covered 886 accounts, 13,491 invoices, and 13,303 payments across
seven tenants; customer payments were 3,020, including deleted-marked rows.
No base-currency payment amount is derived because the Xero rate/bank-amount
semantics need a separate tenant-level reconciliation.
