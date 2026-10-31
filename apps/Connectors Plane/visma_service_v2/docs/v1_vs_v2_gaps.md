# Where v1 holds data that v2 did not fetch

Pilot tenant, measured 2026-09-30. Source of the numbers: `scripts/v1_v2_key_diff.py`
(`docs/v1_v2_key_diff.md`), `scripts/v1_v2_row_counts.py` (`docs/v1_v2_comparison.md`) and
`scripts/reconcile_sources.py` (`docs/reconciliation.md`). v1 was only read.

v1 is the specification of what v2 has to deliver, so every row v1 holds is either in v2, gone from
Visma, or a v2 shortfall. Two shortfalls were found and fixed; the rest is explained.

## Shortfalls in v2, fixed

| Endpoint | v1 | v2 before | v2 now | Cause |
|---|---|---|---|---|
| `project_transactions` | 4,944 documents (periods 202512–202609) | 481 (202609 only) | 4,967 (v1 has 0 that v2 lacks; 23 newer) | No watermark, so the window was the current month only |
| `general_ledger_balances_v2` | 27,313 balances (45 periods) | 15,066 | 28,363 (v1 has 0 that v2 lacks) | `lastModifiedDateTime` returns only balances modified in a period; `periodId` returns all of them |

Both endpoints are now fetched one financial period at a time (`period_scan` in the endpoint
config, `fetch_financial_periods` in `erp_adapter.rs`):

* A balance belongs to exactly one period. Most documents do too (checked on the pilot tenant: the
  months are disjoint and a two-month window returns the same documents and line counts as the two
  months fetched apart), but a `ModulePM` transaction keeps its `refNbr` month after month, each
  month's window holding only that month's lines. See "All tenants" below.
* One wide window is not an option. Nine months of project transactions took a minute per page and
  then timed out, which is why v1 backfills month by month.
* Every run walks the whole range (`first_period` to the current month), because these endpoints
  give the checkpoint nothing to resume from. The executor ignores a stored watermark for them: an
  explicit window leaves one behind, and resuming from it would hide every older period.
* Each month's walk has its own completeness check against the API's `totalCount`.
* Cost: 45 months for project transactions (about 1.5 minutes), 81 for GL balances (about 3 minutes)
  for the pilot tenant. An empty month costs one request.

Things to know about the two:

* `first_period` is one value per endpoint, not per tenant: `202301` for project transactions and
  `202001` for GL balances. Across all 12 tenants the first data is 202308 and 202201, so both
  leave room. A tenant with older data would need a lower value.
* GL balances stop at the current month on purpose. Asked for a future period the API returns a
  copy of the current balances (2,691 rows for each of the next 12 months). DataMart holds 466 real
  rows in future periods (202610–202701) that this scan does not reach; they arrive through the
  DataMart side of the union.
* DataMart has the same 15,066 balances as the modified-since list, so it does not fill the
  period gap: 13,596 ERP balances are missing from DataMart.
* `periodToDateDebit` and `periodToDateCredit` are no longer validation fields. The API leaves them
  out of a balance with no movement in the period (45,574 of 60,655 rows in a scan to 202709).

## All tenants (2026-09-30)

The same two scans for the other 11 tenants v1 registers (7 active, 4 historical-only ones merged
into Aquatiq AS on 2026-01-01). `scripts/tenant_scan_compare.py`, v1 read-only. Distinct keys per
tenant; "multi-period" is how many rows repeat a (module, refNbr) already seen in another period.

| Tenant | PT v1 | PT v2 | PT v1 only | multi-period | PT periods | GL v1 | GL v2 | GL v1 only | GL periods |
|---|---|---|---|---|---|---|---|---|---|
| Aquatiq AS (pilot) | 4,944 | 4,967 | 0 | 0 | 202512..202609 | 27,313 | 28,363 | 0 | 202201..202609 |
| Aqua Pharma AS | 0 | 0 | 0 | 0 | none | 16,403 | 17,079 | 0 | 202301..202609 |
| Aqua Pharma Group AS | 0 | 0 | 0 | 0 | none | 18,624 | 18,731 | 0 | 202201..202609 |
| Aquatiq AB | 893 | 893 | 0 | 0 | 202311..202609 | 19,459 | 20,298 | 0 | 202201..202609 |
| Aquatiq Aps | 0 | 0 | 0 | 0 | none | 2,309 | 2,402 | 0 | 202301..202609 |
| Aquatiq Food Automation ApS | 252 | 252 | 0 | 0 | 202311..202609 | 4,216 | 4,379 | 0 | 202301..202609 |
| Aquatiq Group AS | 0 | 0 | 0 | 0 | none | 29,927 | 30,025 | 0 | 202301..202609 |
| Sensilist AS | 0 | 0 | 0 | 0 | none | 7,219 | 7,228 | 0 | 202301..202609 |
| Consult AS (historical) | 17,109 | 17,114 | 0 | 84 | 202309..202601 | 225,678 | 243,276 | see note | 202301..202609 |
| Aquatiq Food Factory AS (historical) | 2,112 | 2,117 | 0 | 3 | 202309..202512 | 18,156 | 19,482 | 0 | 202301..202609 |
| Aquatiq Hygiene Systems AS (historical) | 6,342 | 6,350 | 0 | 5 | 202308..202601 | 21,758 | 23,426 | 0 | 202206..202609 |
| Aquatiq Sense AS (historical) | 6,479 | 6,479 | 0 | 552 | 202310..202512 | 17,699 | 18,941 | 0 | 202301..202609 |

* No v1 key is missing from v2 for any tenant, on either endpoint. Consult AS has more than 150,000
  GL keys, so its row is compared by distinct counts (v1 225,678, v2 243,276), not by key.
* **`ModulePM` repeats its `refNbr` across periods, and v1 loses lines because of it.** Aquatiq
  Sense's `ModulePM` 1400592 comes back with 68, 66 and 66 lines in 202310, 202311 and 202312, one
  month each. v1 keys on (module, refNbr), so the later months overwrite the earlier ones. The four
  tenants with repeats have them only in `ModulePM`. Line totals, v1 against v2: Sense 37,089 / 47,851, Consult
  61,087 / 63,625, Hygiene Systems 50,224 / 50,615, Food Factory 10,547 / 10,617. The first run
  against these tenants stopped with "duplicate primary key with different payload"; the pilot has
  only 9 `ModulePM` documents, so it never showed.
* The fix: `period_scan.period_field` adds `_scan_period` to every row and the primary key of
  `project_transactions` is now `[module, refNbr, _scan_period]`. Dataform unions the lines of one
  (module, refNbr) over its periods. The walk guard keys on the API's own fields, without it.
* Five tenants have no project transactions at all; all twelve have GL balances. The scans took
  from under a minute to about 15 minutes per tenant and endpoint, Consult AS GL balances (243,276
  rows) being the longest.
* The rows sit in `vsv2_alltenants_20260930` (renamed from `vsv2_ingest_*` on purpose: the
  single-tenant scripts read every `vsv2_ingest_*` database and would mix tenants). It holds
  personal data.

## All endpoints, all tenants (2026-10-01)

Every endpoint config was run for the 11 other tenants (`scripts/tenant_sweep.py`, one scratch
database, results in `docs/evidence/tenant_sweep_*`). 1,468 tenant/endpoint pairs (the two
period-scanned endpoints above were run earlier): 1,081 ok, 387 empty, none failing after the fixes
below; 2,080,921 rows (ERP 935,928, DataMart 1,083,123, Expense 61,870).

* **Quota.** Visma's Finance API allows 15,000 requests per hour and tenant, counted separately for
  each tenant (checked on the `X-RateLimit-*` headers of 12 tenants). The sweep stayed at 120
  requests a minute per tenant, four tenants at a time; the lowest reading was 13,459 left.
* **The 13 endpoints that were empty for the pilot are empty for all 12 tenants** (`budgets`,
  `cash_sales`, `customer_contracts`, `customer_overdue_charges`,
  `fixed_asset_property_tax_groups`, `sales_categories`, `time_cards`, `dm_ar_historical_data`,
  `dm_budgets`, `dm_employees`, `dm_financial_periods`, `dm_inventory_items`,
  `dm_warehouse_locations`). Visma holds nothing for them. (`dm_employees` is empty although the
  ERP `employees` has 111 rows in the pilot: DataMart's employee feed is not delivering.)
  `inventory_receipt_transfers` and `purchase_receipt_transfers` have a few rows in the pilot and
  none elsewhere.
* **Compared with v1** (`scripts/tenant_key_diff.py` -> `docs/tenant_v1_key_diff.md`): 692
  tenant/endpoint pairs, 19 with keys only v1 holds, 120 keys in all. 72 of them are one general
  ledger batch (`008797`, Aquatiq Group AS) that is gone from Visma. Looked up in Visma: Food
  Factory shipment 000213 and purchase order 600666 and Sense supplier 501441 answer 404. The
  rest are one to seven rows per pair in `supplier_invoices`, `supplier_payments` and
  `customer_locations`; they have the profile of the pilot's explained gaps but could not be
  looked up one by one (the supplier invoice path needs a document type).

Shapes and keys the pilot tenant hid, found by running the other tenants, and fixed:

| Endpoint | What the other tenants showed | Change |
|---|---|---|
| `journal_transactions` | 400 on every first sync: the API refuses a `lastModifiedDateTime` more than 6 months back unless `periodId` is set | `period_scan` over `periodId`, 12 months ahead (future periods hold real batches), `page_size` 1000 |
| `project_tasks` | 400: the API wants a project id | partitioned over `/v1/project`, one request walk per project |
| `time_cards` | "partition dimension produced no values" for a tenant without employees | an empty dimension means nothing to fetch |
| `project_budgets` | 64 of 2,056 rows share (project, task, account group) with another | key adds `inventoryNumber` |
| `expense_transactions` | one `transactionId` returns several rows differing in `amount`, `taxFree`, `payCode` (281 of 17,161 ids in Aquatiq Group AS) | key `[transactionId, taxFree, amount]` |
| `general_ledger_transactions` | batch numbers are per module: 4 pairs of rows share (batch, line) in Consult AS | key adds `module` |
| `accounts`, `cash_transactions`, `inventory_adjustments`, `packaging_types`, `project_tasks`, `vat_zones`, `warehouses` | `validation_fields` required fields the API leaves out when empty (a cash transaction without a description, a warehouse without a replenishment class) | those fields are no longer required |

The same changes were run on the pilot tenant with two passes each (`docs/evidence/ingest_probe_20261001_014613`,
all pass): `journal_transactions` 67,514 batches / 574,967 lines (the first full ERP fetch of it; DataMart holds
573,369 lines, 4 ERP batches are missing there), `project_tasks` 3,113 (DataMart 3,108),
`general_ledger_transactions` 574,956, `project_budgets` 5,268, `expense_transactions` 30,586. The endpoint
matrix is now 122 proven, 13 empty, 0 failing.

What the sweep itself needed (and the sync now does):

* The sync's validation error lists the number of rows per reason, not just the first rejection, so
  every optional field shows up in one run (`scripts/relax_validation_fields.py` reads those errors
  from the sweep's logs and removes the fields).
* `vsv2 sync` stores `VSV2_AQUATIQ_DATAMART_COMPANY_ID` for the tenant it runs. The sweep sets it
  per tenant from `vsv2 dm-companies`; the pilot's id left in the environment would point every
  tenant at the pilot's company.
* The completeness check of a partition dimension's listing now applies to every dimension, not
  only those that fill a URL path: a project the listing misses is a project whose tasks are never
  fetched.
* The expense API sits behind CloudFront. A burst of manual requests got my address blocked for a
  while ("Request blocked ... too much traffic"). The sweep's own expense requests were never
  blocked at 60 a minute per process.

## Rows v1 holds that v2 is right not to have

| Endpoint | v1 only | What the evidence says |
|---|---|---|
| `customer_sales_prices` | 684 | All 684 are in DataMart: 512 live, 172 tombstoned. The ERP list shows prices in force today, v1 kept the ones that expired. v2 gets them through the DataMart side |
| `contacts` | 17 | All tombstoned in DataMart: deleted in Visma, v1 never removed them |
| `supplier_documents` | 17 | All tombstoned in DataMart |
| `suppliers` | 2 | Both tombstoned in DataMart |
| `payment_methods` | 5 | Live in DataMart, absent from the ERP list |
| `shipments` | 230 | 30 sampled, read by key from Visma: 28 gone (404), 2 created after v2's fetch |
| `purchase_orders` | 32 | 30 sampled: 22 gone, 8 created after v2's fetch |
| `inventory_issues` | 38 | 30 sampled: all 30 created after v2's fetch |
| `inventory_adjustments` | 13 | 13 sampled: 2 gone, 11 created after v2's fetch |
| `customer_invoices` (v1 `invoices`) | 35 | 30 sampled: 8 gone, 22 created after v2's fetch |
| `cash_transactions` | 10 | A fresh v2 fetch returns 500, the same as v1 |

No sampled row existed in Visma at the time of v2's fetch and was missing from it. The remaining
small gaps (`supplier_invoices` 34, `supplier_payments` 29, `customer_documents` 6, and so on) have
the same profile but were not sampled: their keys are built from a document type plus a number, so a
read by key needs a per-endpoint path. The gaps the samples explain are the same size.

Rows v2 has that v1 lacks are mostly new since v1's last sync (`purchase_receipts` 2,189,
`invoices` 507, `customer_locations` 304, `customer_payments` 222), and v1 cannot hold the ones its
key merges (see `docs/v1_v2_comparison.md`).

## Where v1 is worse

* `journal_transactions`: 9 rows. v1's paginator stops at the first short page of a
  paginate-by-line endpoint.
* `customers` and `inventory_items`: 230,688 and 148,832 rows for 6,382 and 5,812 keys. Traced to a
  sync that kept writing after losing its lock (not reproduced live).
* Documents keyed on their number alone lose the ones that share a number across types
  (at least 51 documents).
* A delete is only flagged after a row has been missing for several full runs, so a row deleted in
  Visma stays live in v1 until then (the `shipments` and `purchase_orders` rows above).

## Not compared

22 v1 tables could not be compared by key; the reason per table is in `docs/v1_v2_key_diff.md`.

* `budgets`, `cash_sales`, `customer_contracts`, `customer_overdue_charges`,
  `fixed_asset_property_tax_groups` and `project_tasks` have no table in v1's bronze dataset.
* Eight tables (`attributes`, `carriers`, `earning_types`, `lot_serial_classes`,
  `sales_categories`, `supplier_classes`, `time_cards`, `unit_of_measures`) are configured to a
  different dataset, `dw_1_staging_visma_v2`, where some do not exist and the rest have another
  column layout (`_tenant_id`). Small reference tables; not followed up.
* `journal_transactions` has no rows in the scratch bronze (its history comes from DataMart, see
  `docs/endpoint_matrix.md`) and `customer_classes` is keyed on a column v1 does not have.
* `general_ledger_transactions` has more than 150,000 keys (row counts agree: v1 573,243, v2
  573,553). `customers`, `inventory_items`, `sales_orders*` and `stocktakes_v2` are keyed on other
  fields in v2.
