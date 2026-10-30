# Visma Silver to Xero Silver: Audit and Build Plan

Audit date: 2026-10-06. Project: `prj-dw-dev`.

Status: **audit only; no silver models, datasets, schedules, or source data changed**. Naming and source contracts were corrected after live staging verification on 2026-10-06. The recommendations below are not an implementation or a completed financial reconciliation.

## 1. Conclusion

Reuse Visma's **account classification, document/line grains, and uniqueness-guarded enrichment**. Xero has one business unit per tenant. Visma's dated business-unit split and mixed-unit bridge do not belong in Xero silver.

The target pipeline is `dw_staging_xero` endpoint tables -> materialized Xero bronze tables in Dataform -> Xero silver, delivered through the Dataform repository's `main` revision. The application directory's `_v2` suffix distinguishes it from the older development application; it is not a warehouse namespace or table suffix. Complete GL parity is conditional on Xero Journals access and extraction coverage. Warehouse, stock-movement, payroll, and some project/ERP concepts do not have equivalent sources in the currently selected Xero API families.

A fresh baseline does not recreate all historic versions or deleted records that Xero no longer returns. Preserve versions from first capture onward, and distinguish observation-time history from source-effective/accounting history. API history endpoints and historic report snapshots have their own coverage and cannot be assumed to reconstruct an earlier complete state. This design does not import v1 history to fill those gaps.

### Priority Findings

| Priority | Finding | Implication for Xero |
|---|---|---|
| High | 72 live Visma silver objects, but only 53 are in the latest observed compiled graph. The other 19 have April modification dates. | Use the successful compiled graph as the reference. A live table list alone is not the supported model specification. |
| High | Visma silver reads older `dw_1_bronze_visma` tables; the observed Dataform graph declares those 84 sources rather than building bronze from `dw_staging_visma`. | Build real Xero bronze transformations from `dw_staging_xero` endpoint tables before Xero silver, then publish them in `main`. |
| High | The current generic `v2_*` Xero source-history tables are empty, although ordinary Xero staging business tables contain data. | Decide provenance and a fresh capture before publishing; an empty technical source table does not mean staging business data is empty. New names should not inherit the application suffix. |
| High | `Dev`/local SQL is not the same business logic as the compiled `main` GL model. | Pin the reviewed compilation and recover its guarded AR/AP/project enrichment before adapting it. |
| High | Xero has one organisation/business unit per tenant; tracking categories are separate dimensions. | Set the reporting business-unit key from `tenant_id`. Do not copy Visma subaccount or mixed-BU allocation rules. |
| High | Xero Journals has separate commercial/security access requirements. | A catalog entry or OAuth scope does not prove that a complete GL can be fetched. |
| Medium | Visma has useful gap reports, but relatively few blocking assertions. | Add stronger source-completeness, accounting, mapping, and cardinality gates for Xero. |
| Medium | `dw_1_bronze_xero` already has 29 legacy objects and `dw_1_silver_xero` has 46; neither is in the latest compiled Xero graph. | Use canonical names in `main`, with an explicit collision and consumer cutover plan for the older objects. |

## 2. Evidence and Scope

Inspected the [live Visma silver dataset](https://console.cloud.google.com/bigquery?project=prj-dw-dev&ws=!1m5!1m4!3m2!1sprj-dw-dev!2sdw_1_silver_visma!23sRESOURCE_LIST), [Dataform `Dev` workspace](https://console.cloud.google.com/bigquery/dataform/locations/europe-north1/repositories/Datawarehouse/workspaces/Dev/files/?project=prj-dw-dev), latest observed workflow invocation, compiled queries/dependencies, relevant mapping SQL, current Xero source projections, and bounded BigQuery aggregate checks.

No BigQuery/Dataform connector tools were exposed in this session. Cloud inspection used authenticated read-only Dataform REST GETs, BigQuery metadata, and eight aggregate SELECTs with a 512 MiB billing ceiling per query. Those SELECTs processed 154,468,267 bytes in total. No financial amounts, personal records, credentials, or raw payloads were copied into the evidence artifact.

The companion [evidence JSON](XERO_SILVER_VISMA_AUDIT_EVIDENCE.json) records object inventories, modification dates, query text/job IDs, profiling results, workspace hashes, compiled-query hashes, and compilation/run identities. It also retains both GL transformation sources to substantiate the workspace/compiled difference; these are SQL, not business records. Snapshots were collected sequentially, not as an atomic cross-service snapshot.

### Infrastructure Snapshot

| Dataset | Location | Objects | Stored-row metadata sum | Interpretation |
|---|---|---:|---:|---|
| `dw_1_silver_visma` | `europe-north2` | 72 | 2,744,302 | 64 tables and 8 views; several grains overlap. |
| `dw_staging_visma` | `europe-north2` | 135 | 4,604,504 | Endpoint-named raw JSON tables; no table labels. |
| `dw_staging_xero` | `europe-north2` | 60 | 461,715 | Existing populated endpoint tables are typed; generic `v2_*` history tables are empty. |
| `dw_1_silver_xero` | `europe-north2` | 46 | 21,676 | Existing objects are from the older Xero implementation. |
| `dw_1_silver_excel` | `europe-north2` | 17 | 8,967 | Shared reference/mapping relations. |

These sums are not transaction totals or unique business-record counts. BigQuery views have no stored rows; metadata zero for a view does **not** establish that querying the view returns zero rows.

Live `COUNT(*)` checks on 2026-10-06 confirmed `dw_staging_xero.accounts` **886**, `contacts` **1,890**, `invoices` **13,401**, `invoice_lines` **23,791**, `payments` **13,256**, `journals` **78,275**, and `journal_lines` **241,345**. All six existing physical `v2_*` record/child/collection/receipt/run history tables still returned **0**. The catalog has 150 operation rows; the dataset has 53 physical tables and 7 views. The earlier statement that Xero staging business tables were empty was incorrect.

Visma staging uses endpoint names such as `accounts` and `customer_balances_v2`; the suffix belongs to that endpoint. `dw_staging_visma.accounts` has `tenant_id`, `record_key`, JSON `payload`, `payload_hash`, first/update timestamps, run ID, and `deleted_at`. It is a latest-record table. Existing `dw_staging_xero.accounts` is typed and does not have the same raw envelope. Xero endpoint tables should use natural endpoint names, while bronze tables can use provider names such as `xero_accounts`. Retaining all observed versions and deletion/removal markers is still required, so the staging design cannot discard history merely to copy Visma's latest-only storage.

Visma's endpoint tables have **no table labels**. Its dataset does carry `source=visma_v2` and `layer=staging`. Xero's existing `accounts` table has no labels, but the generic `v2_record_versions` table and `dw_staging_xero` dataset currently carry `source=xero_v2` and `layer=staging`. The desired new Xero resources should have no labels; existing metadata would need correction during the migration. These labels describe today's objects, not a naming requirement for the new main pipeline.

### Dataform Runtime

| Item | Observed Value |
|---|---|
| Repository | `projects/prj-dw-dev/locations/europe-north1/repositories/Datawarehouse` |
| Workspace | `Dev` |
| Core version | `3.0.42` |
| Latest observed invocation | `1791309906-98afc883-13ec-4ac9-b530-827232681c6a` |
| Invocation start/end UTC | `2026-10-06 18:05:06` / `18:06:50` |
| Compilation result | `e341abb5-8ad8-4ed6-88cc-76c1d9af6583` |
| Compiled revision | `main`, SHA `c14e29da49409a89aa785eba195bbb213abe11f1` |
| Compilation/run outcome | No compilation errors; all 147 executed actions succeeded. |
| Full graph | 234 actions: 87 declarations and 147 executed actions. |
| Visma silver graph | 53 relations: 45 tables and 8 views. |
| Xero actions in this graph | None. |

The Visma bronze declarations do not prove a staging-to-bronze transformation in this graph. `dw_bronze_visma`, mentioned in the Visma staging dataset description, was not found in this project. Building materialized bronze models from staging is therefore an explicit requirement for Xero rather than an already-running Visma Dataform pattern to copy verbatim.

`Dev`'s workflow settings and this compilation use `defaultLocation: europe-north2`; the Dataform repository's `europe-north1` location is the control-plane location. Keep those distinct. Every new Xero model needs an explicit schema: the default dataset is `dw_1_silver_visma_global`, which was not found during metadata inspection. Do not change global defaults just to add Xero.

No workflow configurations were returned. A `production` release configuration exists, but its referenced compilation is not the invocation audited above. Repeated invocations exist; their scheduler/orchestrator and trigger policy were **not proven** in this audit. Register Xero with the actual deployment mechanism rather than assuming that committing SQL automatically schedules it.

All 51 local Visma relation files that also exist in the downloaded `Dev` scope matched it. `Dev` has two additional relation files absent locally: `dim_warehouse_location` and `fact_project_budget`. `Dev`'s GL source already has unique-document AR project linkage. The executed `main` query additionally has guarded AP line/document and PM journal-line/batch linkage, and stronger AR blank/`X` normalisation. Workspace equality is not production parity. Both sources and their hashes are retained under `dataform.general_ledger_source_comparison` in the evidence JSON.

### Structural Checks on Live Visma

| Relation | Rows | Tenants | Duplicate Tested Grain | Null Grain Fields | Unresolved BU Rows |
|---|---:|---:|---:|---:|---:|
| `dim_account` | 5,822 | 12 | 0 | 0 | n/a |
| `fact_customer_invoice` | 31,149 | 12 | 0 | 0 | 0 |
| `fact_customer_invoice_line` | 99,396 | 12 | 0 | 0 | 0 |
| `fact_supplier_invoice` | 21,252 | 12 | 0 | 0 | 0 |
| `fact_supplier_invoice_line` | 59,777 | 12 | 0 | 0 | 0 |
| `fact_customer_payment` | 30,002 | 12 | 0 | 0 | 0 |
| `fact_supplier_payment` | 17,838 | 12 | 0 | 0 | 0 |
| `fact_general_ledger` | 1,104,450 | 12 | 0 | 0 | 0 |

Tested grains: accounts `(tenant_id, account_cd)`; invoices `(tenant_id, reference_number, document_type)`; lines add `line_number`; payments `(tenant_id, ref_nbr)`; GL `(tenant_id, batch_number, ref_number, line_number)`. The account mapping flag was populated for all checked accounts.

This establishes structural coverage at those grains, not correct classifications, correct amounts, source exhaustiveness, or reconciled balances. BU coverage counts only NULL/`UNMAPPED` markers; a wrong but non-null mapping passes that check. A silver rebuild timestamp also does not prove source freshness: the account table was rebuilt on October 6 while its maximum source `synced_at` was October 3. That can be normal for an unchanged master; receipt/run evidence is needed to distinguish it from stale extraction.

## 3. Visma Business Logic Worth Reusing

Paths below are relative to the cloud Dataform repository. Treat compiled queries from the pinned revision as the execution reference, and inspect workspace differences before editing.

| Rule | Visma Implementation | Xero Adaptation |
|---|---|---|
| Tenant-scoped identity | Dimensions/facts consistently include `tenant_id`. | Use Xero's organisation-scoped native IDs, with generation/version in history. Never use display name or invoice number as the sole identity. |
| Group chart classification | `silver/visma/dimentinals/dim_account.sqlx`; shared `silver/excel/chart_of_accounts.sqlx`, company mapping and global range mapping seeds. | Reuse group account hierarchy, BS/PL, FSLI, EBITDA, net-income, cash-flow, NWC, capex, and intercompany outputs. Create approved Xero account crosswalks. |
| Dated business-unit mapping | `silver/excel/business_unit_mapping.sqlx`; Visma segment-2 and business-date crosswalk. | Visma-specific; Xero uses one business unit per tenant, with `business_unit_id = tenant_id`. |
| Organisational reporting history | Mapping preserves historical source entities across the 2026 reorganisation. | Keep source organisation separate from reporting entity/BU; never rewrite raw source identity to the reporting entity. |
| Header and line facts | Separate customer/supplier invoice and line models. | Preserve canonical invoice and line grains, then expose customer/supplier compatibility views. |
| Mixed-BU documents | `silver/visma/facts/fact_sales_order_business_unit.sqlx` aggregates Visma line contributions per order/BU. | No equivalent Xero BU bridge is needed. Tracking options stay available for separate reporting dimensions. |
| Safe project enrichment | Compiled `fact_general_ledger`: AR unique-document project, AP unique-line then unique-document, PM unique-journal-line then unique-batch fallback. | Reuse the uniqueness guards and explicit precedence. Use native source links where available; unresolved/ambiguous joins stay unresolved. |
| FX provenance | `dim_currency_rate` normalises multiply/divide direction and distinguishes market rates. | Preserve endpoint-specific source rates and amount currencies; group reporting FX is a separate approved policy. |
| Unified facts plus aliases | Inventory movement models feed eight compatibility views. | Reuse canonical-fact plus filtered-view design for AR/AP and settlements; do not create overlapping physical facts unnecessarily. |
| Operational quality reports | Shared silver contains project/procurement/customer-class gap reports. | Publish Xero coverage, mapping gaps, settlement gaps, and reconciliation exceptions per tenant/date/endpoint. |

### Rules That Must Change

1. **Account classification:** Visma uses organisation-name-scoped workbook mappings, NO/SE/DK range breakpoints, approved four-digit family roots, and native-type fallbacks. The five-digit profile heuristic is source-specific. Do not apply these numeric ranges or heuristics to another country's Xero chart without finance approval. Map by `(tenant_id, AccountID)`; retain `Code` as an attribute and record mapping method/confidence. Unknown accounts remain explicitly unmapped.
2. **Tracking:** Visma subaccount segments mean intercompany, BU, and department. Xero's reporting BU is the tenant, not a tracking option. Keep category/option IDs as independent analytic dimensions, never infer a BU from their positions or labels. The API permits only two active categories and four total. [Xero tracking categories](https://developer.xero.com/documentation/api/accounting/trackingcategories/).
3. **Document attribution:** Every Xero document and payment retains its source `tenant_id`, which determines its sole reporting BU. A document may have multiple tracking values without becoming a mixed-BU document. Preserve tracking at the appropriate line/header grain for analysis.
4. **Lifecycle:** Several Visma sources filter `isDeleted` before ranking the latest eligible row. That is unsafe on append-only Xero history: it can resurrect an older active version after a deletion. Resolve the latest revision first, propagate conflicts/deletion/removal states, and then filter only in consumer views.
5. **Numeric types:** Visma monetary schemas mix `FLOAT64` and `NUMERIC`. Use exact decimal parsing for Xero amounts; choose adequate decimal precision for FX. Preserve original amounts, tax basis, rounding adjustment, and currencies rather than concealing differences with floating-point tolerances.
6. **Period and project semantics:** Derive Xero fiscal periods from organisation settings; do not invent Visma ledger/branch/closed-period attributes. Visma's PM/AR/AP reference and line-number join rules do not establish equivalent Xero project associations.

## 4. Xero Domain Coverage and Limitations

| Domain | Proposed Xero Silver Contract | Limits / Required Checks |
|---|---|---|
| Companies | `dim_company`, source tenant and canonical group entity crosswalk. | Obtain live connected-tenant inventory, base currency, fiscal year settings, and finance-approved entity mappings. |
| Accounts | `dim_account`, plus bank/payment-eligible account views. | `AccountID` is identity; local code/name and native type are not an approved group mapping. |
| Tracking, taxes and currencies | Tenant-scoped tracking category/option and tax-rate dimensions; currency/reference views. | Preserve category/option IDs and archived states; tax codes/components are organisation/country-specific, not interchangeable Visma VAT categories. Where a source line supplies only account code, use a validated tenant/date-scoped lookup rather than treating that code as `AccountID`. |
| Customers and suppliers | One `dim_contact`; role-specific views/bridge. | A contact can have both roles. Refresh flags/balances periodically rather than relying exclusively on modified-since. [Contacts](https://developer.xero.com/documentation/api/accounting/contacts). |
| Sales invoices and bills | Canonical `fact_invoice`/`fact_invoice_line`; ACCREC/ACCPAY views. | Require authoritative document details, status, tax basis, amounts, rounding, and valid line identity. Numbers are attributes, especially for supplier documents. [Invoices](https://developer.xero.com/documentation/api/accounting/invoices). |
| Customer/supplier payments | Canonical `fact_payment` keyed by `PaymentID`; separate type-filtered views. | Keep invoice-currency `Amount`, account-currency `BankAmount`, status, refund/payment type, account/source links, and reconciliation flag. These are financial records, unlike excluded payment-provider configuration. [Payments](https://developer.xero.com/documentation/api/accounting/payments). |
| Credits and settlements | Credit-note, prepayment, overpayment, refund, and allocation facts/bridges. | Distinguish non-cash allocations from cash payments. Never sum embedded invoice settlements and `/Payments` as independent transactions. Reconcile document balances with explicit settlement categories. |
| Bank movements | Bank transactions/lines and transfers, preserving both account links. | Transfers are not income/expense and must not be counted twice through linked transactions. Transfer rate units differ from other endpoints. [Bank transfers](https://developer.xero.com/documentation/api/accounting/banktransfers). |
| General ledger | Conditional journal-line fact and account/period balances. | Requires entitlement, complete extraction, correct accounting basis, opening/conversion coverage, and report controls. Manual journals alone are not the organisation's GL. |
| Financial reports | Timestamped report snapshots and typed report cells/control results. | Parameterised report grain includes tenant, report, period, basis, filters, currency, and observation. These snapshots are not ledger transactions. |
| Items/inventory | Item dimension and dated quantity/value observations. | Xero tracked inventory exposes item quantity/value, but is not a substitute for Visma warehouse/bin/lot/shipment and full stock-movement models. Broader parity needs the operational inventory source. [Tracked inventory guide](https://developer.xero.com/documentation/guides/how-to-guides/tracked-inventory-in-xero/). |
| Orders/quotes | Purchase order and quote header/line facts. | A quote is not an ERP sales order, shipment, receipt, or confirmed backlog. Label the semantics rather than renaming for superficial parity. |
| Fixed assets | Assets-family asset/type/settings observations and supported schedules/transactions. | Verify actual endpoint fields and historical coverage before promising Visma fixed-asset transaction parity. |
| Projects | Projects-family projects, tasks, time, and expense facts where available. | Project ID namespaces and accounting links need explicit validation; no automatic one-to-one match with Visma project budgets or GL project lines. |
| Expense claims/receipts | Model supported records only when tenant capabilities and fresh data justify it. | The existence of a route does not prove tenant use, accessible history, or parity with Visma's expense workflow. |
| Other API families | Keep Bankfeeds/Finance/Identity responses with source-family and snapshot semantics. | Bankfeeds is not automatically a consumer's complete bank transaction history; Finance snapshots are not additional copies of Accounting facts. |
| Employee/payroll/ERP-only masters | No fabricated parity tables. | The selected six-family catalog is not a Payroll implementation; Xero Users are not employee/payroll records. |
| Payment Services | Explicitly out of scope. | Exclude `getPaymentServices` and `getBrandingThemePaymentServices` from required coverage and silver. Keep customer/supplier Payments in scope. |

### GL Access Is a Build Gate

Current Xero documentation states that Journals requires initial/annual security assessment and use-case approval, starting at the Advanced tier. Verify app entitlement and user permission as well as scopes. Do not treat a 401/403 as an empty ledger. [Journals](https://developer.xero.com/documentation/api/accounting/journals/).

If access exists, verify offset-based extraction and its terminal condition: a response with fewer than 100 journals is not necessarily the end. Preserve accounting basis; default journals are accrual, while `paymentsOnly` requests cash basis. Journal `NetAmount` is in organisation base currency and uses debit-positive/credit-negative signs. Collection retrieval provides `SourceID`/`SourceType`, but individual journal retrieval does not; do not let a sparse detail observation erase source linkage. [Journals](https://developer.xero.com/documentation/api/accounting/journals/).

If access is unavailable, release AR/AP/payment and report-snapshot capabilities with an explicit GL limitation. Do not reconstruct and advertise a complete GL from invoices, payments, or manual journals: tax, FX, conversion, adjustments, and other postings would be unproven.

One existing source-quality rule needs extra scrutiny: `etl/xero/endpoint_coverage.py` accepts `offset_not_advancing` for the collection Journals operation. Before silver trusts that outcome, prove that it represents genuine source exhaustion rather than repeated/stalled pages. This is a static acceptance-risk finding, not a claim that a live fresh journal run has already failed.

### Payload Authority and Currency Are Cross-Cutting Risks

Embedded references are not authoritative document snapshots. Xero's payment examples include referenced invoices with empty child arrays, and the documentation warns that serialization can represent null as an empty array. Therefore an embedded `Invoice.LineItems: []` must not remove known invoice lines. Use endpoint/response-shape authority rules before applying collection-completeness semantics. [Payments](https://developer.xero.com/documentation/api/accounting/payments).

Retain separate document, organisation-base, bank-account, and group-reporting amounts/currencies. Do not apply one universal `CurrencyRate` formula. Approve endpoint-specific conversions and rate dates, including payment/transfer cases, with fixtures before publishing reporting amounts. Market FX feeds and existing Visma group-currency policy can supply reference rules, not rewrite source accounting amounts.

## 5. Proposed Xero Staging, Bronze and Silver Architecture

This is a recommendation, not an approved dataset creation or deployment.

```text
Xero API -> raw GCS -> dw_staging_xero.accounts, invoices, payments, ...
                                | raw endpoint payloads and preserved history
                                v
              Dataform main: dw_1_bronze_xero.xero_accounts,
                             xero_invoices, xero_payments, ...
                                | typed, validated bronze records
                                v
              Dataform main: dw_1_silver_xero.dim_account,
                             fact_invoice, fact_payment, ...
                                | tenant_id is the sole Xero BU key
                                v
                       common/gold/BI consumers
```

The API version is included in a table name only if it is part of the endpoint's own name, as in Visma `customer_balances_v2` or `sales_orders_v3`. The application's `_v2` directory name does not propagate to datasets, tables, Dataform folders, or provider tags.

### Storage and Deployment

- Use endpoint names for staging (`accounts`, `invoices`, `payments`, `journal_lines`) and provider-prefixed names for bronze (`xero_accounts`, `xero_invoices`, `xero_payments`, `xero_journal_lines`). Silver uses `dim_*`/`fact_*` names in `dw_1_silver_xero`. No generic version prefix and no labels on new Xero resources. Dataset metadata also needs review because `dw_staging_xero` currently has labels.
- Build materialized bronze transformations from `dw_staging_xero`, then silver from bronze. A Dataform `declaration` merely points at an existing table and does not create or populate it. Keep explicit database/schema and `europe-north2` execution location because the repository defaults to a Visma-specific dataset.
- Deliver the new Xero definitions through Dataform `main` under natural source folders (`definitions/bronze/xero/`, `definitions/silver/xero/`). The older application's `v1`/`dev` naming does not define this Dataform release. The latest observed `main` compilation has no Xero actions yet.
- Existing names collide: typed staging business tables already occupy `accounts`/`invoices`/`payments`; `dw_1_bronze_xero` has 29 older tables and `dw_1_silver_xero` has 46. Inventory their consumers, retain their data, and plan a controlled namespace transition before publishing new definitions with these names. Do not activate the old SQLX files as the new pipeline by accident.
- Inspect both the read dependency graph and selected execution closure. Including a shared mapping can execute its legacy producers. Reject unrelated writes and older Xero inputs in the new execution plan.
- Separate immutable typed version history from lifecycle projections and current financial views. `is_current`, `is_superseded`, and removal/deletion state can change when a new observation arrives: derive them from complete source history or append explicit state events, rather than freezing them inside an immutable old version row.
- Preserve raw source lineage, generation, record/child/version keys, receipt/run, source-updated time, observed time, payload hash, and mapping/rule version. The staging endpoint table can expose a current projection like Visma, but retained raw/version history and deletion/replacement markers must remain accessible. Do not use Dataform full refresh to erase retained history.
- Partition growing version/snapshot tables by observation date and cluster for tenant/source-key access where measured useful. Current projections must still resolve history globally; a date filter or truncated lookback must not resurrect obsolete versions. No live Visma silver relations inspected were partitioned or clustered, so this is an improvement to evaluate, not a pattern already verified there.
- Define an append/idempotency strategy before materialising history: exact version key checks, retry-safe inserts, late-arrival handling, and controlled parser backfills. For identical payload versions, keep observation/receipt freshness independently of business revision history.
- Publish only a validated generation/run through an explicit release pointer or equivalent isolated publication step. Assertions executed after a table build do not by themselves stop direct queries from seeing unvalidated data. Gate consumer publication, not merely the workflow's final status.

### Source Contracts

| Proposed source role | Natural name example | Gate |
|---|---|---|
| Endpoint staging | `dw_staging_xero.accounts`, `invoices`, `payments` | One endpoint-shaped raw payload contract per business resource, with tenant/key and observed/source timestamps. Keep the full capture history available even if this surface shows current records. |
| Bronze transformation | `dw_1_bronze_xero.xero_accounts`, `xero_invoices`, `xero_payments` | Materialized Dataform models read only the corresponding Xero staging endpoint source. Preserve source identity, payload/provenance, deletion and supersession states. |
| Run and coverage controls | `dw_staging_xero.sync_runs`, `sync_run_entities`, `ingest_receipts`, `endpoint_catalog` | Distinguish complete, bounded, blocked, unsupported, and failed extraction; compare with an independently maintained expected tenant/endpoint matrix. |
| History and collection membership | Endpoint-scoped retained versions/children and current projections | A removed child requires a complete authoritative collection. Missing/null collections are not empty collections; source deletion is a distinct event. |
| Silver dimensions/facts | `dw_1_silver_xero.dim_account`, `fact_invoice`, `fact_payment` | Derived from bronze only, with tenant identity, exact monetary types, and tested grain. |

The current implementation still uses the generic `v2_*` technical tables and projections in `etl/xero/fresh_views.py`. They are **existing code and schema**, not the target names. Its supersession/deletion/removal semantics are useful to preserve when endpoint tables replace the generic naming.

The current coverage query discovers tenants from observed run entities and receipts. A tenant that has never produced either is invisible to that matrix. Compare it with a separately verified expected connected-tenant inventory before treating coverage as complete.

A latest parent can omit a collection and retain a previously observed authoritative collection. Its current children may consequently have an older collection version than the parent. Do not require `child.version_id = latest_parent.version_id` blindly; preserve both versions and use validated collection membership/provenance. Conversely, do not mix arbitrary historical children with a current header.

The endpoint-qualified source key prevents ingestion collisions, but multiple operations can return the same business object. Normalise collection/detail/envelope responses into a canonical entity role before deduplication. Do not union `accounting_get_*` response envelopes with canonical entity rows as if every operation produced another independent invoice/payment.

### Recommended Fact Grains

| Contract | Business Grain Within Generation/Tenant | History/Allocation Detail |
|---|---|---|
| `dim_account`, `dim_contact`, `dim_item` | Native entity ID. | Versioned attributes; dated group crosswalk separately. |
| `fact_invoice` | `InvoiceID`, with source invoice type. | Versioned header; invoice number/reference are attributes. |
| `fact_invoice_line` | Invoice ID plus verified line ID or collection-specific child identity. | Carry source collection/version and line position. Position-only identity must be snapshot/version scoped. |
| `fact_credit_note` / line | `CreditNoteID` and child identity. | Separate original amount, allocations, remaining credit, status, and sign policy. |
| `fact_payment` | `PaymentID`. | Type/status/source links and amount currency; no duplicates from nested document payments. |
| `fact_settlement_allocation` | Native allocation ID when supplied, otherwise verified typed source-document/target-document/collection-version child identity. | Distinct payment, credit, prepayment, overpayment, refund and allocation events; never an unverified amount/date synthetic key. Batch/embedded payments corroborate canonical Payments, not additional cash movements. |
| Xero reporting business unit | One row or computed value per tenant. | `business_unit_id = tenant_id` for all Xero facts; Xero's native organisation ID remains a separately retained source attribute. Tracking categories do not change the BU. |
| `fact_general_ledger` | Accounting basis plus `JournalID`/`JournalLineID`. | Basis belongs in identity if both cash and accrual are stored; source-link and coverage provenance required. |
| `fact_report_snapshot` / cell | Report parameters plus observation/version and structural row/cell path. | Labels are attributes, not globally unique row IDs. |

Financial consumer views must publish their status inclusion, sign policy, date basis, currency basis, and completeness scope. Draft, voided, deleted, archived, superseded, and removed are distinct states. Retain them in history even when excluded from posted/current financial views.

AR/AP classification follows document/payment type and transaction direction, not mutually exclusive customer/supplier contact roles. A dual-role contact must not duplicate a settlement or select the wrong subledger.

## 6. Validation and Release Gates

The reviewed graph has seven assertion actions: purchase-order line quantities, supplier key coverage, project-budget sync coverage, and four built-in chart/global-account mapping assertions. Shared gap-report tables are valuable diagnostics, but are not automatically blocking assertions. Google documents explicit assertion dependencies; use appropriate `dependOnDependencyAssertions`, `includeDependentAssertions`, or explicit assertion dependencies, and verify the compiled graph. [Dataform dependencies](https://docs.cloud.google.com/dataform/docs/dependencies?hl=en).

| Gate | Required Evidence Before Release |
|---|---|
| Fresh baseline | Authorised generation, independently verified expected tenant/endpoint inventory, full required endpoint runs with valid pagination/terminal evidence, raw pointers/receipts, and no fallback to legacy records. A tenant absent from all observations must still appear as missing. |
| Non-empty expectations | Distinguish a genuinely empty successful endpoint from missing baseline/blocked access. A zero-row table must not make all accounting assertions pass vacuously. |
| Identity/cardinality | Unique version/business keys; non-null required IDs; all enrichment preserves expected row count and amount totals. Conflicting mappings/source revisions are visible and block affected financial publication. |
| Mappings | Approved account and legal-entity reporting crosswalks; non-overlapping effective intervals and explicit unresolved accounts. Assert exactly one BU per Xero tenant, identified by `tenant_id`. Tracking classifications remain independent. |
| History | Update, deletion, voiding, child removal, missing/null collection, sparse embedded reference, conflicting revision, retry, out-of-order arrival, and generation change fixtures. No resurrection or physical history loss. |
| Document arithmetic | Header/line net and tax identities under each tax basis, discounts, rounding, zero/negative lines, and source status. Exact decimal reconciliation with documented endpoint/currency-specific tolerances. |
| Settlements | Invoice balance versus recognised payments/credits/allocations in the same document currency and snapshot scope; deleted/reversed payments retained but excluded appropriately. Gate allocation-list completeness independently. Test partial, overpaid, refunded and FX cases; missing FX remains unresolved, never silently substituted. |
| GL/report controls | Balanced journals and period/account totals against same-basis, same-period Xero controls, including opening/conversion adjustments. No claim of complete GL without entitlement and complete journal coverage. |
| Freshness | Successful observation/run age separately from source update age and business date. Reobserve unchanged entities/304s correctly; use periodic refresh where incremental filters miss changing attributes. |
| Deployment | Compile and publish Xero bronze then silver in Dataform `main`; verify actual table creation rather than declarations, required assertions, execution location, consumers, and no older Xero source dependencies. |
| Security | Restrict contact/bank/person fields and raw-pointer access. Tenant-scoped authorised views/consumers; AI/Model Plane access must not bypass those boundaries. |

Report reconciliation must use aligned extraction/snapshot times, currency, basis, period, tracking filters and statuses. An invoice-level outstanding balance is not a substitute for a GL control, and results from different source snapshots cannot prove exact agreement.

## 7. Implementation Sequence

1. **Establish the intended staging contract.** Verify credentials, tenant/scopes/entitlements, source authority, and capture history. Make endpoint-named raw staging tables with natural names and no labels, while retaining versions and deletion/replacement evidence. Produce tenant-by-endpoint coverage; customer/supplier Payments stay in scope and payment-provider configuration stays out.
2. **Resolve existing-name collisions.** Inventory consumers of populated typed staging and the older 29 bronze/46 silver objects. Plan a recoverable transition so new main models can use `dw_staging_xero.accounts` -> `dw_1_bronze_xero.xero_accounts` -> `dw_1_silver_xero.dim_account` without mixing old records into the new pipeline.
3. **Build and publish bronze in Dataform main.** Materialize one provider-prefixed bronze model per supported endpoint from Xero staging. Keep explicit schemas, history/current source contracts, completeness assertions, and exact provenance. Validate that compiled and executed `main` actions actually create bronze tables.
4. **Build silver in Dataform main.** Apply approved group account and entity mappings, exact monetary parsing, tenant-level BU, typed dimensions and facts, and reconciliation assertions. No tracking-to-BU mapping or mixed-BU allocation layer.
5. **Deliver AR/AP and cash.** Invoice/bill headers/lines, credit notes, payments, pre/overpayments and allocation/refund bridges, bank transactions/transfers, and source/report reconciliation. This provides useful accounting coverage without pretending GL availability.
6. **Add GL conditionally.** Approved journal extraction, line fact, accounting-basis separation, account/period controls and group classifications. If access is blocked, publish the capability limit and continue with proven subledgers/report snapshots.
7. **Extend supported domains.** Purchase orders, quotes, items, assets, projects, budgets and remaining families according to fresh data and capability evidence. Document unsupported ERP parity rather than inventing fields or borrowing Visma records.
8. **Validate consumers and cut over.** Contract tests for names/IDs/grains/status/currencies, history retention, tenant security, runtime/cost and mandatory assertions. Only then connect common/gold/BI/Model Plane consumers. Preserve older objects until their owner approves a separate retirement plan.

### Business Inputs Still Needed

| Decision/Input | Why It Is Needed |
|---|---|
| Xero tenant-to-legal-entity reporting crosswalk | A tenant defines exactly one Xero BU; any wider group reporting hierarchy is separate from that source BU. |
| Xero account-to-group chart mapping | Native account types and Nordic ranges cannot prove group FSLI/EBITDA/cash-flow classification. |
| Tracking option semantics | Preserve native IDs and archived labels as separate dimensions, without changing the tenant BU. |
| Posted-status, credit/refund and tax/rounding rules | Keep native amounts while giving consumers consistent financial semantics. |
| Base/group currency, rate source/date and translation policy | Payment/bank/document currencies and group FX have different meanings. |
| Journal entitlement and historical coverage | Determines whether GL and derived period balances can be released. |
| Canonical-name cutover and Dataform main ownership | Existing typed staging and older bronze/silver names collide with the requested new models; preserve them while transitioning consumers. |

## 8. Objects Not to Copy Automatically

These live Visma objects were absent from the latest observed compiled graph. Their modification dates are all 2026-04-16. They may have historical consumers; absence does not authorise deletion or prove their business logic is invalid.

```text
dim_carrier                    dim_contact
dim_credit_terms               dim_lot_serial_class
dim_organization               dim_payment_method
dim_sales_order_type           dim_unit_of_measure
dim_vat                        dim_vat_category
dim_vat_zone                   fact_customer_debit_note
fact_customer_document         fact_expense_claim
fact_purchase_receipt          fact_purchase_receipt_line
fact_sales_order_v2_header      fact_sales_order_v2_line
fact_supplier_document
```

For exact live counts, tested grains, source paths, timestamps, and Dataform dependencies, use the companion evidence JSON. The next build milestone is **endpoint-named Xero staging with retained history**, followed by materialized provider-prefixed bronze and tenant-level-BU silver definitions in Dataform `main`.
