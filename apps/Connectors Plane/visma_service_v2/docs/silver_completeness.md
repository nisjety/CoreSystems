# Silver and gold completeness audit

Audit of the v2 Dataform project (`aquatiq-datavarehus`, BigQuery europe-north2), measured on 2026-10-13 (CEST) with SELECT queries and the Dataform runs of the new assertions. Bronze, silver and gold were rebuilt between 00:23 and 00:25 CEST during the audit, and the department dimension and the project portfolio again at 00:33 (runs by other work); the comparison was repeated after both and gave the same numbers, except where the text says otherwise. Staging was last written on 2026-10-09. Other work changes models in the same tree: re-run the script before relying on a number.

## Result

- **109 models audited**: 53 silver (45 tables, 8 views) and 56 gold and datamart (47 tables, 9 views). Not audited for completeness: the 14 `silver_common` and 17 `silver_excel` models (reports, QA, bridges and seeds, they hold no source records), and the 31 bronze tables that no model reads (listed at the end).
- **Exact: 72** (row count equals the source keys, per tenant). **Class A (defect): 21** (7 lose or collapse rows, 7 have a key that does not join, 7 have a key or attribute that is always NULL; several are the same root cause seen in a gold table). **Class B (intended): 14**. **Derived (no source to compare): 2**. **Class C (explained by deleted source rows): 0**, none of the 36 bronze tables silver reads has a deleted row.
- Staging against bronze: all 72 bronze tables that have a staging endpoint hold exactly the rows of their staging table (totals).
- Business keys of the gold facts against their dimensions: 119 pairs checked, 7 with unresolved keys (7 class A, 0 class B).
- **19 assertions added** in `dataform/definitions/assertions/`, run in the Dev workspace: 12 passed and 7 failed on the class A defects, which are fixed (see "Fixed the same day"): all 19 pass now. See the last section.

## Fixed the same day (2026-10-13)

Every class A defect below was fixed in the Dev workspace within hours of the audit; the audit text below is
kept as it was found. A full run of the project after the fixes: **258 of 258 actions succeeded, including all
40 assertions** (the 7 that failed on the defects pass with 0 rows). `scripts/audit_silver_completeness.py` exits
0: every silver and gold model is exact or an intended class B difference, 119 key pairs and 0 unresolved,
72 bronze tables equal to staging. The fixes are not on Dataform `main` (the Dev workspace only, like the
other changes of the day).

| # | Defect | Change | Before, after |
|---|---|---|---|
| 1 | GL balance `business_unit_key` | `fact_general_ledger_balance.sqlx`: the INT64 hash of tenant and business unit, as `fact_general_ledger` | 0 of 450,798 rows resolved, all resolve. The column type changes from STRING to INT64 |
| 2 | Movement lines | `fact_inventory_movement_line.sqlx`: read `inventoryItem.number`, trim the warehouse id, read `extCost` | lines with an item 11,893, now 137,657; padded warehouse ids 125,764, now 0; `amount` empty on every line, now 750.7 M on 125,764 lines (adjustments 17.2 M, issues 340.4 M, receipts 393.0 M) |
| 3 | Shipment lines | `fact_shipment_line.sqlx`: `inventoryNumber` | item on 0 of 77,607 lines, now all |
| 4 | `dim_inventory_item` | keyed on the item code (`inventoryNumber`), the internal id is kept as an attribute | invoice lines that reach an item row 381 of 84,412, now all; supplier invoice lines 2,745, now 77,119 of 77,119; purchase order lines 5, now 13,898 of 13,898; sales order lines 13, now 90,159 of 90,159. Gold 17,347 rows, now 12,934: the 4,413 placeholder rows are gone and every `item_key` changed |
| 5 | `dim_location` | the business account is in the silver key and in gold `location_key`; the invoice joins match tenant, customer or supplier and location | 149 locations, now 35,726. Invoices with another account's address: customers 31,033 of 31,466, now 0; suppliers 12,602 of 12,671, now 0. Where the account has no row in `visma_locations` `location_key` is NULL instead of wrong: 54 customer invoices and 17,732 of 30,403 supplier invoices |
| 6 | Project budget | `fact_project_budget.sqlx`: `inventoryNumber` in the partition | +70 rows, actual +10.90 M (income +7.50 M, expense +3.40 M); `BI_ProjectBudgetRecon`: of the 31 affected projects 9 reconciled, now 21; the absolute delta 11.53 M, now 0.63 M |
| 7 | Customer payments | `fact_customer_payment.sqlx`: `payment_type` in the partition | +134 rows (30,103, now 30,237), `payment_amount` +4,423,283.99; all 134 Payment/Void pairs net to 0 |
| 8 | Supplier payments | `fact_supplier_payment.sqlx`: `payment_type` | +108 rows (23,899, now 24,007), `payment_amount` -686,699.58 (the survivor of a collision was arbitrary, so the error changed sign between builds); all 108 pairs net to 0 |
| 9 | `fixed_asset_key` | business unit out of both keys | 682 of 3,339 unresolved, now 0 |
| 10 | NULL attributes | datetime strings cast through DATETIME, the right payload fields (`promised`, `requested`, `id`, `Multiply`/`Divide`) | `dim_financial_period` dates 696 NULL, now 0; `dim_project` start 2,602, now 0 (end 1,406 remain: no end date in Visma); `dim_currency_rate` date, rate and inverse 33,871, now 0; purchase order line dates 13,994, now 0; `default_warehouse_id` on 9,158 items, before 0 |
| 11 | Supplier invoices | `fact_supplier_invoice.sqlx`, `fact_supplier_invoice_line.sqlx`: `document_type` in the partition | +3 headers, +60 lines; `vat_total` +864.33 |
| 12 | `purchase_order_key` | tenant, order type and order number hashed in header and line | 13 of 13,994 lines unresolved, now 0 |

Left open:

* **Bins.** `location_key` is still NULL on all inventory movement lines and on 77,597 shipment lines: those lines
  hold warehouse bins and `dim_location` holds business-account locations. A bin dimension from
  `dim_warehouse_location` is missing.
* **Supplier locations.** `visma_supplier_locations` (8,741 rows) is read by no model and would resolve 24,961 of the
  30,403 supplier invoices; adding it to `dim_location` conflicts with `assert_location_rows_complete`. Not built:
  nothing in Power BI reads that key, and a NULL is honest where the address was wrong.
* **NOK on voids.** Where a void was applied on another date than its payment, the NOK-converted gold amounts do not
  net exactly (3 of 134 customer pairs, 18,572.14; 9 of 108 supplier pairs, -13,244.06): the two lines convert at
  their own date's rate. In base currency all pairs net to 0.
* **Power BI.** Values of `item_key`, `location_key`, `fixed_asset_key` and `purchase_order_key` changed and the type of
  `fact_general_ledger_balance.business_unit_key` with them. Nothing in this repository reads them (the Power BI model
  is not here), and before the fix none of them joined; a report that stored the old values needs a refresh. A report
  that filters payments on `status = 'Closed'` counts voids without their originals.
* **Dataform `main`** does not have any of this yet.

## Method

1. **Key of the source.** The staging key of an endpoint is its `primary_key` in `config/endpoints/*.json`; bronze carries the same columns (a nested key such as `account.number` is read with `JSON_VALUE`). For each silver model the script counts, per tenant, the distinct source keys of the non-deleted bronze rows, or for a line model the elements of the JSON array the model unnests, and compares them with the rows of the model. Bronze keeps one row per staging row (checked on totals for all 72 tables), so the staging key and the bronze key give the same count.
2. **Key of the model.** Read from the SQL: every `QUALIFY ROW_NUMBER`, `PARTITION BY`, `GROUP BY`, `DISTINCT` and join key that can drop or multiply a row. Every model that is not class B or derived is meant to keep every source record (a fact or a dimension); it must equal the source keys, a larger model has a fan-out, a smaller one a drop. The B models filter, union or aggregate on purpose, the reason is in the table.
3. **Gold** is compared with the silver model it is built from, per company (`company_key` is `FARM_FINGERPRINT(tenant_id)`), and every business key of a gold fact is looked up in its dimension, because a wrong key loses nothing at the row level but breaks the join.
4. **Class.** A is a defect; B an intended filter, union or aggregation (the cause is in the table); C a difference explained by deleted source rows. A-drop: rows lost or collapsed. A-key: a key that does not join. A-null: a key or attribute that is NULL in every row.
5. **Re-run:** `scripts/audit_silver_completeness.py [--layer silver|gold|keys|staging] [--tenants]` prints the table, the key coverage and the staging check; exit status 1 when a check no longer matches its class (a new defect, or a fixed one). The 19 assertions guard the same comparisons inside Dataform.

## Class A defects, ranked by impact

Impact is the number of fact rows or amount a report gets wrong. None of the fixes is applied: no model in `silver`, `gold`, `bronze` or `includes` was edited.

**1. Gold `fact_general_ledger_balance.business_unit_key` joins nothing (A-key)**

- Effect: 450,798 of 450,798 balance rows (100%). `fact_general_ledger_balance.sqlx:40` passes on the silver key (STRING, includes `bu_valid_from`); the gold dimension and `fact_general_ledger` use the INT64 hash of tenant and business unit. Any balance report by business unit shows nothing.
- Fix: Compute the key as `fact_general_ledger.sqlx` lines 36-41. Tested: 0 unresolved after. Assertion `assert_gold_fact_keys_resolve` (fails, 1 of 7 rows).

**2. `fact_inventory_movement_line`: no item, no warehouse key, no amount (A-null)**

- Effect: 125,764 of 137,657 lines (91.4%) have no `inventory_id` (adjustment, issue and receipt branches read `$.inventory.id`, Visma sends `inventoryItem.number`) and a padded warehouse id that never matches; `amount` is NULL on all lines (`extCost`, 750,702,874.59 in total, is not read). Only the 11,893 transfer lines carry an item (2,906 of them unresolved by the item dimension, see 4).
- Fix: Read `$.inventoryItem.number` (lines 46, 83, 120), `TRIM` the warehouse ids (48, 85, 122), read `$.extCost`. Assertion `assert_line_item_codes_populated` (fails, 11 of 20 rows).

**3. `fact_shipment_line`: no item on any line (A-null)**

- Effect: 77,607 of 77,607 lines (100%): `fact_shipment_line.sqlx:57` reads `$.inventory.id`, Visma sends `inventoryNumber`. Gold `fact_shipment_detail_line.item_key` is NULL everywhere; no shipment can be reported per item.
- Fix: Add `JSON_VALUE(line, '$.inventoryNumber')`; all 77,607 numbers match the item master. Assertion `assert_line_item_codes_populated` (fails, 9 of 20 rows).

**4. `dim_inventory_item` is keyed on the wrong id (A-key)**

- Effect: Silver and gold key the item on Visma's numeric internal id (`dim_inventory_item.sqlx:28`), every fact carries the item code. Facts that reach a master row: invoice lines 381 of 84,412, supplier invoice lines 2,745 of 77,062, purchase order lines 5 of 13,898, sales order lines 13 of 90,159, movement lines 0 of 11,893. Gold papers over it with 4,413 placeholder rows from sales lines (17,347 rows for 12,934 items), which carry only a description and a unit. `item_key` is unresolved on 11,944 invoice lines, 55,052 supplier invoice lines, 641 purchase order lines and 2,906 movement lines (70,543 rows).
- Fix: `inventory_id = COALESCE(NULLIF(TRIM(inventoryNumber), ''), inventoryId)`. After it every code finds its master row (100%) and the placeholders disappear. Assertion `assert_gold_fact_keys_resolve` (fails, 4 of 7 rows).

**5. `dim_location` collapses 35,726 locations to 149 (A-drop)**

- Effect: `dim_location.sqlx:73` keys on the location id alone, which is unique only inside a business account (29,008 accounts; `HOVED` belongs to 26,628). 31,033 of 31,466 customer invoices (98.6%) with a known own location get the address of another account's location. Gold `dim_location` inherits it.
- Fix: Add the account to the key, to gold `location_key` and to the fact joins. Assertion `assert_location_rows_complete` (fails, 12 tenants).

**6. `fact_project_budget` loses 70 budget lines, 10.9M to 23.6M actual (A-drop)**

- Effect: `fact_project_budget.sqlx:15` omits `inventoryNumber`. 2 tenants (f630d6b0 -65, 2b6e5cd6 -5). The rows are stable, the amount is not, because the survivor of a collision is arbitrary: `actual_amount` +10,899,159.89 when fixed in the build of 2026-10-13 00:24 CEST (income +7,495,535.94, expense +3,403,623.95; revised +11,155,485.82, original +11,344,386.01) against +23,555,150.91 in the build before (income +15,829,130.51, expense +7,726,020.40). `BI_ProjectBudgetRecon` understates the budget side of the affected projects.
- Fix: Add `src.inventoryNumber` to the `PARTITION BY`. Assertion `assert_project_budget_rows_complete` (fails, 2 tenants).

**7. `fact_customer_payment` loses 134 payments or their voids (A-drop)**

- Effect: `fact_customer_payment.sqlx:91` omits the type. 130 Payment/VoidPayment and 4 Refund/VoidRefund pairs share a refNbr; silver keeps the void in 122 pairs and the original in 12, so 122 voids stand without their payment. `payment_amount` is 4,423,283.99 too low in both builds measured (the pairs net to zero).
- Fix: Add `source_data.payment_type`. Assertion `assert_customer_payment_rows_complete` (fails, 11 tenants).

**8. `fact_supplier_payment` loses 108 payments or their voids (A-drop)**

- Effect: `fact_supplier_payment.sqlx:82`, same cause: 72 Payment/VoidPayment, 27 Prepayment/VoidPayment, 9 Refund/VoidRefund pairs. `payment_amount` is wrong in a direction that changes with the build: 1,221,033.56 too low before the rebuild of 2026-10-13 00:24 CEST, 686,699.58 too high after it (silver keeps the void in 47 of the 108 pairs), because the survivor of a collision is arbitrary.
- Fix: Add `source_data.payment_type`. Assertion `assert_supplier_payment_rows_complete` (fails, 12 tenants).

**9. Gold `fixed_asset_key` carries a business unit that differs between fact and dimension (A-key)**

- Effect: 682 of 3,339 asset transactions (20.43%) find no asset: the fact's unit is mapped by transaction date, the dimension's is the tenant's current unit.
- Fix: Drop `business_unit_id` from both keys (as for `warehouse_key`). Tested: 0 unresolved after. Assertion `assert_gold_fact_keys_resolve` (fails).

**10. Attributes that are NULL on every row (A-null)**

- Effect: `dim_financial_period` start and end date, fiscal year and month (696 rows) and `dim_project` start and end date (2,602 rows): `SAFE_CAST` of `2022-11-01T00:00:00` to DATE. `dim_currency_rate` effective date and exchange rate (33,871 rows; unused downstream): the same cast, and `multDiv` is tested against `M` and `D` while Visma sends `Multiply` and `Divide`. `fact_purchase_order_line.promised_date` and `requested_date` (13,994 rows): the payload fields are `promised` and `requested`. `dim_inventory_item.default_warehouse_id` (12,934 rows): the payload key is `id`, not `warehouseId`.
- Fix: Cast with `COALESCE(SAFE_CAST(x AS DATE), DATE(SAFE_CAST(x AS DATETIME)))`; read the right payload fields. Not covered by an assertion.

**11. `fact_supplier_invoice` and `_line` lose 3 documents and 60 lines (A-drop)**

- Effect: `fact_supplier_invoice.sqlx:100` and `fact_supplier_invoice_line.sqlx:157` omit the document type; numbers 209487-209489 of tenant 33f319b9 are shared by two types. Zero-value documents: `vat_total` +864.33 when fixed, amounts unchanged.
- Fix: Add `src.document_type`. Assertion `assert_supplier_invoice_rows_complete` (fails, 1 tenant, header and line).

**12. Gold `purchase_order_key` carries the business unit and lacks the order type (A-key)**

- Effect: 13 of 13,994 purchase order lines (0.09%) find no header because header and line derive a different unit. A PO number shared by two order types (RegularOrder 3,831, DropShip 437, StandardBlanket 1) would collide: none does today.
- Fix: Hash tenant, order_type and order_nbr in both models. Tested: 0 unresolved after. Assertion `assert_gold_fact_keys_resolve` (fails).

## Class B, intended differences

`fact_project_transaction` (documents, one row per module and refNbr from the period slices), `fact_sales_order_business_unit` (aggregate of the lines, 38 orders without lines have no row), `dim_department` (+158 synthetic `Uspesifisert` rows), `dim_currency_market_rate` (direct and inverse), gold `dim_customer` (+11 referenced customers), gold `dim_supplier` (+151 referenced suppliers, +381 business unit rows), gold `dim_business_unit` (19 units for 20 validity windows), `dim_currency` (15 codes), `dim_currency_rate` (a copy of the market rates), `dim_date` (generated spine), `dim_missing_gl_accounts`, `dim_customer_combined`, `dim_entity`, `BI_ProjectPortfolioPnL` and `BI_ProjectBudgetRecon` (one row per project). Each source count above is the expected count under that rule and matches the model.

Latent, not a defect today: `fact_general_ledger` deduplicates on `(tenant, batch_number, ref_number, line_number)` without the module (staging key `(module, batchNumber, lineNumber)`), and `fact_project_portfolio_line` has `WHERE project_id IS NOT NULL` (line 123); neither removes a row now. `assert_general_ledger_rows_complete` and `assert_gold_fact_rows_complete` would show it.

## Class C, deleted source rows

None. Only 18 staging tables hold soft-deleted rows (17 DataMart `dm_*` tables and `purchase_receipt_transfers`); the 36 bronze tables silver reads hold 0 deleted rows, so the `isDeleted` filter of the models removes nothing.

## Per tenant: the models that lose rows

| Model | Tenant | Source keys | Model rows | Difference |
|---|---|---:|---:|---:|
| `dim_location` | 2b6e5cd6 Aquatiq AB | 164 | 4 | -160 |
| `dim_location` | 33f319b9 Aquatiq AS | 14,557 | 116 | -14,441 |
| `dim_location` | 61320c0a Aquatiq Hygiene Systems AS | 5,170 | 3 | -5,167 |
| `dim_location` | 7d2c489e Aquatiq Group AS | 71 | 2 | -69 |
| `dim_location` | 9b1fa69b Consult AS | 4,519 | 3 | -4,516 |
| `dim_location` | ab0e5d06 Aqua Pharma AS | 99 | 2 | -97 |
| `dim_location` | bb6c024e Aquatiq Sense AS | 5,232 | 3 | -5,229 |
| `dim_location` | bed46bb6 Aquatiq Food Automation ApS | 46 | 4 | -42 |
| `dim_location` | d1507c4a Aquatiq Aps | 13 | 4 | -9 |
| `dim_location` | eda0dd16 Sensilist AS | 463 | 3 | -460 |
| `dim_location` | f630d6b0 Aquatiq Food Factory AS | 5,296 | 3 | -5,293 |
| `dim_location` | ffa4a061 Aqua Pharma Group AS | 96 | 2 | -94 |
| `fact_customer_payment` | 2b6e5cd6 Aquatiq AB | 481 | 460 | -21 |
| `fact_customer_payment` | 33f319b9 Aquatiq AS | 21,482 | 21,460 | -22 |
| `fact_customer_payment` | 61320c0a Aquatiq Hygiene Systems AS | 2,396 | 2,379 | -17 |
| `fact_customer_payment` | 7d2c489e Aquatiq Group AS | 355 | 351 | -4 |
| `fact_customer_payment` | 9b1fa69b Consult AS | 3,565 | 3,553 | -12 |
| `fact_customer_payment` | ab0e5d06 Aqua Pharma AS | 150 | 148 | -2 |
| `fact_customer_payment` | bb6c024e Aquatiq Sense AS | 809 | 790 | -19 |
| `fact_customer_payment` | bed46bb6 Aquatiq Food Automation ApS | 255 | 241 | -14 |
| `fact_customer_payment` | eda0dd16 Sensilist AS | 167 | 166 | -1 |
| `fact_customer_payment` | f630d6b0 Aquatiq Food Factory AS | 271 | 259 | -12 |
| `fact_customer_payment` | ffa4a061 Aqua Pharma Group AS | 262 | 252 | -10 |
| `fact_project_budget` | 2b6e5cd6 Aquatiq AB | 355 | 350 | -5 |
| `fact_project_budget` | f630d6b0 Aquatiq Food Factory AS | 2,056 | 1,991 | -65 |
| `fact_supplier_invoice` | 33f319b9 Aquatiq AS | 11,519 | 11,516 | -3 |
| `fact_supplier_invoice_line` | 33f319b9 Aquatiq AS | 40,311 | 40,251 | -60 |
| `fact_supplier_payment` | 2b6e5cd6 Aquatiq AB | 389 | 385 | -4 |
| `fact_supplier_payment` | 33f319b9 Aquatiq AS | 9,145 | 9,120 | -25 |
| `fact_supplier_payment` | 61320c0a Aquatiq Hygiene Systems AS | 4,655 | 4,638 | -17 |
| `fact_supplier_payment` | 7d2c489e Aquatiq Group AS | 3,760 | 3,751 | -9 |
| `fact_supplier_payment` | 9b1fa69b Consult AS | 431 | 428 | -3 |
| `fact_supplier_payment` | ab0e5d06 Aqua Pharma AS | 1,313 | 1,311 | -2 |
| `fact_supplier_payment` | bb6c024e Aquatiq Sense AS | 646 | 645 | -1 |
| `fact_supplier_payment` | bed46bb6 Aquatiq Food Automation ApS | 415 | 391 | -24 |
| `fact_supplier_payment` | d1507c4a Aquatiq Aps | 142 | 140 | -2 |
| `fact_supplier_payment` | eda0dd16 Sensilist AS | 641 | 639 | -2 |
| `fact_supplier_payment` | f630d6b0 Aquatiq Food Factory AS | 1,316 | 1,300 | -16 |
| `fact_supplier_payment` | ffa4a061 Aqua Pharma Group AS | 1,154 | 1,151 | -3 |

## Business keys of the gold facts against their dimensions

119 pairs: the 48 business keys of the gold facts listed in the script (46 of them in `assert_gold_fact_keys_resolve`, plus the two `department_key` pairs) and the 71 `company_key`, `date_key` and `currency_key` columns of the gold and datamart tables. Only the unresolved ones are shown.

| Fact | Key | Dimension | Keyed rows | Unresolved | % | Known |
|---|---|---|---:|---:|---:|---|
| `fact_customer_invoice_line` | `item_key` | inventory_item | 84,412 | 11,944 | 14.15 | A-key: item dimension keyed on the internal id |
| `fact_fixed_asset_transaction` | `fixed_asset_key` | fixed_asset | 3,339 | 682 | 20.43 | A-key: business unit inside the key |
| `fact_general_ledger_balance` | `business_unit_key` | business_unit | 450,798 | 450,798 | 100.00 | A-key: silver key with the validity date (GL balance) |
| `fact_inventory_movement_line` | `item_key` | inventory_item | 11,893 | 2,906 | 24.43 | A-key: item dimension keyed on the internal id |
| `fact_supplier_invoice_line` | `item_key` | inventory_item | 77,062 | 55,052 | 71.44 | A-key: item dimension keyed on the internal id |
| `fact_purchase_order_line` | `item_key` | inventory_item | 13,898 | 641 | 4.61 | A-key: item dimension keyed on the internal id |
| `fact_purchase_order_line` | `purchase_order_key` | purchase_order | 13,994 | 13 | 0.09 | A-key: business unit inside the key |

The other 112 of 119 pairs resolve completely. `department_key` of the GL and the project lines resolves completely since the department dimension was rebuilt at 00:33 CEST by other work; in the build before, 61,642 GL rows (115 department codes) and 40,516 project lines had no department, and in the build before that 31,867 GL rows (745 codes).

## Silver models

Source keys: distinct staging keys in bronze (line models: array elements, views: the rows of the table they filter). Difference: model rows minus source keys.

| Model | Source | Grain | Key used | Source keys | Model rows | Difference | Class | Cause | Proposed fix and measured effect | Assertion |
|---|---|---|---|---:|---:|---:|---|---|---|---|
| `dim_account` | visma_accounts | chart of accounts | (tenant, accountCD) | 5,821 | 5,821 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_branch` | visma_branches | per tenant + branch | (tenant, branchId) | 12 | 12 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_business_unit` | silver_excel.business_unit_mapping | historized business unit | (tenant, unit, validity window) | 20 | 20 | 0 | exact | - | - | - |
| `dim_cash_account` | visma_cash_accounts | cash and bank accounts | (tenant, number) | 156 | 156 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_company` | visma_branches | per tenant + branch | (tenant, branchId) | 12 | 12 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_currency` | visma_currencies | tenant currency | (tenant, id) | 122 | 122 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_currency_market_rate` | dw_staging_currency.HistoryratesPowerBI | pair + date + direction | (base, quote, date), direct and inverse | 23,368 | 23,368 | 0 | B | Source keys shown as two times the 11,684 pairs: every pair is stored direct and inverse. | - | - |
| `dim_currency_rate` | visma_currency_rates | (tenant, type, date, from, to) | the same; staging key is id | 33,871 | 33,871 | 0 | A-null | `dim_currency_rate.sqlx:29` `SAFE_CAST(effectiveDate AS DATE)` is NULL on a datetime string; line 38 tests `multDiv = 'M'` or `'D'` while Visma sends `Multiply` and `Divide`, so `exchange_rate` is NULL. | Same cast fix, compare with `Multiply` and `Divide`. `effective_date`, `exchange_rate`, `inverse_exchange_rate` and `visma_timestamp` are NULL on all 33,871 rows; nothing downstream reads the table. | `assert_dimension_rows_complete` |
| `dim_customer` | visma_customers | customer master | (tenant, number) | 27,287 | 27,287 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_department` | visma_departments | per (tenant, department) | (tenant, departmentId) | 94 | 252 | +158 | B | Intended: the model adds a synthetic `Uspesifisert` row for every department code of the subaccounts that the master list lacks: +158 in the build of 2026-10-13 00:33 CEST (it added only the 12 zero-only placeholders before). The master rows (94) are all present: `assert_dimension_rows_complete` counts the rows that have a sync time. | - | `assert_dimension_rows_complete` |
| `dim_employee` | visma_employees | employee master | (tenant, employeeId) | 227 | 227 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_financial_period` | visma_financial_periods | fiscal period | (tenant, period); staging (year, period) | 696 | 696 | 0 | A-null | `dim_financial_period.sqlx:23-26`, `SAFE_CAST(startDate AS DATE)` is NULL on `'2022-11-01T00:00:00'`. | `COALESCE(SAFE_CAST(x AS DATE), DATE(SAFE_CAST(x AS DATETIME)))` for both dates. `start_date`, `end_date`, `fiscal_year` and `fiscal_month` are NULL on all 696 rows. | `assert_dimension_rows_complete` |
| `dim_fixed_asset` | visma_fixed_assets | asset register | (tenant, assetID) | 163 | 163 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_inventory_item` | visma_inventory_items | per tenant + inventory | (tenant, inventoryId); staging: number | 12,934 | 12,934 | 0 | A-key | `dim_inventory_item.sqlx:28`, `inventoryId AS inventory_id` is Visma's numeric internal id (2688461). Every fact carries the item code (`inventoryNumber`, KJ1015) and the gold dimension keys `item_key` on `inventory_id`. | `inventory_id = COALESCE(NULLIF(TRIM(inventoryNumber), ''), inventoryId)`; the numeric id stays in `inventory_internal_id`. Fact lines that reach a master item today: invoice 381 of 84,412, supplier invoice 2,745 of 77,062, purchase order 5 of 13,898, sales order 13 of 90,159, movement 0 of 11,893; all of them after the fix. | `assert_gold_fact_keys_resolve` |
| `dim_ledger` | visma_ledgers | GL ledger | (tenant, number) | 48 | 48 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_location` | visma_locations | location master | (tenant, locationId); staging adds baccount.number | 35,726 | 149 | -35,577 | A-drop | `dim_location.sqlx:73`, `PARTITION BY source_data.tenant_id, source_data.location_id`. A location id is unique only inside its business account: 29,008 accounts, `HOVED` alone belongs to 26,628. The latest-modified account wins and its address is attached to every document with that id. | Add the account (`JSON_VALUE(baccount, '$.number')`) to the key and the dimension, then to gold `location_key` and the gold fact joins (customer_id or supplier_id plus location_id). 149 to 35,726 rows (+35,577, 12 tenants). Of 31,466 customer invoices with a known own location, 31,033 (98.6%) get another account's address. | `assert_location_rows_complete` |
| `dim_project` | visma_projects | project master | (tenant, projectID) | 2,602 | 2,602 | 0 | A-null | `dim_project.sqlx:20-21`, `SAFE_CAST(startDate AS DATE)` is NULL on `'2023-01-01T00:00:00'`. | Same cast fix. `start_date` and `end_date` are NULL on all 2,602 rows. | `assert_dimension_rows_complete` |
| `dim_sales_person` | visma_sales_persons | salesperson master | (tenant, salespersonID) | 133 | 133 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_subaccount` | visma_subaccounts | subaccount segments | (tenant, subaccountId) | 13,611 | 13,611 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_supplier` | visma_suppliers | supplier master | (tenant, number) | 3,123 | 3,123 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_warehouse` | visma_warehouses | warehouse master | (tenant, warehouseID) | 36 | 36 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `dim_warehouse_location` | visma_warehouses.locations | (tenant, warehouse, location) | (tenant, warehouse, location) | 2,013 | 2,013 | 0 | exact | - | - | `assert_dimension_rows_complete` |
| `fact_cash_transaction` | visma_cash_transactions | cash transaction | (tenant, ref, type) | 4,345 | 4,345 | 0 | exact | - | - | `assert_cash_transaction_rows_complete` |
| `fact_customer_credit_note` | visma_customer_credit_notes | per credit note (tenant + ref) | (tenant, ref) | 1,421 | 1,421 | 0 | exact | - | - | `assert_customer_credit_note_rows_complete` |
| `fact_customer_invoice` | visma_customer_invoices | invoice header | (tenant, ref, type) | 31,520 | 31,520 | 0 | exact | - | - | `assert_customer_invoice_rows_complete` |
| `fact_customer_invoice_line` | visma_customer_invoices.invoiceLines | invoice line | (tenant, ref, type, line) | 99,847 | 99,847 | 0 | exact | - | - | `assert_customer_invoice_rows_complete` |
| `fact_customer_payment` | visma_customer_payments | per payment (tenant + refNbr) | (tenant, refNbr) | 30,237 | 30,103 | -134 | A-drop | `fact_customer_payment.sqlx:91`, `bu_match_rank` is `PARTITION BY source_data.tenant_id, source_data.ref_nbr`. A Payment and its VoidPayment (130 numbers) and a Refund and its VoidRefund (4) share one refNbr. The type is not in the key and the tie-break (business unit mapping) is arbitrary, so which half survives is random. | Add `source_data.payment_type` to that `PARTITION BY` and to the stated grain. +134 rows in 11 tenants (30,103 to 30,237); `payment_amount` +4,423,283.99 net in both builds measured: silver keeps the void in 122 of the 134 pairs, a void without its payment. Gold `fact_customer_payment` +134. | `assert_customer_payment_rows_complete` |
| `fact_fixed_asset_transaction` | visma_fixed_asset_transactions | (tenant + refNo + lineNo) | (tenant, refNo, lineNo) | 3,339 | 3,339 | 0 | exact | - | - | `assert_fixed_asset_transaction_rows_complete` |
| `fact_general_ledger` | visma_general_ledger_transactions | GL line | (tenant, batch, ref, line) | 1,108,577 | 1,108,577 | 0 | exact | - | - | `assert_general_ledger_rows_complete` |
| `fact_general_ledger_balance` | visma_general_ledger_balances_v2 | GL balance | (tenant, account, subaccount, ledger, period) | 450,798 | 450,798 | 0 | exact | - | - | `assert_general_ledger_rows_complete` |
| `fact_inventory_adjustment` | fact_inventory_movement where type = ADJUSTMENT | view | filter by type | 6,474 | 6,474 | 0 | exact | - | - | - |
| `fact_inventory_adjustment_line` | fact_inventory_movement_line where type = ADJUSTMENT | view | filter by type | 33,865 | 33,865 | 0 | exact | - | - | - |
| `fact_inventory_issue` | fact_inventory_movement where type = ISSUE | view | filter by type | 24,664 | 24,664 | 0 | exact | - | - | - |
| `fact_inventory_issue_line` | fact_inventory_movement_line where type = ISSUE | view | filter by type | 68,035 | 68,035 | 0 | exact | - | - | - |
| `fact_inventory_movement` | the four visma_inventory_* tables | (tenant + type + reference) | (tenant, type, reference) | 36,579 | 36,579 | 0 | exact | - | - | `assert_inventory_movement_rows_complete` |
| `fact_inventory_movement_line` | the four line arrays | (tenant + type + reference + line) | (tenant, type, reference, line, item) | 137,657 | 137,657 | 0 | A-null | `fact_inventory_movement_line.sqlx:46, 83, 120` (adjustment, issue, receipt) read `$.inventory.id`; Visma sends `inventoryItem.number`, which the transfer branch (line 157) already reads. `warehouse.id` is space padded and not trimmed (lines 48, 85, 122), and `amount` reads `$.amount` while Visma sends `extCost`. | Read `$.inventoryItem.number`, TRIM the warehouse ids, read `$.extCost`. `inventory_id` NULL on 125,764 of 137,657 lines (91.4%), all found by `inventoryItem.number`; warehouse key NULL on the same lines; `amount` NULL on all 137,657 (`extCost` sums to 750,702,874.59: adjustments 17,237,998.05, issues 340,425,540.73, receipts 393,039,335.81). | `assert_line_item_codes_populated` |
| `fact_inventory_receipt` | fact_inventory_movement where type = RECEIPT | view | filter by type | 4,440 | 4,440 | 0 | exact | - | - | - |
| `fact_inventory_receipt_line` | fact_inventory_movement_line where type = RECEIPT | view | filter by type | 23,864 | 23,864 | 0 | exact | - | - | - |
| `fact_inventory_transfer` | fact_inventory_movement where type = TRANSFER | view | filter by type | 1,001 | 1,001 | 0 | exact | - | - | - |
| `fact_inventory_transfer_line` | fact_inventory_movement_line where type = TRANSFER | view | filter by type | 11,893 | 11,893 | 0 | exact | - | - | - |
| `fact_project_budget` | visma_project_budgets | per project + task + account group | (tenant, project, task, group) | 19,241 | 19,171 | -70 | A-drop | `fact_project_budget.sqlx:15`, `rn` is `PARTITION BY src.tenantId, src.projectID, src.projectTaskID, src.accountGroupID`. `inventoryNumber`, the fourth staging key part, is missing. 69 groups hold an `<N/A>` roll-up line next to item lines (139 rows, 70 lost) and 61 of them carry different amounts; the survivor is arbitrary. | Add `src.inventoryNumber` (already selected). +70 rows in 2 tenants (19,171 to 19,241). `actual_amount` +10,899,159.89 in the build of 2026-10-13 00:24 CEST (income +7,495,535.94 on 14 rows, expense +3,403,623.95 on 56; revised budget +11,155,485.82, original +11,344,386.01) and +23,555,150.91 in the build before: the survivor changes between builds. `BI_ProjectBudgetRecon` budget side is understated for those projects. | `assert_project_budget_rows_complete` |
| `fact_project_transaction` | visma_project_transactions | per document (tenant, module, ref) | (tenant, module, ref) | 38,199 | 38,199 | 0 | B | Intended aggregation: one header per document, line totals cover every period slice. | - | `assert_project_transaction_documents_complete` |
| `fact_project_transaction_line` | visma_project_transactions.lines | tenant + module + ref + tran_id | (tenant, module, ref, period slice, tran) | 209,886 | 209,886 | 0 | exact | - | - | `assert_project_transaction_lines_complete` |
| `fact_purchase_order` | visma_purchase_orders | (tenant + type + number) | (tenant, type, number) | 4,269 | 4,269 | 0 | exact | - | - | `assert_purchase_order_rows_complete` |
| `fact_purchase_order_line` | visma_purchase_orders.lines | PO line | (tenant, type, number, line, item) | 13,994 | 13,994 | 0 | exact | - | - | `assert_purchase_order_rows_complete` |
| `fact_sales_order` | visma_sales_orders_v3 | (tenant, type, id) | (tenant, type, id) | 27,377 | 27,377 | 0 | exact | - | - | `assert_sales_order_rows_complete` |
| `fact_sales_order_business_unit` | silver.fact_sales_order_line | tenant + order + line unit | the same | 27,359 | 27,359 | 0 | B | Intended aggregation of the lines. An order without lines has no row (38 of 27,377 orders); orders that split over several business units add 20 rows. | - | - |
| `fact_sales_order_line` | visma_sales_orders_v3.orderLines | (tenant, type, id, line) | (tenant, type, id, line) | 90,161 | 90,161 | 0 | exact | - | - | `assert_sales_order_rows_complete` |
| `fact_shipment` | visma_shipments | per shipment (tenant + number) | (tenant, number), freshest sync | 25,000 | 25,000 | 0 | exact | - | - | `assert_shipment_rows_complete` |
| `fact_shipment_line` | visma_shipments.shipmentDetailLines | (tenant + shipment + line) | (tenant, shipment, line, item) | 77,607 | 77,607 | 0 | A-null | `fact_shipment_line.sqlx:57` reads `$.inventory.id`, `$."inventory.id"`, `$.inventory` and `$.inventoryId`; Visma sends the item as `inventoryNumber`. `inventory_id` is NULL on every line, so gold `item_key` is NULL. | Add `JSON_VALUE(line, '$.inventoryNumber')` to that COALESCE. 77,607 of 77,607 lines (100%) have `inventoryNumber` and all match `dim_inventory_item.inventory_number`. | `assert_line_item_codes_populated` |
| `fact_supplier_invoice` | visma_supplier_invoices | per invoice (tenant + ref) | (tenant, ref) | 30,403 | 30,400 | -3 | A-drop | `fact_supplier_invoice.sqlx:100`, `PARTITION BY src.tenant_id, src.reference_number`. Numbers 209487, 209488 and 209489 of tenant 33f319b9 are shared by CreditAdj and DebitAdj, and by DebitAdj and Invoice. | Add `src.document_type`. +3 headers (30,400 to 30,403); `vat_total` +864.33, `detail_total` 0.00. | `assert_supplier_invoice_rows_complete` |
| `fact_supplier_invoice_line` | visma_supplier_invoices.invoiceLines | per line (tenant + ref + line) | (tenant, ref, line, item) | 112,571 | 112,511 | -60 | A-drop | `fact_supplier_invoice_line.sqlx:157`, `PARTITION BY src.tenant_id, src.reference_number, src.line_number, src.inventory_id`: no document type, the same three numbers. | Add `src.document_type`. +60 lines (112,511 to 112,571); `amount` 0.00 net. | `assert_supplier_invoice_rows_complete` |
| `fact_supplier_payment` | visma_supplier_payments | per payment (tenant + refNbr) | (tenant, refNbr) | 24,007 | 23,899 | -108 | A-drop | `fact_supplier_payment.sqlx:82`, same `PARTITION BY source_data.tenant_id, source_data.ref_nbr`. Payment and VoidPayment share 72 numbers, Prepayment and VoidPayment 27, Refund and VoidRefund 9. | Add `source_data.payment_type`. +108 rows in 12 tenants (23,899 to 24,007). `payment_amount`: silver is 686,699.58 too high in the build of 2026-10-13 00:24 CEST (it keeps the void in 47 of the 108 pairs) and was 1,221,033.56 too low in the build before: the survivor changes between builds. Gold `fact_supplier_payment` +108. | `assert_supplier_payment_rows_complete` |

## Gold and datamart models

Source keys here are the rows of the silver model (or the expected rows under the stated rule); `-` where the model is derived.

| Model | Source | Grain | Key used | Source keys | Model rows | Difference | Class | Cause | Proposed fix and measured effect | Assertion |
|---|---|---|---|---:|---:|---:|---|---|---|---|
| `datamart.BI_FinanceTable` | silver.fact_general_ledger | per GL line | silver key | 1,108,577 | 1,108,577 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `datamart.BI_ProjectBudgetRecon` | fact_project_budget + BI_ProjectPortfolioPnL | per tenant + project | (tenant, project) | 2,234 | 2,234 | 0 | B | Intended outer join of project budget and P&L; the budget side inherits the `fact_project_budget` drop. | - | - |
| `datamart.BI_ProjectPortfolioPnL` | gold_finance.fact_project_portfolio_line | per tenant + project | (tenant, project) | 2,184 | 2,184 | 0 | B | Intended aggregation of the lines to one row per project. | - | - |
| `datamart.BI_SalesOrder` | silver.fact_sales_order_line | per sales order line | silver key | 90,161 | 90,161 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `datamart.RLS_sales_person` | dim_sales_person emails | user to salesperson | (user, tenant, salesperson) | - | 96 | - | derived | Mapping built from the active salesperson emails (96); no source count. | - | - |
| `gold_common.dim_business_unit` | silver.dim_business_unit | tenant + business unit | (tenant, unit) | 19 | 19 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_company` | silver.dim_branch | per tenant | (tenant) | 12 | 12 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_currency` | silver.dim_currency + rate feed | currency code | (code) | 15 | 15 | 0 | B | Intended global code grain: 15 codes from 122 tenant rows and the rate feed. | - | - |
| `gold_common.dim_currency_market_rate` | silver.dim_currency_market_rate | pair + date | silver key | 23,368 | 23,368 | 0 | exact | - | - | - |
| `gold_common.dim_currency_market_rate_to_nok_daily` | market rates, fill-forward | daily bridge | (date, currency) | - | 21,915 | - | derived | Fill-forward daily spine over the rate range; unique on (date_key, currency_key). | - | - |
| `gold_common.dim_currency_rate` | silver.dim_currency_market_rate | pair + date | silver key | 23,368 | 23,368 | 0 | B | A copy of `dim_currency_market_rate`; the tenant rates of silver `dim_currency_rate` (33,871 rows) are not used. | - | - |
| `gold_common.dim_customer_combined` | silver_common.bridge_customer_to_combined | combined customer | (identity) | 5,077 | 5,077 | 0 | B | Intended cross-tenant deduplication of 27,298 customers. | - | - |
| `gold_common.dim_date` | generated spine 2020-2030 | per day | date | 4,018 | 4,018 | 0 | B | Generated spine 2020-01-01 to 2030-12-31: a fact dated outside it gets no date row (none today). | - | - |
| `gold_common.dim_department` | silver.dim_department | department | key | 252 | 252 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_employee` | silver.dim_employee | employee | key | 227 | 227 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_entity` | gold_common.dim_company entity codes | reporting entity | (entity code) | 12 | 12 | 0 | B | Intended grouping of the 12 legal entities. | - | - |
| `gold_common.dim_financial_period` | silver.dim_financial_period | period | key | 696 | 696 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_inventory_item` | silver.dim_inventory_item | tenant + item | key | 12,934 | 17,347 | +4,413 | A-key | Inherits silver `dim_inventory_item` (key = internal id) and adds 4,413 placeholder rows built from sales order lines, keyed on the item code the facts use. The master rows and the placeholders describe the same items. | See silver `dim_inventory_item`. 17,347 rows for 12,934 items; no placeholders after. | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_location` | silver.dim_location | location | key | 149 | 149 | 0 | A-drop | Inherits silver `dim_location`: 149 of 35,726 locations. | See silver `dim_location`. | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_missing_gl_accounts` | silver.dim_account without GL lines | account no GL line uses | (tenant, account) | 3,513 | 3,513 | 0 | B | Intended anti-join: 3,513 accounts of the master without a GL line. | - | - |
| `gold_common.dim_project` | silver.dim_project | project | key | 2,602 | 2,602 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_sales_person` | silver.dim_sales_person | salesperson | key | 133 | 133 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_subaccount` | silver.dim_subaccount | subaccount | key | 13,611 | 13,611 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_common.dim_warehouse` | silver.dim_warehouse | warehouse | key | 36 | 36 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_finance.dim_account` | silver.dim_account | account | key | 5,821 | 5,821 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_finance.dim_customer` | silver.dim_customer | tenant + customer | key | 27,287 | 27,298 | +11 | B | Intended: master customers plus customers that invoices, payments, credit notes or orders reference but the master lacks (+11). All 27,287 master customers are present. | - | `assert_gold_dimension_rows_complete` |
| `gold_finance.dim_fixed_asset` | silver.dim_fixed_asset | fixed asset | key | 163 | 163 | 0 | exact | - | - | `assert_gold_dimension_rows_complete` |
| `gold_finance.dim_inventory_item` | gold_common.dim_inventory_item (view) | view | select * | 17,347 | 17,347 | 0 | exact | - | - | - |
| `gold_finance.dim_supplier` | silver.dim_supplier | tenant + business unit + supplier | key | 3,123 | 3,655 | +532 | B | Intended: master suppliers plus 151 that documents reference but the master lacks, and one row per business unit (381 extra rows). All 3,123 master suppliers are present. | - | `assert_gold_dimension_rows_complete` |
| `gold_finance.fact_customer_credit_note` | silver.fact_customer_credit_note | credit note | silver key | 1,421 | 1,421 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_customer_invoice` | silver.fact_customer_invoice | invoice header | silver key | 31,520 | 31,520 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_customer_invoice_line` | silver.fact_customer_invoice_line | invoice line | silver key | 99,847 | 99,847 | 0 | A-key | `item_key` is unresolved on 11,944 of the 84,412 lines that have one (14.15%): the item dimension is keyed on the internal id (see `dim_inventory_item`). | Fixed by the `dim_inventory_item` fix: all 84,412 codes exist in the master by number. | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_customer_payment` | silver.fact_customer_payment | payment | silver key | 30,103 | 30,103 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_fixed_asset_transaction` | silver.fact_fixed_asset_transaction | asset txn | silver key | 3,339 | 3,339 | 0 | A-key | `fixed_asset_key` is the hash of tenant, business_unit_id and asset_id in the fact (`fact_fixed_asset_transaction.sqlx:25`) and in `dim_fixed_asset.sqlx:14`. The fact's business unit is mapped by transaction date, the dimension's is the tenant's current one: they differ on 682 of 3,339 rows. | Drop `business_unit_id` from both keys, as was done for `warehouse_key`. 682 of 3,339 transactions (20.43%) have no asset row; 0 after. | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_general_ledger` | silver.fact_general_ledger | GL journal line | silver key | 1,108,577 | 1,108,577 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_general_ledger_balance` | silver.fact_general_ledger_balance | GL balance | silver key | 450,798 | 450,798 | 0 | A-key | `fact_general_ledger_balance.sqlx:40` takes `bu.business_unit_key` from silver `dim_business_unit`: a STRING hash of tenant, business unit and `bu_valid_from`. Gold `dim_business_unit` and `fact_general_ledger` use the INT64 hash of tenant and business unit, so no balance row ever joins. | Compute `business_unit_key` as `fact_general_ledger.sqlx` lines 36-41 do. 450,798 of 450,798 rows (100%) carry a key that is in no dimension row; 0 after (tested on the data). | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_inventory_adjustment` | fact_inventory_movement where type = ADJUSTMENT | view | filter by type | 6,474 | 6,474 | 0 | exact | - | - | - |
| `gold_finance.fact_inventory_adjustment_line` | fact_inventory_movement_line where type = ADJUSTMENT | view | filter by type | 33,865 | 33,865 | 0 | exact | - | - | - |
| `gold_finance.fact_inventory_issue` | fact_inventory_movement where type = ISSUE | view | filter by type | 24,664 | 24,664 | 0 | exact | - | - | - |
| `gold_finance.fact_inventory_issue_line` | fact_inventory_movement_line where type = ISSUE | view | filter by type | 68,035 | 68,035 | 0 | exact | - | - | - |
| `gold_finance.fact_inventory_movement` | silver.fact_inventory_movement | movement header | silver key | 36,579 | 36,579 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_inventory_movement_line` | silver.fact_inventory_movement_line | movement line | silver key | 137,657 | 137,657 | 0 | A-null | Inherits silver `fact_inventory_movement_line`: `item_key` and both warehouse keys are NULL on 125,764 of 137,657 rows; `location_key` is NULL on all (bins again). | See the silver model. | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_inventory_receipt` | fact_inventory_movement where type = RECEIPT | view | filter by type | 4,440 | 4,440 | 0 | exact | - | - | - |
| `gold_finance.fact_inventory_receipt_line` | fact_inventory_movement_line where type = RECEIPT | view | filter by type | 23,864 | 23,864 | 0 | exact | - | - | - |
| `gold_finance.fact_inventory_transfer` | fact_inventory_movement where type = TRANSFER | view | filter by type | 1,001 | 1,001 | 0 | exact | - | - | - |
| `gold_finance.fact_inventory_transfer_line` | fact_inventory_movement_line where type = TRANSFER | view | filter by type | 11,893 | 11,893 | 0 | exact | - | - | - |
| `gold_finance.fact_project_portfolio_line` | silver.fact_project_transaction_line | project line | silver key | 209,886 | 209,886 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_supplier_invoice` | silver.fact_supplier_invoice | invoice header | silver key | 30,400 | 30,400 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_supplier_invoice_line` | silver.fact_supplier_invoice_line | invoice line | silver key | 112,511 | 112,511 | 0 | A-key | `item_key` is unresolved on 55,052 of the 77,062 lines that have one (71.44%): same cause. | Fixed by the `dim_inventory_item` fix: all 77,062 codes exist in the master by number. | `assert_gold_fact_rows_complete` |
| `gold_finance.fact_supplier_payment` | silver.fact_supplier_payment | payment | silver key | 23,899 | 23,899 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_sales.dim_purchase_order` | silver.fact_purchase_order | PO header | silver key | 4,269 | 4,269 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_sales.dim_sales_order` | silver.fact_sales_order | sales order header | silver key | 27,377 | 27,377 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_sales.dim_shipment` | silver.fact_shipment | shipment header | silver key | 25,000 | 25,000 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_sales.fact_purchase_order_line` | silver.fact_purchase_order_line | PO line | silver key | 13,994 | 13,994 | 0 | A-key | `purchase_order_key` is the hash of tenant, business_unit_id and order_nbr in `dim_purchase_order.sqlx:14` and `fact_purchase_order_line.sqlx:37`. The header's business unit comes from its representative line, the line's from its own subaccount: they differ on 13 lines. `order_type` (RegularOrder, DropShip, StandardBlanket) is not in the key either. | Hash tenant, order_type and order_nbr in both models. 13 of 13,994 lines have no header row; 0 after. | `assert_gold_fact_rows_complete` |
| `gold_sales.fact_sales_order_line` | silver.fact_sales_order_line | sales order line | silver key | 90,161 | 90,161 | 0 | exact | - | - | `assert_gold_fact_rows_complete` |
| `gold_sales.fact_shipment_detail_line` | silver.fact_shipment_line | shipment line | silver key | 77,607 | 77,607 | 0 | A-null | Inherits `fact_shipment_line`: `item_key` NULL on 77,607 rows. `location_key` is NULL on 77,597: the lines hold warehouse bins (`1`, `10E4`), gold `dim_location` holds business-account locations. | See `fact_shipment_line`. Bins need a gold dimension built from `dim_warehouse_location`. | `assert_gold_fact_rows_complete` |

## Assertions added

All 19 are in `dataform/definitions/assertions/`, schema `visma_assertions`, in the pattern of `assert_sales_order_rows_complete`: they return the rows that differ, so zero rows is a pass. Pushed with `dataform/tools/deploy.py push` and run in the Dev workspace with `dataform/tools/deploy.py run --actions visma_assertions.<name>,...` (invocation `1791842091-1f67f2e8-...`, 12 SUCCEEDED, 7 FAILED). Row counts are `SELECT COUNT(*)` on the assertion views.

| Assertion | Covers | Rows |
|---|---|---:|
| `assert_customer_invoice_rows_complete` | fact_customer_invoice, _line | 0 |
| `assert_customer_credit_note_rows_complete` | fact_customer_credit_note | 0 |
| `assert_customer_payment_rows_complete` | fact_customer_payment (class A) | 11 |
| `assert_supplier_invoice_rows_complete` | fact_supplier_invoice, _line (class A) | 2 |
| `assert_supplier_payment_rows_complete` | fact_supplier_payment (class A) | 12 |
| `assert_general_ledger_rows_complete` | fact_general_ledger, _balance | 0 |
| `assert_purchase_order_rows_complete` | fact_purchase_order, _line | 0 |
| `assert_shipment_rows_complete` | fact_shipment, _line | 0 |
| `assert_inventory_movement_rows_complete` | fact_inventory_movement, _line, per type | 0 |
| `assert_cash_transaction_rows_complete` | fact_cash_transaction | 0 |
| `assert_fixed_asset_transaction_rows_complete` | fact_fixed_asset_transaction | 0 |
| `assert_project_budget_rows_complete` | fact_project_budget (class A) | 2 |
| `assert_project_transaction_documents_complete` | fact_project_transaction | 0 |
| `assert_dimension_rows_complete` | 19 silver dimensions | 0 |
| `assert_location_rows_complete` | dim_location (class A) | 12 |
| `assert_line_item_codes_populated` | item codes of shipment and movement lines (class A) | 20 |
| `assert_gold_fact_rows_complete` | 21 gold and datamart tables against silver | 0 |
| `assert_gold_dimension_rows_complete` | 15 gold dimensions against silver | 0 |
| `assert_gold_fact_keys_resolve` | 46 business keys of the gold facts (class A: 7 pairs) | 7 |

Already present and unchanged: `assert_sales_order_rows_complete` (0), `assert_project_transaction_lines_complete` (0). The failing ones stay failing until the models are fixed, so a Dataform run of the whole project shows FAILED because of them (they are not on `main`: nothing was committed).

## Bronze tables no model reads

31 of the 74 bronze tables are read by no silver, gold or assertion model (`visma_tenants` only by the bronze models themselves), so their records never reach a report. Not a drop inside a model, but a coverage statement: `visma_supplier_documents` 30,403, `visma_contacts` 17,412, `visma_supplier_locations` 8,741, `visma_purchase_receipts` 5,266, `visma_expense_claims` 4,832, `visma_countries` 3,120, `visma_kit_specifications` 1,283, `visma_kit_assemblies` 986, `visma_number_sequences` 908, `visma_project_account_groups` 832, `visma_vats` 671, `visma_vat_categories` 504, `visma_unit_of_measures` 312, `visma_dimensions` 276, `visma_sales_order_types` 167, `visma_credit_terms` 170, `visma_fixed_asset_classes` 146, `visma_vat_zones` 120, `visma_carriers` 109, `visma_deferral_codes` 77, `visma_payment_methods` 73, `visma_earning_types` 49, `visma_customer_classes` 46, `visma_customer_debit_notes` 45 (DebitNote documents, with 23 SmallCreditWo in `visma_customer_documents`, are in no AR fact), `visma_stocktakes_v2` 45, `visma_discount_codes` 32, `visma_landed_cost_codes` 19, `visma_lot_serial_classes` 14, `visma_tenants` 12, `visma_customer_payment_methods` 9, `visma_packaging_types` 3.

## Not verified

- Whether Visma holds records that staging lacks. Staging keys without a type part (`purchase_orders` on `orderNbr`, `shipments`, `cash_transactions`, the inventory movements, `customer_invoices` on `referenceNumber`) could have merged two documents of different types into one staging row; that needs Visma calls, which were out of scope. Evidence so far: `customer_invoices` returns one document type (Invoice), `shipments` two (Issue, Transfer) with unique numbers, `purchase_orders` three (RegularOrder, DropShip, StandardBlanket) and key `orderNbr` only.
- The old project (out of scope), the values of the silver and gold measures (only the amounts of the class A models were compared with bronze), the 14 `silver_common` and 17 `silver_excel` report, QA, bridge and seed models, and `bronze` itself beyond the row counts against staging.
- The 100% NULL columns of `fact_cash_transaction` (`document_ref_type` and `_number`: 146 of 4,345 bronze rows have a `documentRef`), `fact_purchase_order_line` (`account_id`, `amount_base`), `fact_sales_order` and `fact_sales_order_line` (v3 fields), `dim_customer` (`main_state`, `main_contact_name`), were not traced to the payload; they may simply be absent in Visma.
- The assertions ran in the Dev workspace only. `main` was not touched, so a run from `main` does not contain them. No fix was applied and none was run, so the "after the fix" numbers are tested on the data with SELECT queries, not by rebuilding the models.

