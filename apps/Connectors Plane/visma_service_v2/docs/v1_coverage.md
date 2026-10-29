# Does v2 have everything v1 has? Coverage inventory

Analysis only, written 2026-10-12 21:35 UTC. BigQuery table metadata and row counts were read at 2026-10-12T21:33:40Z; column fill counts and the read history about 40 minutes earlier (SELECT and INFORMATION_SCHEMA only, plus dataset ACLs of two datasets). The old project (`prj-dw-dev`, datasets `dw_1_*`) is the specification and was not changed; nothing was written to BigQuery, Visma was not called. Facts only; what could not be verified is in section 4 and in the rows themselves.

> **Update 2026-10-13.** The inventory below is a snapshot and is kept as written. Several of its gaps
> were closed the next day: the Excel loader (gap 7), the v1 quality checks as assertions (gap 9),
> `dim_inventory_item` and the other defects found by the completeness audit (gap 10 in part: the
> columns that v2 does not fetch from Visma remain), CI, the embedded migration runner and the
> production compose file (gap 11), the launcher default (gap 12), and the currency service's target
> (gap 2, in its template). Gaps 1, 3, 4 (alerting is built, the channel is not chosen), 5, 6 and 8
> still need a decision or access. The current status of each is in
> [PRODUCTION_READINESS.md](PRODUCTION_READINESS.md), "Open items".

## Summary

| | count |
|---|---:|
| Part 1: v1 tables and views in the 11 datasets the business or Dataform reads | 256 (232 tables, 24 views) |
| equivalents found (same name in the mapped v2 dataset) | 220 |
| of those, schema-identical (same columns, types, order, nullability) | 204 |
| of those, identical and v2 has extra columns (all v1 columns present) | 11 |
| with column gaps (columns missing in v2) | 5 |
| retyped | 0 |
| no equivalent in the mapped dataset but the same table exists in `visma_silver_common` | 10 |
| missing tables (no equivalent anywhere) | 26 |
| equivalents with a column filled in v1 and completely empty in v2 | 80 tables, 242 columns (222 v1 sync-bookkeeping, 20 other) |
| assertion views in `dw_1_silver_visma_global_assertions` | 98 (6 Visma, all with a v2 equivalent; 92 Xero, out of scope) |
| Part 2: service and operations items | 42: 16 covered, 7 replaced by design, 8 partial, 5 gaps, 6 not needed |

The data layer is structurally complete: every v1 silver, gold, datamart, Excel and currency object that the Dataform repository defines exists in v2 with the same columns and types, and bronze has every v1 business column (five reference tables lack loader metadata, eleven tables have extra columns). What v1 has and v2 does not is mostly operational: nothing runs by itself in the new project (the scheduler has not written to the new staging since the snapshot of 2026-10-09, Dataform has no schedule), the currency rates and the Excel mapping are one-off copies, no Power BI access is set up, and there is no alerting. The ranked list is in section 3.

## 1. Data coverage

### 1.1 Method

* Dataset mapping: `dw_1_bronze_visma` to `visma_bronze`, `dw_1_silver_visma` to `visma_silver`, and `dw_1_<layer>` to `visma_<layer>` for `silver_common`, `silver_excel`, `gold_common`, `gold_finance`, `gold_sales`; `dw_1_gold_bi` to `visma_datamart` (the old `datamart` dataset exists but is empty); `dw_1_bronze_excel` to `visma_bronze_excel`; `dw_1_staging_currency_n2` to `dw_staging_currency`. `dw_1_silver_quality` has no counterpart. Table names are unchanged.
* Existence, columns, types, order and nullability come from `INFORMATION_SCHEMA.TABLES` and `COLUMNS`; row counts from `__TABLES__`. "In the v1 repo" means the local `Dataform/` copy.
* Fill: for every pair, `COUNTIF(col IS NOT NULL)` per column on the whole table on both sides, plus for text columns a count that ignores blank, `null`, `[]`, `{}`. `dataform/tools/compare_columns.py` was run as well (output in the scratch folder, `docs/dataform_v1_*.md` untouched) and agrees.
* Reads: `INFORMATION_SCHEMA.JOBS_BY_PROJECT`, 45 and 180 days.

### 1.2 Result by dataset

| v1 dataset | v2 dataset | v1 objects | equivalent found | identical | identical + new columns | columns missing | retyped | relocated | missing | with columns empty in v2 |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `dw_1_bronze_visma` | `visma_bronze` | 76 | 73 | 57 | 11 | 5 | 0 | 0 | 3 | 71 |
| `dw_1_bronze_excel` | `visma_bronze_excel` | 2 | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 |
| `dw_1_silver_visma` | `visma_silver` | 72 | 53 | 53 | 0 | 0 | 0 | 0 | 19 | 1 |
| `dw_1_silver_common` | `visma_silver_common` | 14 | 14 | 14 | 0 | 0 | 0 | 0 | 0 | 1 |
| `dw_1_silver_excel` | `visma_silver_excel` | 17 | 17 | 17 | 0 | 0 | 0 | 0 | 0 | 2 |
| `dw_1_silver_quality` | `(none)` | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 4 | 0 |
| `dw_1_gold_common` | `visma_gold_common` | 29 | 19 | 19 | 0 | 0 | 0 | 10 | 0 | 1 |
| `dw_1_gold_finance` | `visma_gold_finance` | 26 | 26 | 26 | 0 | 0 | 0 | 0 | 0 | 4 |
| `dw_1_gold_sales` | `visma_gold_sales` | 6 | 6 | 6 | 0 | 0 | 0 | 0 | 0 | 0 |
| `dw_1_gold_bi` | `visma_datamart` | 6 | 6 | 6 | 0 | 0 | 0 | 0 | 0 | 0 |
| `dw_1_staging_currency_n2` | `dw_staging_currency` | 4 | 4 | 4 | 0 | 0 | 0 | 0 | 0 | 0 |
| **Total** | | **256** | **220** | **204** | **11** | **5** | **0** | **10** | **26** | **80** |

Assertion views are not in the table: of the 98 views in `dw_1_silver_visma_global_assertions`, 92 belong to the Xero project (84 `ods_xero_*` and 8 `assert_fact_*`) and 6 are Visma. `assert_purchase_order_line_quantities` and `assert_supplier_key_coverage` exist in `visma_assertions` under the same name; the four Excel ones (`chart_of_accounts` and `global_account_mapping`, `uniqueKey` and `rowConditions`) under a `visma_silver_excel_` prefix.

### 1.3 One row per v1 object

Status: *identical* = same column names, types, order and nullability; *identical + new columns* = all v1 columns present, v2 adds fields it fetches; *relocated* = no table of that name in the mapped dataset, an identical one in `visma_silver_common`. Rows are v1 / v2 from `__TABLES__` at the snapshot and are not compared; the "filled in v1" counts in the notes were taken about 20 minutes earlier, and v1 changes every hour. **B** = the three v1 sync-bookkeeping columns are empty in v2 (section 1.4). v1 bronze holds duplicate rows, so its counts are higher.

#### `dw_1_bronze_visma` to `visma_bronze`: 76 objects, 73 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `visma_accounts` | 12,230 / 5,821 | `visma_bronze.visma_accounts` | identical | B |
| `visma_attributes` | 494 / 133 | `visma_bronze.visma_attributes` | identical | B |
| `visma_branches` | 13 / 12 | `visma_bronze.visma_branches` | identical + new columns | +9 new: `bankSettings`, `currency`, `defaultCountry`, `deliveryAddress`, `deliveryContact`, `industryCode` ...; B |
| `visma_carriers` | 260 / 109 | `visma_bronze.visma_carriers` | identical | B |
| `visma_cash_accounts` | 313 / 156 | `visma_bronze.visma_cash_accounts` | identical | B |
| `visma_cash_transactions` | 17,015 / 4,345 | `visma_bronze.visma_cash_transactions` | identical | B |
| `visma_contacts` | 17,741 / 17,412 | `visma_bronze.visma_contacts` | identical | B |
| `visma_countries` | 260 / 3,120 | `visma_bronze.visma_countries` | **columns missing** | Missing: `partitionDate`, `metadata` (v1 ingestion metadata of an older loader; no business column). |
| `visma_credit_terms` | 186 / 170 | `visma_bronze.visma_credit_terms` | identical | B |
| `visma_currencies` | 511 / 122 | `visma_bronze.visma_currencies` | **columns missing** | Missing: `_validated_at`, `metadata`, `partitionDate`, `_validator` (v1 ingestion metadata of an older loader; no business column).; B |
| `visma_currency_rates` | 816,880 / 33,871 | `visma_bronze.visma_currency_rates` | identical | B |
| `visma_customer_classes` | 0 / 46 | `visma_bronze.visma_customer_classes` | identical |  |
| `visma_customer_credit_notes` | 9,162 / 1,421 | `visma_bronze.visma_customer_credit_notes` | identical + new columns | +3 new: `applications`, `invoiceAddress`, `invoiceContact`; B; empty in v2: `dunningLetterDate` (3 of 9,162 filled in v1), `dunningLetterLevel` (3 of 9,162 filled in v1) |
| `visma_customer_debit_notes` | 359 / 45 | `visma_bronze.visma_customer_debit_notes` | identical + new columns | +3 new: `applications`, `invoiceAddress`, `invoiceContact`; B; empty in v2: `dunningLetterDate` (18 of 359 filled in v1), `dunningLetterLevel` (18 of 359 filled in v1) |
| `visma_customer_documents` | 63,791 / 33,009 | `visma_bronze.visma_customer_documents` | identical | B |
| `visma_customer_invoices` | 48,953 / 31,520 | `visma_bronze.visma_customer_invoices` | identical | B; empty in v2: `errorInfo` (1 of 48,953 filled in v1) |
| `visma_customer_locations` | 17,887 / 32,309 | `visma_bronze.visma_customer_locations` | identical | B; empty in v2: `deletedAt` (1 of 17,887 filled in v1), `exportType` (1 of 17,887 filled in v1) |
| `visma_customer_payment_methods` | 31 / 9 | `visma_bronze.visma_customer_payment_methods` | identical | B |
| `visma_customer_payments` | 45,552 / 30,237 | `visma_bronze.visma_customer_payments` | identical | B |
| `visma_customer_sales_prices` | 17,976 / 17,222 | `visma_bronze.visma_customer_sales_prices` | identical | B |
| `visma_customers` | 776,045 / 27,287 | `visma_bronze.visma_customers` | identical + new columns | +4 new: `glAccounts`, `gln`, `invoiceOrdersSeparately`, `parentRecord`; B |
| `visma_deferral_codes` | 312 / 77 | `visma_bronze.visma_deferral_codes` | identical | B |
| `visma_departments` | 198 / 94 | `visma_bronze.visma_departments` | identical | B |
| `visma_dimensions` | 299 / 276 | `visma_bronze.visma_dimensions` | **columns missing** | Missing: `partitionDate`, `_validator`, `metadata`, `_validated_at` (v1 ingestion metadata of an older loader; no business column).; B |
| `visma_discount_codes` | 105 / 32 | `visma_bronze.visma_discount_codes` | identical | B |
| `visma_earning_types` | 204 / 49 | `visma_bronze.visma_earning_types` | **columns missing** | Missing: `partitionDate`, `_validator`, `metadata`, `_validated_at` (v1 ingestion metadata of an older loader; no business column).; B |
| `visma_employees` | 1,017 / 227 | `visma_bronze.visma_employees` | identical | B |
| `visma_expense_claims` | 22,300 / 4,832 | `visma_bronze.visma_expense_claims` | identical | B |
| `visma_financial_periods` | 17,976 / 696 | `visma_bronze.visma_financial_periods` | identical | B |
| `visma_fixed_asset_classes` | 146 / 146 | `visma_bronze.visma_fixed_asset_classes` | identical | B |
| `visma_fixed_asset_transactions` | 52,740 / 3,339 | `visma_bronze.visma_fixed_asset_transactions` | identical | B |
| `visma_fixed_assets` | 285 / 163 | `visma_bronze.visma_fixed_assets` | identical + new columns | +1 new: `accounts`; B |
| `visma_general_ledger_balances_v2` | 1,096,274 / 450,798 | `visma_bronze.visma_general_ledger_balances_v2` | identical | B |
| `visma_general_ledger_transactions` | 1,489,570 / 1,108,577 | `visma_bronze.visma_general_ledger_transactions` | identical | B |
| `visma_inventory_adjustments` | 10,830 / 6,474 | `visma_bronze.visma_inventory_adjustments` | identical | B |
| `visma_inventory_issues` | 31,180 / 24,664 | `visma_bronze.visma_inventory_issues` | identical | B |
| `visma_inventory_items` | 258,804 / 12,934 | `visma_bronze.visma_inventory_items` | identical + new columns | +5 new: `accountInformation`, `body`, `effectiveDate`, `note`, `priceWorkgroupID`; B |
| `visma_inventory_receipts` | 7,906 / 4,440 | `visma_bronze.visma_inventory_receipts` | identical | B |
| `visma_inventory_transfers` | 2,510 / 1,001 | `visma_bronze.visma_inventory_transfers` | identical | B |
| `visma_journal_transactions` | 19,230 / 146,124 | `visma_bronze.visma_journal_transactions` | identical | B; empty in v2: `businessUnitId` (18,619 of 19,230 filled in v1), `businessUnitName` (18,619 of 19,230 filled in v1) |
| `visma_kit_assemblies` | 1,913 / 986 | `visma_bronze.visma_kit_assemblies` | identical + new columns | +3 new: `kitAllocations`, `nonStockComponentLines`, `stockComponentLines`; B |
| `visma_kit_specifications` | 2,627 / 1,283 | `visma_bronze.visma_kit_specifications` | identical | B |
| `visma_landed_cost_codes` | 89 / 19 | `visma_bronze.visma_landed_cost_codes` | identical | B |
| `visma_ledgers` | 1,024 / 48 | `visma_bronze.visma_ledgers` | identical | B |
| `visma_locations` | 494 / 35,726 | `visma_bronze.visma_locations` | identical | B |
| `visma_lot_serial_classes` | 54 / 14 | `visma_bronze.visma_lot_serial_classes` | identical | B |
| `visma_number_sequences` | 3,706 / 908 | `visma_bronze.visma_number_sequences` | identical | B |
| `visma_organizations` | 311 / 12 | `visma_bronze.visma_organizations` | identical | B |
| `visma_packaging_types` | 13 / 3 | `visma_bronze.visma_packaging_types` | identical | B |
| `visma_payment_methods` | 302 / 73 | `visma_bronze.visma_payment_methods` | identical | B |
| `visma_price_classes` | 159 / 162 | `visma_bronze.visma_price_classes` | identical | Derived in Dataform from customers and customer locations (hand-written `visma_price_classes.sqlx`), not read from an endpoint.; B; empty in v2: `deletedAt` (1 of 159 filled in v1) |
| `visma_project_account_groups` | 21,543 / 832 | `visma_bronze.visma_project_account_groups` | identical | B |
| `visma_project_budgets` | 19,313 / 19,241 | `visma_bronze.visma_project_budgets` | identical | B |
| `visma_project_transactions` | 43,636 / 38,843 | `visma_bronze.visma_project_transactions` | identical + new columns | +1 new: `scanPeriod`; B |
| `visma_project_transactions__predup_bak` | 42,417 / - | - | **MISSING** | Manual backup before a dedupe (2026-06-02); not in Dataform. |
| `visma_projects` | 5,545 / 2,602 | `visma_bronze.visma_projects` | identical | B |
| `visma_purchase_orders` | 10,378 / 4,269 | `visma_bronze.visma_purchase_orders` | identical | B |
| `visma_purchase_receipts` | 8,418 / 5,266 | `visma_bronze.visma_purchase_receipts` | identical + new columns | +1 new: `note`; B |
| `visma_sales_order_types` | 509 / 167 | `visma_bronze.visma_sales_order_types` | identical | B |
| `visma_sales_orders_v2` | 7 / - | - | **MISSING** | Retired Sales Order API v2. Declared in the v1 Dataform, 7 rows, last written 2026-04-14. No v2 endpoint config and not built by the new Dataform (`dataform/README.md`: nothing reads it). People ran 15 query jobs on it in the last 45 days (last 2026-10-01). |
| `visma_sales_orders_v3` | 41,588 / 27,377 | `visma_bronze.visma_sales_orders_v3` | identical + new columns | +3 new: `commissions`, `note`, `payments`; B; empty in v2: `lines` (41,492 of 41,587 filled in v1) |
| `visma_sales_orders_v3_backup_20260416_dedupe` | 22,691 / - | - | **MISSING** | Manual backup (2026-04-16); not in Dataform. |
| `visma_sales_persons` | 141 / 133 | `visma_bronze.visma_sales_persons` | identical | B |
| `visma_shipments` | 41,267 / 25,000 | `visma_bronze.visma_shipments` | identical | B |
| `visma_stocktakes_v2` | 1,602 / 45 | `visma_bronze.visma_stocktakes_v2` | identical | B; empty in v2: `referenceNumber` (1,602 of 1,602 filled in v1) |
| `visma_subaccounts` | 13,653 / 13,611 | `visma_bronze.visma_subaccounts` | identical | B |
| `visma_supplier_documents` | 35,302 / 30,403 | `visma_bronze.visma_supplier_documents` | identical | B |
| `visma_supplier_invoices` | 572,785 / 30,403 | `visma_bronze.visma_supplier_invoices` | identical | B |
| `visma_supplier_locations` | 9,143 / 8,741 | `visma_bronze.visma_supplier_locations` | identical | B |
| `visma_supplier_payments` | 355,416 / 24,007 | `visma_bronze.visma_supplier_payments` | identical | B |
| `visma_suppliers` | 3,832 / 3,123 | `visma_bronze.visma_suppliers` | identical + new columns | +1 new: `note`; B |
| `visma_unit_of_measures` | 973 / 312 | `visma_bronze.visma_unit_of_measures` | **columns missing** | Missing: `metadata`, `partitionDate`, `_validator`, `_validated_at` (v1 ingestion metadata of an older loader; no business column).; B |
| `visma_vat_categories` | 576 / 504 | `visma_bronze.visma_vat_categories` | identical | B |
| `visma_vat_zones` | 373 / 120 | `visma_bronze.visma_vat_zones` | identical | B |
| `visma_vats` | 672 / 671 | `visma_bronze.visma_vats` | identical | B |
| `visma_warehouses` | 121 / 36 | `visma_bronze.visma_warehouses` | identical | B |

#### `dw_1_bronze_excel` to `visma_bronze_excel`: 2 objects, 2 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `raw_charts_of_accounts` | 113 / 113 | `visma_bronze_excel.raw_charts_of_accounts` | identical |  |
| `raw_entity_mapping` | 6 / 6 | `visma_bronze_excel.raw_entity_mapping` | identical |  |

#### `dw_1_silver_visma` to `visma_silver`: 72 objects, 53 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `dim_account` | 5,825 / 5,821 | `visma_silver.dim_account` | identical |  |
| `dim_branch` | 12 / 12 | `visma_silver.dim_branch` | identical |  |
| `dim_business_unit` | 20 / 20 | `visma_silver.dim_business_unit` | identical |  |
| `dim_carrier` | 109 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `dim_cash_account` | 156 / 156 | `visma_silver.dim_cash_account` | identical |  |
| `dim_company` | 12 / 12 | `visma_silver.dim_company` | identical |  |
| `dim_contact` | 16,905 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `dim_credit_terms` | 168 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `dim_currency` | 133 / 122 | `visma_silver.dim_currency` | identical |  |
| `dim_currency_market_rate` | 23,392 / 23,368 | `visma_silver.dim_currency_market_rate` | identical |  |
| `dim_currency_rate` | 34,097 / 33,871 | `visma_silver.dim_currency_rate` | identical |  |
| `dim_customer` | 25,879 / 27,287 | `visma_silver.dim_customer` | identical |  |
| `dim_department` | 106 / 106 | `visma_silver.dim_department` | identical |  |
| `dim_employee` | 227 / 227 | `visma_silver.dim_employee` | identical |  |
| `dim_financial_period` | 696 / 696 | `visma_silver.dim_financial_period` | identical |  |
| `dim_fixed_asset` | 164 / 163 | `visma_silver.dim_fixed_asset` | identical |  |
| `dim_inventory_item` | 10,159 / 12,934 | `visma_silver.dim_inventory_item` | identical | B |
| `dim_ledger` | 48 / 48 | `visma_silver.dim_ledger` | identical |  |
| `dim_location` | 149 / 149 | `visma_silver.dim_location` | identical |  |
| `dim_lot_serial_class` | 15 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `dim_organization` | 25 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. Last read 2026-04-16. |
| `dim_payment_method` | 80 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. Last read 2026-04-15. |
| `dim_project` | 2,620 / 2,602 | `visma_silver.dim_project` | identical |  |
| `dim_sales_order_type` | 13 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `dim_sales_person` | 133 / 133 | `visma_silver.dim_sales_person` | identical |  |
| `dim_subaccount` | 13,616 / 13,611 | `visma_silver.dim_subaccount` | identical |  |
| `dim_supplier` | 3,130 / 3,123 | `visma_silver.dim_supplier` | identical |  |
| `dim_unit_of_measure` | 13 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `dim_vat` | 12 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `dim_vat_category` | 13 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `dim_vat_zone` | 13 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `dim_warehouse` | 36 / 36 | `visma_silver.dim_warehouse` | identical |  |
| `dim_warehouse_location` | 2,020 / 2,013 | `visma_silver.dim_warehouse_location` | identical |  |
| `fact_cash_transaction` | 4,395 / 4,345 | `visma_silver.fact_cash_transaction` | identical |  |
| `fact_customer_credit_note` | 1,430 / 1,421 | `visma_silver.fact_customer_credit_note` | identical |  |
| `fact_customer_debit_note` | 54 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. Last read 2026-04-15. |
| `fact_customer_document` | 28,296 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. Last read 2026-04-15. |
| `fact_customer_invoice` | 31,330 / 31,520 | `visma_silver.fact_customer_invoice` | identical |  |
| `fact_customer_invoice_line` | 100,000 / 99,847 | `visma_silver.fact_customer_invoice_line` | identical |  |
| `fact_customer_payment` | 30,126 / 30,103 | `visma_silver.fact_customer_payment` | identical |  |
| `fact_expense_claim` | 4,050 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. Last read 2026-04-15. |
| `fact_fixed_asset_transaction` | 3,342 / 3,339 | `visma_silver.fact_fixed_asset_transaction` | identical |  |
| `fact_general_ledger` | 1,110,927 / 1,108,577 | `visma_silver.fact_general_ledger` | identical |  |
| `fact_general_ledger_balance` | 415,576 / 450,798 | `visma_silver.fact_general_ledger_balance` | identical |  |
| `fact_inventory_adjustment` | view | `visma_silver.fact_inventory_adjustment` | identical |  |
| `fact_inventory_adjustment_line` | view | `visma_silver.fact_inventory_adjustment_line` | identical |  |
| `fact_inventory_issue` | view | `visma_silver.fact_inventory_issue` | identical |  |
| `fact_inventory_issue_line` | view | `visma_silver.fact_inventory_issue_line` | identical |  |
| `fact_inventory_movement` | 36,747 / 36,579 | `visma_silver.fact_inventory_movement` | identical |  |
| `fact_inventory_movement_line` | 138,017 / 137,657 | `visma_silver.fact_inventory_movement_line` | identical |  |
| `fact_inventory_receipt` | view | `visma_silver.fact_inventory_receipt` | identical |  |
| `fact_inventory_receipt_line` | view | `visma_silver.fact_inventory_receipt_line` | identical |  |
| `fact_inventory_transfer` | view | `visma_silver.fact_inventory_transfer` | identical |  |
| `fact_inventory_transfer_line` | view | `visma_silver.fact_inventory_transfer_line` | identical |  |
| `fact_project_budget` | 19,313 / 19,171 | `visma_silver.fact_project_budget` | identical |  |
| `fact_project_transaction` | 38,354 / 38,199 | `visma_silver.fact_project_transaction` | identical |  |
| `fact_project_transaction_line` | 197,111 / 209,886 | `visma_silver.fact_project_transaction_line` | identical |  |
| `fact_purchase_order` | 4,324 / 4,269 | `visma_silver.fact_purchase_order` | identical |  |
| `fact_purchase_order_line` | 14,270 / 13,994 | `visma_silver.fact_purchase_order_line` | identical |  |
| `fact_purchase_receipt` | 94 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `fact_purchase_receipt_line` | 210 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. No read in 180 days. |
| `fact_sales_order` | 26,744 / 27,377 | `visma_silver.fact_sales_order` | identical | Grain: v2 keys sales orders by (tenant, order type, order id), v1 by (tenant, orderId), so v1 collapsed a quote and the order made from it (v2 27,377 headers and 90,161 lines against 26,410 and 85,980 before the 2026-10-10 fix; the fix is in the Dev workspace, not yet on Dataform `main`; v1 is unchanged; `docs/v2_roadmap.md` decision 8). |
| `fact_sales_order_business_unit` | 26,712 / 27,359 | `visma_silver.fact_sales_order_business_unit` | identical |  |
| `fact_sales_order_line` | 87,167 / 90,161 | `visma_silver.fact_sales_order_line` | identical | Same grain change as `fact_sales_order`. |
| `fact_sales_order_v2_header` | 22,158 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. Last read 2026-04-16. |
| `fact_sales_order_v2_line` | 71,675 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. Last read 2026-04-16. |
| `fact_shipment` | 25,456 / 25,000 | `visma_silver.fact_shipment` | identical |  |
| `fact_shipment_line` | 79,168 / 77,607 | `visma_silver.fact_shipment_line` | identical |  |
| `fact_supplier_document` | 13 / - | - | **MISSING** | Not in the v1 Dataform repo; written once on 2026-04-16 and never refreshed. Last read 2026-04-15. |
| `fact_supplier_invoice` | 21,422 / 30,400 | `visma_silver.fact_supplier_invoice` | identical |  |
| `fact_supplier_invoice_line` | 60,664 / 112,511 | `visma_silver.fact_supplier_invoice_line` | identical |  |
| `fact_supplier_payment` | 17,931 / 23,899 | `visma_silver.fact_supplier_payment` | identical |  |

#### `dw_1_silver_common` to `visma_silver_common`: 14 objects, 14 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `bridge_customer_price_class` | 1,650 / 1,775 | `visma_silver_common.bridge_customer_price_class` | identical |  |
| `bridge_customer_price_class_period` | 1,267 / 988 | `visma_silver_common.bridge_customer_price_class_period` | identical |  |
| `bridge_customer_to_combined` | 25,998 / 27,298 | `visma_silver_common.bridge_customer_to_combined` | identical |  |
| `customer_price_class_cross_tenant_candidates` | 0 / 0 | `visma_silver_common.customer_price_class_cross_tenant_candidates` | identical |  |
| `customer_price_class_gap_report` | 23,521 / 24,693 | `visma_silver_common.customer_price_class_gap_report` | identical | empty in v2: `inferred_price_class_evidence` (4 of 23,521 filled in v1), `customer_specific_price_evidence` (7 of 23,521 filled in v1) |
| `customer_price_class_gap_report_summary` | 24 / 22 | `visma_silver_common.customer_price_class_gap_report_summary` | identical |  |
| `qa_gl_project_gap_ap` | 210,512 / 157,488 | `visma_silver_common.qa_gl_project_gap_ap` | identical |  |
| `qa_gl_project_gap_ar` | 89,401 / 89,826 | `visma_silver_common.qa_gl_project_gap_ar` | identical |  |
| `qa_gl_project_gap_pm` | 4,752 / 4,408 | `visma_silver_common.qa_gl_project_gap_pm` | identical |  |
| `qa_gl_project_gap_summary` | 64 / 65 | `visma_silver_common.qa_gl_project_gap_summary` | identical |  |
| `source_gap_procurement_line_backlog` | 1,467 / 1,916 | `visma_silver_common.source_gap_procurement_line_backlog` | identical |  |
| `source_gap_procurement_line_summary` | 44 / 46 | `visma_silver_common.source_gap_procurement_line_summary` | identical |  |
| `source_gap_purchase_order_line` | 1,360 / 97 | `visma_silver_common.source_gap_purchase_order_line` | identical |  |
| `source_gap_supplier_invoice_line` | 711,802 / 35,452 | `visma_silver_common.source_gap_supplier_invoice_line` | identical |  |

#### `dw_1_silver_excel` to `visma_silver_excel`: 17 objects, 17 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `business_unit_mapping` | 20 / 20 | `visma_silver_excel.business_unit_mapping` | identical |  |
| `chart_of_accounts` | 131 / 131 | `visma_silver_excel.chart_of_accounts` | identical |  |
| `customer_price_class_business_review_pack_4_5` | 0 / 0 | `visma_silver_excel.customer_price_class_business_review_pack_4_5` | identical |  |
| `customer_price_class_mapping` | 2,134 / 3,837 | `visma_silver_excel.customer_price_class_mapping` | identical |  |
| `customer_price_class_review_shortlist` | 19 / 15 | `visma_silver_excel.customer_price_class_review_shortlist` | identical | empty in v2: `customer_class_sample_customers` (1 of 19 filled in v1) |
| `customer_price_class_unmapped_review` | 19 / 15 | `visma_silver_excel.customer_price_class_unmapped_review` | identical | empty in v2: `customer_class_sample_customers` (1 of 19 filled in v1) |
| `entity_mapping` | 9 / 9 | `visma_silver_excel.entity_mapping` | identical |  |
| `global_account_mapping` | 195 / 195 | `visma_silver_excel.global_account_mapping` | identical |  |
| `sales_order_line_business_unit_strategy` | 12 / 12 | `visma_silver_excel.sales_order_line_business_unit_strategy` | identical |  |
| `seed_chart_of_accounts` | 112 / 112 | `visma_silver_excel.seed_chart_of_accounts` | identical |  |
| `seed_company_account_mapping` | 5,806 / 5,806 | `visma_silver_excel.seed_company_account_mapping` | identical |  |
| `seed_customer_price_class_mapping` | 178 / 183 | `visma_silver_excel.seed_customer_price_class_mapping` | identical |  |
| `seed_entity_mapping` | 17 / 17 | `visma_silver_excel.seed_entity_mapping` | identical |  |
| `seed_global_account_mapping_scope` | 12 / 12 | `visma_silver_excel.seed_global_account_mapping_scope` | identical |  |
| `seed_global_account_range_mapping` | 195 / 195 | `visma_silver_excel.seed_global_account_range_mapping` | identical |  |
| `seed_rls_sales_person` | 0 / 0 | `visma_silver_excel.seed_rls_sales_person` | identical |  |
| `seed_sales_person_directory` | 108 / 108 | `visma_silver_excel.seed_sales_person_directory` | identical |  |

#### `dw_1_silver_quality` to `(none)`: 4 objects, 0 equivalents

The new project has no `silver_quality` dataset.

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `assert_bu_mapping_coverage_by_tenant_date` | view | - | **MISSING** | Assertion view (business-unit mapping or PO supplier coverage); not in the v1 repo; created 2026-04-16; no read in 180 days; no counterpart in `visma_assertions`. |
| `assert_bu_mapping_no_overlaps` | view | - | **MISSING** | Assertion view (business-unit mapping or PO supplier coverage); not in the v1 repo; created 2026-04-16; no read in 180 days; no counterpart in `visma_assertions`. |
| `assert_bu_name_null_rate_threshold` | view | - | **MISSING** | Assertion view (business-unit mapping or PO supplier coverage); not in the v1 repo; created 2026-04-16; no read in 180 days; no counterpart in `visma_assertions`. |
| `assert_po_supplier_coverage_by_tenant` | view | - | **MISSING** | Assertion view (business-unit mapping or PO supplier coverage); not in the v1 repo; created 2026-04-16; no read in 180 days; no counterpart in `visma_assertions`. |

#### `dw_1_gold_common` to `visma_gold_common`: 29 objects, 19 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `bridge_customer_price_class` | 1,068 / - | `visma_silver_common.bridge_customer_price_class` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-05-16. |
| `bridge_customer_price_class_period` | 1,002 / - | `visma_silver_common.bridge_customer_price_class_period` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-05-20. |
| `bridge_customer_to_combined` | 25,907 / - | `visma_silver_common.bridge_customer_to_combined` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-06-24. |
| `customer_price_class_cross_tenant_candidates` | 0 / - | `visma_silver_common.customer_price_class_cross_tenant_candidates` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-05-06. |
| `customer_price_class_gap_report` | 23,846 / - | `visma_silver_common.customer_price_class_gap_report` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-06-24. |
| `customer_price_class_gap_report_summary` | 24 / - | `visma_silver_common.customer_price_class_gap_report_summary` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-05-20. |
| `dim_business_unit` | 19 / 19 | `visma_gold_common.dim_business_unit` | identical |  |
| `dim_company` | 12 / 12 | `visma_gold_common.dim_company` | identical |  |
| `dim_currency` | 15 / 15 | `visma_gold_common.dim_currency` | identical |  |
| `dim_currency_market_rate` | 23,392 / 23,368 | `visma_gold_common.dim_currency_market_rate` | identical |  |
| `dim_currency_market_rate_to_nok_daily` | 21,915 / 21,915 | `visma_gold_common.dim_currency_market_rate_to_nok_daily` | identical |  |
| `dim_currency_rate` | 23,392 / 23,368 | `visma_gold_common.dim_currency_rate` | identical |  |
| `dim_customer_combined` | 5,117 / 5,077 | `visma_gold_common.dim_customer_combined` | identical |  |
| `dim_date` | 4,018 / 4,018 | `visma_gold_common.dim_date` | identical |  |
| `dim_department` | 106 / 106 | `visma_gold_common.dim_department` | identical |  |
| `dim_employee` | 227 / 227 | `visma_gold_common.dim_employee` | identical |  |
| `dim_entity` | 12 / 12 | `visma_gold_common.dim_entity` | identical |  |
| `dim_financial_period` | 696 / 696 | `visma_gold_common.dim_financial_period` | identical |  |
| `dim_inventory_item` | 10,397 / 17,347 | `visma_gold_common.dim_inventory_item` | identical | B |
| `dim_location` | 149 / 149 | `visma_gold_common.dim_location` | identical |  |
| `dim_missing_gl_accounts` | 3,517 / 3,513 | `visma_gold_common.dim_missing_gl_accounts` | identical |  |
| `dim_project` | 2,620 / 2,602 | `visma_gold_common.dim_project` | identical |  |
| `dim_sales_person` | 133 / 133 | `visma_gold_common.dim_sales_person` | identical |  |
| `dim_subaccount` | 13,616 / 13,611 | `visma_gold_common.dim_subaccount` | identical |  |
| `dim_warehouse` | 36 / 36 | `visma_gold_common.dim_warehouse` | identical |  |
| `source_gap_procurement_line_backlog` | 616 / - | `visma_silver_common.source_gap_procurement_line_backlog` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-05-16. |
| `source_gap_procurement_line_summary` | 42 / - | `visma_silver_common.source_gap_procurement_line_summary` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-05-04. |
| `source_gap_purchase_order_line` | 1,350 / - | `visma_silver_common.source_gap_purchase_order_line` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-06-24. |
| `source_gap_supplier_invoice_line` | 690,701 / - | `visma_silver_common.source_gap_supplier_invoice_line` | relocated | Stale copy of the same-named `silver_common` table (identical columns), written 2026-06-24, last read 2026-06-24. |

#### `dw_1_gold_finance` to `visma_gold_finance`: 26 objects, 26 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `dim_account` | 5,825 / 5,821 | `visma_gold_finance.dim_account` | identical |  |
| `dim_customer` | 25,998 / 27,298 | `visma_gold_finance.dim_customer` | identical | empty in v2: `inferred_price_class_evidence` (690 of 25,998 filled in v1), `customer_specific_price_evidence` (20 of 25,998 filled in v1) |
| `dim_fixed_asset` | 164 / 163 | `visma_gold_finance.dim_fixed_asset` | identical |  |
| `dim_inventory_item` | view | `visma_gold_finance.dim_inventory_item` | identical | B |
| `dim_supplier` | 3,628 / 3,655 | `visma_gold_finance.dim_supplier` | identical |  |
| `fact_customer_credit_note` | 1,430 / 1,421 | `visma_gold_finance.fact_customer_credit_note` | identical |  |
| `fact_customer_invoice` | 31,330 / 31,520 | `visma_gold_finance.fact_customer_invoice` | identical |  |
| `fact_customer_invoice_line` | 100,000 / 99,847 | `visma_gold_finance.fact_customer_invoice_line` | identical |  |
| `fact_customer_payment` | 30,126 / 30,103 | `visma_gold_finance.fact_customer_payment` | identical |  |
| `fact_fixed_asset_transaction` | 3,342 / 3,339 | `visma_gold_finance.fact_fixed_asset_transaction` | identical |  |
| `fact_general_ledger` | 1,110,927 / 1,108,577 | `visma_gold_finance.fact_general_ledger` | identical |  |
| `fact_general_ledger_balance` | 415,576 / 450,798 | `visma_gold_finance.fact_general_ledger_balance` | identical |  |
| `fact_inventory_adjustment` | view | `visma_gold_finance.fact_inventory_adjustment` | identical |  |
| `fact_inventory_adjustment_line` | view | `visma_gold_finance.fact_inventory_adjustment_line` | identical |  |
| `fact_inventory_issue` | view | `visma_gold_finance.fact_inventory_issue` | identical |  |
| `fact_inventory_issue_line` | view | `visma_gold_finance.fact_inventory_issue_line` | identical | empty in v2: `source_warehouse_key` (160 of 68,195 filled in v1) |
| `fact_inventory_movement` | 36,747 / 36,579 | `visma_gold_finance.fact_inventory_movement` | identical |  |
| `fact_inventory_movement_line` | 138,017 / 137,657 | `visma_gold_finance.fact_inventory_movement_line` | identical |  |
| `fact_inventory_receipt` | view | `visma_gold_finance.fact_inventory_receipt` | identical |  |
| `fact_inventory_receipt_line` | view | `visma_gold_finance.fact_inventory_receipt_line` | identical | empty in v2: `source_warehouse_key` (27 of 23,916 filled in v1) |
| `fact_inventory_transfer` | view | `visma_gold_finance.fact_inventory_transfer` | identical |  |
| `fact_inventory_transfer_line` | view | `visma_gold_finance.fact_inventory_transfer_line` | identical |  |
| `fact_project_portfolio_line` | 197,111 / 209,886 | `visma_gold_finance.fact_project_portfolio_line` | identical |  |
| `fact_supplier_invoice` | 21,422 / 30,400 | `visma_gold_finance.fact_supplier_invoice` | identical |  |
| `fact_supplier_invoice_line` | 60,664 / 112,511 | `visma_gold_finance.fact_supplier_invoice_line` | identical |  |
| `fact_supplier_payment` | 17,931 / 23,899 | `visma_gold_finance.fact_supplier_payment` | identical |  |

#### `dw_1_gold_sales` to `visma_gold_sales`: 6 objects, 6 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `dim_purchase_order` | 4,324 / 4,269 | `visma_gold_sales.dim_purchase_order` | identical |  |
| `dim_sales_order` | 26,744 / 27,377 | `visma_gold_sales.dim_sales_order` | identical | Same grain change as silver `fact_sales_order`. |
| `dim_shipment` | 25,456 / 25,000 | `visma_gold_sales.dim_shipment` | identical |  |
| `fact_purchase_order_line` | 14,270 / 13,994 | `visma_gold_sales.fact_purchase_order_line` | identical |  |
| `fact_sales_order_line` | 87,167 / 90,161 | `visma_gold_sales.fact_sales_order_line` | identical | Same grain change as silver `fact_sales_order`. |
| `fact_shipment_detail_line` | 79,168 / 77,607 | `visma_gold_sales.fact_shipment_detail_line` | identical |  |

#### `dw_1_gold_bi` to `visma_datamart`: 6 objects, 6 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `BI_FinanceTable` | 1,110,927 / 1,108,577 | `visma_datamart.BI_FinanceTable` | identical |  |
| `BI_ProjectBudgetRecon` | 2,254 / 2,234 | `visma_datamart.BI_ProjectBudgetRecon` | identical |  |
| `BI_ProjectPortfolioPnL` | 2,037 / 2,184 | `visma_datamart.BI_ProjectPortfolioPnL` | identical |  |
| `BI_SalesOrder` | 87,167 / 90,161 | `visma_datamart.BI_SalesOrder` | identical | Carries the same grain change (orders and quotes restored for Aquatiq AS). |
| `RLS_sales_person` | 96 / 96 | `visma_datamart.RLS_sales_person` | identical |  |
| `assert_project_budget_sync_coverage` | view | `visma_datamart.assert_project_budget_sync_coverage` | identical |  |

#### `dw_1_staging_currency_n2` to `dw_staging_currency`: 4 objects, 4 equivalents

| v1 object | rows v1 / v2 | v2 equivalent | status | notes |
|---|---:|---|---|---|
| `Historyrates` | 11,696 / 11,684 | `dw_staging_currency.Historyrates` | identical |  |
| `HistoryratesPowerBI` | view | `dw_staging_currency.HistoryratesPowerBI` | identical |  |
| `Rate` | 14 / 14 | `dw_staging_currency.Rate` | identical |  |
| `RatePowerBI` | view | `dw_staging_currency.RatePowerBI` | identical |  |


Ten names are declared in the v1 Dataform bronze but never had a table in `dw_1_bronze_visma`: `visma_budgets`, `visma_cash_sales`, `visma_customer_contracts`, `visma_customer_overdue_charges`, `visma_fixed_asset_property_tax_groups`, `visma_project_tasks`, `visma_sales_categories`, `visma_supplier_classes`, `visma_time_cards`, `visma_vouchers`. They are not counted above. The new staging holds rows for `project_tasks` (12,159) and `supplier_classes` (106).

### 1.4 Columns present in v2 but empty where v1 has values

**242 columns in 80 of the 220 equivalent objects are filled in v1 and completely empty in v2** (no non-NULL value, and for text columns no value other than blank, `null`, `[]`, `{}`; whole-table counts, see section 4).

**B. 222 of them are v1's sync-bookkeeping columns** `seenInFullScopeRuns`, `consecutiveMissingFullRuns`, `syncBatch` (snake_case in silver and gold). They sit in 74 tables (`dw_1_bronze_visma` 71, `dw_1_silver_visma` 1, `dw_1_gold_common` 1, `dw_1_gold_finance` 1). v2 stages `deleted_at` and soft-deletes after a full scan instead; the new bronze SQL writes these columns as `CAST(NULL ...)`. No silver or gold logic is known to need them (not checked column by column).

**The other 20 columns, in 14 tables:**

| table | column | filled in v1 | rows v2 | what the code shows |
|---|---|---:|---:|---|
| `dw_1_bronze_visma.visma_customer_credit_notes` | `dunningLetterDate` | 3 of 9,162 | 1,421 | bronze SQL: `CAST(NULL ...)  -- v2 does not fetch this field` |
| `dw_1_bronze_visma.visma_customer_credit_notes` | `dunningLetterLevel` | 3 of 9,162 | 1,421 | same marker |
| `dw_1_bronze_visma.visma_customer_debit_notes` | `dunningLetterDate` | 18 of 359 | 45 | same marker |
| `dw_1_bronze_visma.visma_customer_debit_notes` | `dunningLetterLevel` | 18 of 359 | 45 | same marker |
| `dw_1_bronze_visma.visma_customer_invoices` | `errorInfo` | 1 of 48,953 | 31,520 | same marker |
| `dw_1_bronze_visma.visma_customer_locations` | `deletedAt` | 1 of 17,887 | 32,309 | bronze maps staging `deleted_at`, which is NULL on every staged row |
| `dw_1_bronze_visma.visma_customer_locations` | `exportType` | 1 of 17,887 | 32,309 | same marker |
| `dw_1_bronze_visma.visma_journal_transactions` | `businessUnitId` | 18,619 of 19,230 | 146,124 | same marker. Business unit is not read from journal bronze by any silver model (grep) |
| `dw_1_bronze_visma.visma_journal_transactions` | `businessUnitName` | 18,619 of 19,230 | 146,124 | same marker |
| `dw_1_bronze_visma.visma_price_classes` | `deletedAt` | 1 of 159 | 162 | `visma_price_classes.sqlx` (hand-written) sets `deletedAt` to `CAST(NULL AS TIMESTAMP)` |
| `dw_1_bronze_visma.visma_sales_orders_v3` | `lines` | 41,492 of 41,587 | 27,377 | same marker; `orderLines` is filled (v1 41,492, v2 27,339) and `fact_sales_order_line` reads `orderLines` first |
| `dw_1_bronze_visma.visma_stocktakes_v2` | `referenceNumber` | 1,602 of 1,602 | 45 | same marker |
| `dw_1_gold_finance.dim_customer` | `customer_specific_price_evidence` | 20 of 25,998 | 27,298 | derived in Dataform; cause not established (a value question, out of scope here) |
| `dw_1_gold_finance.dim_customer` | `inferred_price_class_evidence` | 690 of 25,998 | 27,298 | derived in Dataform; cause not established (a value question, out of scope here) |
| `dw_1_gold_finance.fact_inventory_issue_line` | `source_warehouse_key` | 160 of 68,195 | 68,035 | derived in Dataform; cause not established (a value question, out of scope here) |
| `dw_1_gold_finance.fact_inventory_receipt_line` | `source_warehouse_key` | 27 of 23,916 | 23,864 | derived in Dataform; cause not established (a value question, out of scope here) |
| `dw_1_silver_common.customer_price_class_gap_report` | `customer_specific_price_evidence` | 7 of 23,521 | 24,693 | derived in Dataform; cause not established (a value question, out of scope here) |
| `dw_1_silver_common.customer_price_class_gap_report` | `inferred_price_class_evidence` | 4 of 23,521 | 24,693 | derived in Dataform; cause not established (a value question, out of scope here) |
| `dw_1_silver_excel.customer_price_class_review_shortlist` | `customer_class_sample_customers` | 1 of 19 | 15 | derived in Dataform; cause not established (a value question, out of scope here) |
| `dw_1_silver_excel.customer_price_class_unmapped_review` | `customer_class_sample_customers` | 1 of 19 | 15 | derived in Dataform; cause not established (a value question, out of scope here) |

**Partly empty.** `dataform/tools/compare_columns.py` (active tenants only, rows before 2026-10-01 where every row has a date; run with `--drop 0 --out <scratch>`; 559 columns drop by any amount, 107 by more than 20 points) puts all of the big drops in one dimension: `visma_gold_common.dim_inventory_item` (42 columns), `visma_gold_finance.dim_inventory_item` (42 columns), `visma_silver.dim_inventory_item` (23 columns). The availability columns of `dim_inventory_item` (`availability_*`, `primary_availability_*`, `inventory_internal_id`) are filled for about 58 % of items in v2 against 98 % in v1 (silver: 77 % against 100 %). The same tool finds 17 columns at 0 % in silver, gold and the datamart, all of them in my list.

### 1.5 Objects that exist only in the new warehouse

* `visma_bronze.visma_tenants`: the tenant list with DataMart company ids (hard-coded rows in `visma_tenants.sqlx`).
* `visma_assertions`: 8 views with no v1 counterpart: `assert_gl_batch_nets_to_zero`, `assert_gl_company_period_nets_to_zero`, `assert_journal_batch_control_totals`, `assert_journal_batch_header_unique`, `assert_project_transaction_lines_complete`, `assert_sales_order_rows_complete`, `visma_gold_sales_dim_sales_order_assertions_uniqueKey_0`, `visma_silver_fact_sales_order_assertions_uniqueKey_0`.
* Bronze tables with extra columns: 11 (the fields v2 fetches that v1 never wrote).
* `dw_staging_visma`: 135 raw JSON tables, one per endpoint. v1 has no counterpart in the same region (its staging datasets are in europe-west1). 63 of them have no bronze table: 38 DataMart `dm_*` tables and 25 ERP or Expense endpoints that v1 never had in bronze (`dataform/README.md`, section "Not done").

### 1.6 Notes on the data tables

* **Missing, by group.** 26 objects have no equivalent: 2 backups in bronze (`visma_project_transactions__predup_bak`, `visma_sales_orders_v3_backup_20260416_dedupe`), the retired `visma_sales_orders_v2`, 19 silver tables written once on 2026-04-16 (`dim_carrier`, `dim_contact`, `dim_credit_terms`, `dim_lot_serial_class`, `dim_organization`, `dim_payment_method`, `dim_sales_order_type`, `dim_unit_of_measure`, `dim_vat`, `dim_vat_category`, `dim_vat_zone`, `fact_customer_debit_note`, `fact_customer_document`, `fact_expense_claim`, `fact_purchase_receipt`, `fact_purchase_receipt_line`, `fact_sales_order_v2_header`, `fact_sales_order_v2_line`, `fact_supplier_document`), and the 4 `silver_quality` views. The 10 relocated `gold_common` copies are stale duplicates of `silver_common` tables (written 2026-06-24, identical columns).
* **Not counted: MERGE chunk tables.** v1's sync leaves `<table>_stg_<id>_chunk1_<id>` tables in `dw_1_bronze_visma`: 3 at the snapshot (`visma_cash_transactions_stg_<id>_chunk` created 2026-06-05, `visma_inventory_receipts_stg_<id>_chunk` created 2026-04-19, `visma_supplier_payments_stg_<id>_chunk` created 2026-06-01). Some are orphans from failed MERGEs; they are not in Dataform and have no equivalent in v2. A read 20 minutes earlier also showed `visma_purchase_receipts_stg_<id>_chunk`, which has since disappeared: these tables come and go with v1's sync, so the count moves.
* **Missing columns.** Only 5 bronze reference tables (`visma_countries`, `visma_currencies`, `visma_dimensions`, `visma_earning_types`, `visma_unit_of_measures`) lack columns, all four of them loader metadata (`partitionDate`, `metadata` struct, `_validator`, `_validated_at`; `countries` has two). No table is retyped, and there is no rename that name matching could find.
* **Empty tables.** No v2 table is empty where the v1 table has rows. Three tables are empty on both sides (`customer_price_class_cross_tenant_candidates`, `customer_price_class_business_review_pack_4_5`, `seed_rls_sales_person`).
* **Reads.** 201 of the 256 v1 objects were read by a person (a user login; this includes Power BI under a personal credential) in the last 45 days. Of the 36 missing or relocated objects, 1 was read: `visma_sales_orders_v2` (15 query jobs, last 2026-10-01).

## 2. Service and operations coverage

Status: COVERED = same function in v2; REPLACED = done by a different mechanism, by design; PARTIAL = part is a gap; GAP = no v2 equivalent; N/A = v1-only, not needed. Sources for v1: `app.py`, `scripts/`, `docs/`, `HIDDEN_README_OPERATIONAL.md` (not quoted: it holds credentials), the repo-root `scripts/` and `.github/workflows`, and notes from earlier sessions. Nothing on the VPS was touched.

Two sets of tier scripts exist. `apps/visma_service/scripts/` has `sync-tier1-hourly.sh`, `sync-tier2-daily.sh`, `sync-tier3-weekly.sh`, `sync-full-system.sh` and a `crontab.example`; its `README.md` also lists `daily-incremental-sync.sh`, `sync_gl_monthly.sh` and `visma_sync_common.sh`, which are not in that folder. The maintained set (July 2026) is in the repo-root `scripts/`: `tier1-hourly-sync.sh`, `tier2-daily-sync.sh`, `tier3-weekly-sync.sh`, `daily-incremental-sync.sh`, `catchup-guardian.sh`, `sync_gl_monthly.sh`, `visma_sync_common.sh` and `README-SYNC-STRATEGY.md`. The rows below describe the root set; `README-SYNC-STRATEGY.md` still says Tier 3 is paused, while `docs/TIER3_PAUSE_HANDOFF.md` says it was re-enabled on 2026-03-20.

### 2.1 v1 HTTP service (`app.py`; Flask, not FastAPI; 61 route decorators = 62 URL rules; port 5001 locally, 8004 on the VPS) (rows A1 to A10)

| # | v1 item | v2 equivalent | status | note or recommendation |
|---|---|---|---|---|
| A1 | `GET /health`: static JSON `healthy`, no dependency check. | `vsv2 healthcheck` (Postgres, Redis, lease path; compose healthcheck of `vsv2`) and `GET /v1/health` in `vsv2-http` (Postgres `SELECT 1`, no auth). | **COVERED** | Stronger than v1. Neither checks that the scheduler loop makes progress; the `vsv2-http` compose service has no healthcheck. |
| A2 | OAuth onboarding and token routes (9 rules: `/auth-url`, `/callback`, `/api/visma/callback`, `/authenticate`, `/status`, `/disconnect`, `/refresh-org-names`, `/organizations`, `/token/status`). Authorization-code flow plus client_credentials; tokens kept in Postgres `visma_tokens` with a Redis cache (`auth/redis_token_manager.py`). | client_credentials only, per tenant and scope, tokens cached in memory (`core/crates/vsv2-auth/src/oauth.rs`); `vsv2 token --scope --tenant`. | **REPLACED** | No interactive consent flow and no persisted tokens. v1's own tenant script (`scripts/add_tenants.py`) also authenticated with client_credentials. Prerequisite outside the code: the Visma app must be granted per tenant and per scope (ERP, DataMart, Expense, SalesOrder). Not verified: a token for every tenant and scope. |
| A3 | Tenant management: Postgres `visma_organizations`, `scripts/add_tenants.py\|.sh`, `HISTORICAL_TENANTS`, `/organizations`. | `config/tenants/*.json` (12 files, `status` active or historical), `vsv2 tenants-register [--dry-run]`, table `vsv2.tenant_company_map`; MCP tool `visma_list_tenants`. | **COVERED** | Onboarding is several manual steps, see the operations table below. |
| A4 | Sync triggers (6 rules): `/sync/start`, `/sync/chunked/start`, `/sync/multi-org/start`, `/sync/gcp`, `/bigquery/sync/start` (tier, endpoints, `date_from`, `full_refresh`), `/bigquery/sync/all`. | `vsv2 sync --tenant [--endpoint] [--date-from --date-to \| --full]`; `POST /v1/sync/backfill` and `POST /v1/schedules/{id}/trigger` in `vsv2-http` (bearer token, one at a time). | **COVERED** |  |
| A5 | Job tracking and error audit (16 rules): `/sync/jobs[/<id>]`, `/sync/dashboard`, `/sync/errors` (declared twice) and `/sync/errors/<id>/resolve`, `/sync/multi-org/status/<id>`, `/bigquery/sync/status[/<id>]`, `/bigquery/sync/jobs\|runs\|runs/<id>\|errors`, `/sync/tracking/comprehensive`, `/sync/recent-records`, `/bigquery/cron/status`. State in Postgres `visma_sync_runs`, `visma_sync_table_results`, `visma_sync_errors`. | Postgres `vsv2.sync_run`, `sync_checkpoint`, `sync_page_metric`, `scheduler_pass`, `sync_operation`; `GET /v1/runs\|/v1/checkpoints\|/v1/backfills/{id}`; MCP tool `visma_sync_status`. | **COVERED** | Read side only. Not carried over: the error-resolve workflow and the dashboard roll-up (nothing in the repo shows they were used). |
| A6 | Live read routes (11 rules): `/data`, `/count`, `/counts/all`, `/bigquery/data`, `/gcp/query`, `/gcp/status`, `/bigquery/tables/row-counts`, `/bigquery/sync/tables\|endpoints`, `/bigquery/sync/tier1/capabilities[/report]`. | MCP read tools: `visma_erp_query`, `visma_datamart_query`, `visma_datamart_schema`, `visma_salesorder_get`, `visma_list_endpoints`, `warehouse_query\|list_tables\|describe_table\|overview`; `scripts/endpoint_matrix.py`. | **REPLACED** | Read-only and guarded. The default dataset list of the MCP warehouse tools now names the `visma_*` datasets (changed while this was written, uncommitted); `scripts/run_mcp.sh` still defaults the project to `prj-dw-dev`. |
| A7 | Redis response cache: `/cache/clear`, `/cache/stats`. | None. v2 uses Redis only for leases and the rate limiter. | **N/A** | No API-response cache is needed for a scheduled sync. |
| A8 | `GET /incremental-check/<endpoint>/<tenant>`: compare Visma with BigQuery for one endpoint and tenant. | Gate-e `scripts/verify_endpoint_counts.py` (new, uncommitted: Visma's total against staging's live rows for the 63 countable ERP REST endpoints and 12 tenants; its report of 2026-10-12 shows PASS 752, WARN 1, FAIL 3 of 756 cells, 33 endpoints not countable), gate-b `verify_population_vs_visma.py` (open invoices against Visma's own list), gate-s `verify_sales_order_counts.py` (sales orders), `verify_bq_vs_visma.py` (key spot check); `scripts/run_gates.py` runs five gates. | **COVERED** | Count level for most endpoints, key level for invoices. Not run by me; the 3 FAIL cells of the gate-e report are not analysed here. |
| A9 | `POST /bigquery/cleanup/staging-tables`: delete orphaned `_stg_*_chunk*` MERGE tables. | None. `dw_staging_visma` holds 135 tables and no temporary tables. | **N/A** | v1 leaves transient MERGE tables behind (3 in `dw_1_bronze_visma` at the snapshot). Not verified how `vsv2-bq-stage` cleans up after a failed load. |
| A10 | ODS builder (14 `/ods/*` rules, `sync/ods_manager.py`, `ods_scheduler.py`, `ods_quality.py`, Postgres `ods_*`): user-defined BigQuery tables. | None. Dataform layers replace it. | **N/A** | Docs are archived (`docs/archive/ods`), the default target `dw_2_ods_visma_v2` does not exist, and `dw_2_ods_visma`, `dw_8_ods`, `dw_9_dm` had no reads in 180 days. Route usage not verified (no access logs). |

### 2.2 v1 processes and state (VPS, from `docs/AGENT_BOOTSTRAP_VISMA_INFRA.md` and the deploy notes) (rows B1 to B5)

| # | v1 item | v2 equivalent | status | note or recommendation |
|---|---|---|---|---|
| B1 | `visma-sync-worker`: Redis queue consumer for `/bigquery/sync/start` jobs (`scripts/run_bigquery_sync_worker.py`). | No queue. The scheduler and the CLI run syncs in-process. | **REPLACED** |  |
| B2 | `visma-sync-sweeper` (`scripts/run_bigquery_sync_sweeper.py`): every 30 s fails or requeues stale queued and running jobs. | `abandon_stale_runs` (runs `running` for more than 6 h, called when `schedule` or `vsv2-http` starts); `scheduler_pass.heartbeat_at` every 60 s, a pass silent for 10 min is abandoned and re-run; `fail_stale_sync_operations` at `vsv2-http` start. | **REPLACED** |  |
| B3 | Shared state: Postgres `aquatiq_visma` (runs, errors, tokens, capabilities, `ods_*`), Redis (queue, leases, limiter). | Own Postgres database `visma_v2` (migrations 0001 to 0012) and Redis (leases, limiter), from `docker-compose.yml`. | **COVERED** | `docker-compose.yml` is the development stack (dev Postgres password, host ports on 127.0.0.1). `docker-compose.prod.yml` (new, untracked, written while this was read) overrides it: `VSV2_POSTGRES_PASSWORD` required, no host ports for Postgres and Redis, the HTTP API only with `--profile http` and a bearer token, log rotation, 90 s stop grace. Where it runs is not decided. |
| B4 | Deploy: GitHub Actions `.github/workflows/deploy.yml` (rsync to `/opt/aquatiq-integrations`, rebuild) and `build-test.yml` (Docker build of `visma_service`). | None. Neither workflow mentions `visma_service_v2`; deploy is `docker compose build vsv2` by hand (`docs/DEPLOYMENT.md` section 4a). | **GAP** | Needs a decision on the host first (roadmap decision 1: VPS beside v1 or a GCP VM). Recommendation: needs a decision on the host, then build CI and the deploy step. |
| B5 | Rate limiting and retry: Redis-backed limiter (`utils/retry.py`; `VISMA_RATE_LIMIT_MINUTE` 60, `VISMA_RATE_LIMIT_DAY` 5000), retries with back-off (`VISMA_MAX_RETRIES`). | `vsv2-ratelimit` (token bucket plus a shared Redis cooldown set on HTTP 429 or Retry-After), `VSV2_RATELIMIT_*`; the scheduler uses its own budget (240 requests/min ERP, 120 DataMart); `vsv2-erp-rest` retries gateway errors (502, 503, 504), re-authenticates on 401 and sets the cooldown on 429. | **COVERED** | The limiter is one bucket per process. An earlier session note says Visma's quota is shared with live v1, so v1 and a live v2 scheduler must be run with bounded budgets (not re-verified). |

### 2.3 v1 schedules and scheduler policy (rows C1 to C11)

| # | v1 item | v2 equivalent | status | note or recommendation |
|---|---|---|---|---|
| C1 | Hourly tier 1: `_TIER_1_ENDPOINTS` in `app.py` (19), triggered by `scripts/tier1-hourly-sync.sh` at the repo root. | Hourly tier, `VSV2_SCHEDULE_TIER1_SECS` = 3600, the 15 endpoints with `tier: 1`. | **PARTIAL** | 6 of v1's 19 hourly endpoints are not hourly in v2: `general_ledger_transactions`, `expense_claims`, `fixed_asset_transactions`, `customer_credit_notes`, `stocktakes_v2` run daily (no `tier` set) and `time_cards` weekly (`tier: 4`). v2 adds `customer_documents` and `kit_assemblies` to the hourly tier. Recommendation: needs a decision (gap 5). |
| C2 | Daily tier 2 (32 endpoints), 02:00. | Daily tier: every endpoint with tier 2, 3 or none, `VSV2_SCHEDULE_TIER2_SECS` = 86400. | **COVERED** |  |
| C3 | Weekly tier 3 (30 endpoints), Sunday 04:00. | Tier 3 sits in the daily tier; the weekly Full pass (`VSV2_SCHEDULE_TIER4_SECS` = 604800) reads every enabled endpoint over its whole scope and reconciles deletes. | **COVERED** |  |
| C4 | Nightly replay chain `scripts/daily-incremental-sync.sh` (00:30): tier 1 then tier 2 with `date_from` = now minus 7 days. The documented safety net for changes the hourly sync missed. | No replay window. Incremental reads start at the checkpoint minus an overlap: 24 h for endpoints whose name contains `general_ledger`, `journal` or `transaction`, 10 min for the rest, 70 min across the October fall-back hour (`executor.rs overlap_watermark`); the weekly Full pass catches the rest. | **PARTIAL** | A change that does not move `lastModifiedDateTime` (a payment on an open invoice) stays stale until the weekly pass. Being closed for invoices only: `docs/DEPLOYMENT.md` section 4b (written while this was read) describes an open-set reconciliation of `customer_invoices` and `supplier_invoices`, once a day per tenant after the hourly sync (`vsv2-orchestrator/src/open_set*`, `open_set: true` in two endpoint configs, uncommitted; not reviewed or run by me). Purchase and sales orders are not covered. Recommendation: needs a decision (gap 6). |
| C5 | `scripts/catchup-guardian.sh` (every 30 min): compare the last success per tier with the schedule and trigger a catch-up. | Built into the scheduler: what is due is read from `vsv2.scheduler_pass`, so a restart or sleep does not shift it; failed endpoints are retried after 15 min (hourly), 2 h (daily), 12 h (full); a stale pass is re-run. | **REPLACED** |  |
| C6 | Manual GL full refresh `scripts/sync_gl_monthly.sh` (default start 2023-01-01). | `vsv2 sync --tenant <id> --endpoint general_ledger_transactions --full`, or `--date-from/--date-to`; period-scan endpoints walk financial periods from `first_period` (2020-01 for GL and journals, 2023-01 for project transactions). | **COVERED** |  |
| C7 | Historical tenants (4, frozen since 2025-12-31) removed from the cron arrays and listed in `HISTORICAL_TENANTS`. | `status: historical` in `config/tenants/*.json`; the scheduler runs only the weekly Full pass for them (`scheduler.rs Tier::covers`). | **COVERED** |  |
| C8 | Monthly full-system sync (`scripts/crontab.example`, `sync-full-system.sh`). | The weekly Full pass. | **COVERED** |  |
| C9 | Scheduler policy (`docs/SCHEDULER_POLICY_AND_ENDPOINT_AUDIT.md`): one global lease (one sync at a time); proposed per-tenant lease and a per-tenant capability registry for endpoints that return HTTP 400 because a module is off (`visma_tier1_capabilities`, `sync/endpoint_capability_policy.py`). | Per-tenant Redis lease (5 min TTL, renewed each minute) with `VSV2_SCHEDULE_PARALLEL_TENANTS` = 4. The capability registry is only a table: `vsv2.endpoint_capability` (migration 0004) is not read or written by any Rust or Python code. | **PARTIAL** | A tenant without a module will fail that endpoint on every pass (retry 15 min, 2 h, 12 h). Not verified how the executor treats a page-1 HTTP 400. Recommendation: needs a decision after the first live pass (gap 8). |
| C10 | Dataform refresh every 4 h: Cloud Function `dataform-refresher` (`apps/dataform_refresher`, compiles `main`, invokes a workflow) plus Cloud Scheduler job `dataform-refresher-every-4h` (`5 */4 * * *`, Europe/Oslo) in `prj-dw-dev`. | None. The release config `production` has no schedule and no workflow config exists (`dataform/README.md`: "Nothing is scheduled"). | **GAP** | Build. In `prj-dw-dev` a release-config cron was refused (`strictActAsChecks`), which is why v1 uses a function; not checked for the new project. Recommendation: build (gap 1). |
| C11 | Delete detection: counters `seenInFullScopeRuns` and `consecutiveMissingFullRuns` with thresholds `FULL_SCOPE_DELETE_MIN_SEEN_RUNS` = 3 and `FULL_SCOPE_DELETE_MIN_MISSING_RUNS` = 2; soft delete through `isDeleted`. | One complete scan (`vsv2 sync --full` or the weekly Full pass) sets `deleted_at` on what Visma no longer returns, never more than max(50, 20 %) of a tenant's rows at once (`delete_skipped` is logged; `--force-delete` on `vsv2-bq-stage` overrides); a returning record clears it. `vsv2 reconcile` is the older ID-scan command. | **REPLACED** | v2 deletes after one complete scan; v1 waited for repeated misses. The 20 % guard is the only brake. |

### 2.4 v1 monitoring, validation and repair jobs (rows D1 to D8)

| # | v1 item | v2 equivalent | status | note or recommendation |
|---|---|---|---|---|
| D1 | `scripts/monitor_pm_pipeline.py` (VPS cron every 4 h): duplicate keys in bronze `project_transactions` and `journal_transactions`, and silver GL project coverage of at least 60 %. | Dataform assertions `assert_journal_batch_header_unique`, `assert_project_transaction_lines_complete`, `assert_gl_batch_nets_to_zero`, `assert_gl_company_period_nets_to_zero`, `assert_journal_batch_control_totals` in `visma_assertions`, run by hand through `scripts/run_gates.py` (gate-c). | **PARTIAL** | The coverage floor is not ported, and nothing runs the assertions on a schedule. Recommendation: build (gap 9). |
| D2 | `scripts/e2e_*.sh`: service health, data recency per tier in BigQuery, schema health. | `_sync_heartbeat` plus the staleness rule of gate-b (26 h), `scripts/check_bq_staging_live.py`, `scripts/verify_bq_staging.py`. Manual. | **PARTIAL** | The heartbeat table does not exist yet in `dw_staging_visma`. Recommendation: build, together with the monitoring check in gap 4. |
| D3 | Per-endpoint validators: `validate_recent_*.py` (59), `run-tier3-validation-*.sh` (58), `validate_tier2_*`, `run_tier2_validation.py`. | `scripts/validation_coverage.py` (null and absent share per field against live data), `scripts/ingest_probe.py`, the gates, `make validate-config`. | **REPLACED** | Population-level checks instead of a recent-records sample per endpoint. |
| D4 | Nightly reconciliation report: `scripts/build_sync_reconciliation_report.py` from the job-status routes into `${HOME}/logs/aquatiq/reconciliation`. | `docs/gates/RESULTS.md`, written by `scripts/run_gates.py` on demand. | **PARTIAL** | Not nightly, not scheduled. Recommendation: build; a scheduled run of `run_gates.py` covers it (gap 4). |
| D5 | Config drift and capability probes: `audit_tier1_tier2_config_drift.py`, `probe_tier3_endpoint_support.py`, `probe_endpoint_request_shapes.py`, `discover_endpoints.py`. | `make validate-config` (`vsv2-validate-config`: OK for 135 endpoint configs in this session), `scripts/discover_erp_endpoints.py`, `expand_coverage.py`, `endpoint_matrix.py`. | **COVERED** |  |
| D6 | Repair and backfill one-offs: `backfill_*.py`, `scripts/incident_repair/*`, `bigquery_dedupe_*.py`, `repair_customer_locations_remaining.py`, `drop_bu_columns_from_bigquery.py`, `migrate_bigquery_legacy_schemas.py`, `bq_partition_cluster.sh`. | None needed: they repair v1 defects (business-unit columns, duplicate MERGE rows, legacy schemas). v2 stages raw rows with soft delete and rebuilds bronze on each Dataform run. | **N/A** | Not verified on live data: no live v2 sync has run yet. |
| D7 | Alerting: not implemented in v1 either. The docs list recommended alerts; `scripts/run-tier3-validation.sh:79` has a TODO to send one; scripts only log or exit non-zero. | None. No Slack, mail, webhook, Prometheus or Sentry code in the v2 tree. | **GAP** | Gap in both. Needs a decision on the channel, then build. Recommendation: needs a decision on the channel, then build (gap 4). |
| D8 | Backups of Postgres and Redis: described in `HIDDEN_README_OPERATIONAL.md` (daily dump cron); `scripts/backup.sh` is not in the repo, so not verified. | None documented. The state is rebuildable by a full pass, but checkpoints, leases and run history would be lost. | **GAP** | Low impact. Needs a decision. Recommendation: needs a decision. |

### 2.5 What finance and operators rely on, and the couplings to other systems (rows E1 to E7)

| # | v1 item | v2 equivalent | status | note or recommendation |
|---|---|---|---|---|
| E1 | Power BI datamart: `dw_1_gold_bi` (BI_FinanceTable, BI_SalesOrder, BI_ProjectPortfolioPnL, BI_ProjectBudgetRecon, RLS_sales_person, one assertion view). Read by people (user logins, not a service account): in the last 45 days BI_SalesOrder 263 query jobs, RLS_sales_person 226, BI_ProjectPortfolioPnL 15, BI_ProjectBudgetRecon 11, BI_FinanceTable 10. | `visma_datamart`: the same 6 objects with identical schemas. Access: `dataform/tools/setup_gcp.py` provisions only the `visma-dataform` service account. The dataset ACL of `visma_datamart` (read at the snapshot) lists the owners, project readers and writers and `visma-dataform`; there is no Power BI identity. | **PARTIAL** | Data covered; the connection is not set up. v1's Power BI reads came from user logins, so access probably follows project-level roles (project IAM not read). Dataset names change: `dw_1_gold_bi` to `visma_datamart`, `dw_1_gold_*` to `visma_gold_*`, `dw_1_silver_*` to `visma_silver*`; table names are unchanged. The old empty dataset `datamart` is not used. Recommendation: build (gap 3). |
| E2 | Excel mapping workbook `Dataform/finance docs/reference_data.xlsx`: loaded by hand into `dw_1_bronze_excel.raw_entity_mapping` and `raw_charts_of_accounts` (no loader script in the repo), feeding 17 `dw_1_silver_excel` tables. | `visma_bronze_excel`: the same 2 tables, a snapshot of 2026-10-09 (`dataform/tools/copy_external_inputs.py`); the 17 `visma_silver_excel` tables are built and identical in schema. | **PARTIAL** | A manual snapshot. A change to the workbook after 2026-10-09 must be loaded by hand into the new project. Recommendation: needs a decision (gap 7). |
| E3 | Sales-person directory seed `Dataform/definitions/bronze/excel/Selgere 20260420.xlsx` into `seed_sales_person_directory` and `RLS_sales_person`. | Ported with the silver_excel definitions; `RLS_sales_person` has 96 rows on both sides. | **COVERED** |  |
| E4 | Currency service (`apps/currency_service`, container `aquatiq-currency-service`, cron Mon to Fri 16:10 fetch and 16:30 BigQuery sync, from the 2026-06-24 deploy notes, not re-checked) writing `dw_1_staging_currency_n2`: `Rate`, `Historyrates` and the two Power BI views. All Dataform currency models read it. | `dw_staging_currency` in `aquatiq-datavarehus`: the same 4 objects, a snapshot of 2026-10-09 (Rate 14 rows on both sides; Historyrates 11,684 against 11,696 in v1 when checked). The service still writes `prj-dw-dev`. | **GAP** | Build: repoint the service (project and dataset settings) or add a copy job. The service is outside this repo; how its config is changed was not checked. Recommendation: build (gap 2). |
| E5 | Dataform repository `Datawarehouse` (`prj-dw-dev`, europe-north1, workspace `Dev`, release config `production`); `Dataform/sync_to_gcp.py`. | Repository `Visma` in `aquatiq-datavarehus`; `dataform/tools/deploy.py` (push, compile, run, commit), `setup_gcp.py`, the parity and gate tools. | **COVERED** | The whole `dataform/` project is untracked in git, and `main` only moves with `deploy.py commit` (roadmap decision 2). |
| E6 | Finance reconciliation artefacts: `docs/Avvik mellom visma og superset - *.xlsx` (4), `scripts/recon_excel_postfix*.py`, `logs/reports/AQUATIQ_AS_REVENUE_GAP_REPORT_2026-05-15.md`. | One-off investigations of BI_FinanceTable against finance's Excel files. v2 has its gate reports in `docs/gates/` and `docs/dataform_validation.md`. | **N/A** | Not needed: one-off investigations. |
| E7 | `sync/shopify_integration/` (Visma to Shopify product, inventory and order sync). | None. | **N/A** | Not imported by `app.py` or any script and absent from `docker-compose.prod.yml`; its `order_sync.py` imports a `shopify_client` module that is not in the tree. |

### 2.6 What the sync fetches (rows F1 to F1)

| # | v1 item | v2 equivalent | status | note or recommendation |
|---|---|---|---|---|
| F1 | 83 endpoint configs (`sync/endpoint_configs/*.json`); 74 live bronze tables in `dw_1_bronze_visma` (84 are declared in the v1 Dataform; 10 declared names never had a table); tiers in `app.py`: T1 19, T2 32, T3 30 endpoints. | 135 endpoint configs (`config/endpoints`: 96 ERP REST, 38 DataMart GraphQL, 1 Expense), 135 staging tables, 74 bronze tables. | **COVERED** | 79 of the 83 v1 configs have a same-named v2 config; `invoices` is `customer_invoices`, `price_classes` is derived in Dataform from customers and locations, `customers_v2` and `sales_orders_v2` (retired API) are not carried. 63 staging tables have no bronze table: the 38 `dm_*` tables and 25 ERP or Expense endpoints that v1 never had in bronze. |

### 2.7 The v2 operations items, from the code

| item | status | what the code shows |
|---|---|---|
| Scheduler (`vsv2 schedule`, compose service `vsv2`) | **BUILT, NOT RUNNING LIVE** | Compose service `vsv2` runs `schedule --config-dir config/endpoints --tenants-dir config/tenants`, `restart: unless-stopped`, healthcheck `vsv2 healthcheck`, ERP budget 240 requests/min and 345,600/day, DataMart 120/min. Three tiers (hourly, daily, weekly full), due work read from `vsv2.scheduler_pass`, per-tenant Redis lease, 4 tenants in parallel, graceful stop on SIGTERM. 20 integration tests in `vsv2-orchestrator/tests` (scheduler 8, tenant_register 6, stale_runs 4, lease 2; they need Postgres, not run here). No live run: `dw_staging_visma` has no `_sync_heartbeat` table and its newest table write is 2026-10-09 17:10 UTC (checked 2026-10-12 21:33 UTC). No host chosen and no systemd unit or deploy automation; `docker-compose.prod.yml` (untracked) adds the production overrides described in row B3. The HTTP `/v1/schedules` API stores and validates cron text but nothing evaluates it; only `POST .../trigger` runs a schedule. |
| Backfill | **PARTIAL: windowed from the CLI** | `vsv2 sync --tenant X --endpoint E --date-from D --date-to D` (inclusive bounds); `--full` ignores checkpoints and soft-deletes what Visma no longer returns; for the four period-scan endpoints (`general_ledger_transactions`, `general_ledger_balances_v2`, `journal_transactions`, `project_transactions`) the window sets which financial periods are walked (`erp_adapter.rs period_range`; with `resume: true` the periods are always the whole range and the window is a modification filter). `POST /v1/sync/backfill` runs the same asynchronously. There is no `--period YYYYMM` flag; period backfill is by date window. `scripts/full_sweep_replay.sh` needs `ALLOW_FULL_SWEEP=YES`. |
| Monitoring and alerting | **DATA EXISTS, NO ALERTING** | Postgres: `sync_run`, `sync_checkpoint`, `sync_page_metric`, `scheduler_pass` (status, `failed_names`, `heartbeat_at`, `abandoned`). BigQuery: `_sync_heartbeat` is written by `vsv2-bq-stage` after each successful staging write (code in `tooling/.../heartbeat.py`), but the table does not exist yet. MCP `visma_sync_status`: checkpoints, recent runs, runs in the last 24 h, `stale_hours` filter. Gates a, b, c, e and s are scripts run by hand (`scripts/run_gates.py` runs all five). Logs go to stdout through `tracing`. Nothing evaluates any of this unattended and nothing notifies anyone. |
| Health check | **BASIC** | `vsv2 healthcheck` (compose `vsv2`, every 30 s): Postgres and Redis connectivity and the lease path; it passes while the scheduler loop is stuck. `GET /v1/health` (`vsv2-http`): Postgres only, no auth, no compose healthcheck for that service. The Dockerfile has no `HEALTHCHECK`. v1's `/health` returned a static `healthy`. |
| Config validation | **WORKS, NOT ENFORCED** | `make validate-config` runs `vsv2-validate-config`: JSON schema, `endpoint_name` equals the file name, ERP endpoints need an executable HTTP path and an explicit pagination profile, period-scan and partitioning settings must be consistent, DataMart query files and normalizers must exist. It printed `OK, validated 135 endpoint config file(s)` when run in this session. Tenant files are checked by the Rust loader, not by this validator (`config/tenants/README.md`: an unknown field is an error, two tenants may not share a company id; not re-tested). No CI runs the validator: neither GitHub workflow mentions `visma_service_v2`, although `README.md` says CI must. |
| Secrets handling | **ADEQUATE FOR ONE HOST, NO MANAGER** | `.env` is git-ignored (`git check-ignore` confirms; untracked) and excluded from the image by `.dockerignore`; `.env.example` holds placeholders and dev defaults only. The Google key is mounted read-only at `/run/secrets/gcp.json`, not baked into the image; the container runs as uid 1001. `vsv2-http` refuses to start without a non-empty `VSV2_HTTP_BEARER_TOKEN` and compares it in constant time. Write tools need a separate scope, allow-lists and `VSV2_WRITE_CONFIRM_SECRET`. Weak points: no secret manager (v1's `TokenManager` was backed by GCP Secret Manager); `AppConfig` and `OAuthClient` derive `Debug` over plain `String` secrets and DSNs (no place that logs them was found); `docker-compose.yml` carries a development password for Postgres and Redis has none (both bound to 127.0.0.1 there; `docker-compose.prod.yml` requires `VSV2_POSTGRES_PASSWORD` and publishes neither port, Redis stays without a password inside the compose network); `vsv2 token` prints a bearer token by design. The service-account key committed in the old Dataform repository (`docs/V1_STATUS_AND_ROADMAP.md`) is a separate open item. |
| Tenant onboarding | **MANUAL, SEVERAL PLACES** | (1) Grant the Visma app for the tenant and scopes, outside the code. (2) Add `config/tenants/<name>.json` (unknown field is an error, two tenants may not share a company id). (3) `vsv2 dm-companies --tenant <id>` and put the result in `datamart_company_id`. (4) `vsv2 tenants-register [--dry-run]` (`schedule` also registers at start). (5) Restart `vsv2`: tenants are read once at start; `config/` is a read-only bind mount, so no rebuild. (6) Add the tenant to `dataform/definitions/bronze/visma/visma_tenants.sqlx`, a hard-coded list of 12 rows that supplies `organizationName`. (7) Add its entity and accounts to the Excel mapping workbook. (8) `scripts/tenant_scan_compare.py` holds a third hard-coded list used by gate-b. v1 was comparable: `add_tenants.py`, tenant arrays in three cron scripts, and `visma_sync_common.sh`. |
| Hot-path `unwrap()` and `panic!` in non-test Rust code | **NONE IN THE SYNC HOT PATH** | Counted over the 110 files under `core/crates/*/src` (plus `build.rs`) with every `#[cfg(test)]` item and every file declared `#[cfg(test)] mod x;` removed and comments and string literals masked. Non-test runtime code: **`.unwrap()` 0**, `.expect(` 13, **`panic!` 2**, `unreachable!`, `todo!`, `unimplemented!` and `assert!` 0. `vsv2-mcp/build.rs` (build time only): `.expect(` 5, `panic!` 2. For comparison, plain `grep` over `*/src` counts 379 lines with `.unwrap()`, 87 with `.expect(` and 14 with `panic!(`, nearly all inside test modules; `*/tests` adds more. `docs/CUTOVER_READINESS.md` (2026-06-03, since replaced by `PRODUCTION_READINESS.md`) counted 33 `unwrap()`/`panic!`; that figure is out of date. Where they are: client constructors (`reqwest::Client::builder().build().expect`), run once at start: `vsv2-auth/oauth.rs:57`, `vsv2-datamart/client.rs:57`, `vsv2-erp-rest/client.rs:76`, `vsv2-expense/client.rs:40`, `vsv2-interactive/transport.rs:60`; signal-handler install in `vsv2-http`, at start: `vsv2-http/main.rs:103`, `vsv2-http/main.rs:109`; `Mutex::lock().expect`, panics only if the mutex is poisoned: `vsv2-interactive/audit.rs:78`, `vsv2-interactive/audit.rs:89`, `vsv2-mcp/results.rs:56`, `vsv2-mcp/results.rs:70`, `vsv2-mcp/warehouse/mod.rs:168`; HMAC key (`new_from_slice` accepts any length): `vsv2-interactive/plan.rs:84`; `panic!` on invalid embedded resource (MCP server start): `vsv2-mcp/known_issues.rs:33`, `vsv2-mcp/skills.rs:280`. None is in the executor, scheduler, paginators, BigQuery writer or state store; `vsv2-erp-rest`, `vsv2-datamart` and `vsv2-expense` have one constructor each. Amplifier: `[profile.release] panic = "abort"`, so any panic would end the whole scheduler process for all tenants; compose restarts it and the schedule is read from the database. Not counted: indexing, slicing, arithmetic overflow and other implicit panics. Counted again at the time of writing because the tree is changing. |

## 3. Gaps, ranked by impact

Impact is judged against the cutover (Power BI reading `aquatiq-datavarehus` and v1 retired). Recommendation: build, not needed, or needs a decision.

| rank | gap | impact | evidence | recommendation |
|---:|---|---|---|---|
| 1 | Nothing refreshes the new warehouse: the vsv2 scheduler has not written to the new staging since the snapshot and Dataform has no schedule | The new project is a frozen snapshot of 2026-10-09. v1 refreshes hourly and daily and rebuilds Dataform every 4 h. Cutover is impossible until both run. | `dw_staging_visma`: no `_sync_heartbeat`, newest write 2026-10-09 17:10 UTC; `dataform/README.md`: "Nothing is scheduled"; roadmap "Scheduling: none". GCP not queried for a workflow config. | Build. Needs a decision on the host (roadmap decision 1) and on the Dataform trigger (workflow config or a function like `dataform-refresher`). |
| 2 | Currency rates are a snapshot | Every NOK conversion (`BI_FinanceTable.amount_nok`, market rates, `dim_currency*`) freezes at 2026-10-09 in the new project. | The currency service still writes `prj-dw-dev.dw_1_staging_currency_n2`; `dw_staging_currency` was copied once (`copy_external_inputs.py`). | Build: repoint the service to `aquatiq-datavarehus.dw_staging_currency`, or add a copy job. |
| 3 | Power BI access to the new warehouse is not set up | The five BI tables are identical (6 of 6 objects), but the connection and dataset names must change, and nothing in the code or in the dataset ACL provisions Power BI. | `setup_gcp.py` provisions only `visma-dataform`; `visma_datamart` ACL: owners, project readers and writers, `visma-dataform`. People ran about 9,000 query jobs on `dw_1_gold_*` and `dw_1_silver_excel` in 45 days (user logins). Project IAM was not read. | Build: confirm or grant read access for whoever refreshes Power BI, and change the connection. Needs a decision on the cutover date. |
| 4 | No alerting and no unattended monitoring | A failed or stalled sync is silent. v1 had cron jobs for it (`catchup-guardian.sh`, `monitor_pm_pipeline.py`) but only logged. | No notifier code in v2; `_sync_heartbeat` not created; gates run by hand; `vsv2 healthcheck` passes with a stuck scheduler. | Build a small scheduled check (heartbeat staleness, failed `scheduler_pass`, gate-c) and decide the channel. |
| 5 | Finance transactions drop from hourly to daily | `general_ledger_transactions`, `expense_claims`, `fixed_asset_transactions`, `customer_credit_notes`, `stocktakes_v2` are hourly in v1; daily in v2 (`time_cards` weekly). With a daily Dataform run, the BI finance data is up to a day old instead of about five hours. | `app.py _TIER_1_ENDPOINTS` against `config/endpoints/*.json` (`tier` unset). | Needs a decision. Setting `tier: 1` is a one-line config change; for GL a pass walks every period from 2020-01 plus 12 months ahead (about 94 period requests per tenant, short when little changed). |
| 6 | The nightly 7-day replay was not carried over | v1's documented safety net for missed changes. v2 has a 24 h overlap on GL, journal and transaction endpoints, 10 min elsewhere, and the weekly full pass. A payment on an open invoice leaves the balance stale until then; that case is being closed for customer and supplier invoices only (open-set reconciliation, `docs/DEPLOYMENT.md` section 4b), not for purchase or sales orders. | `executor.rs overlap_watermark`; `docs/v2_roadmap.md` phase 1; the open-set code and its documentation appeared in the tree while this was written (uncommitted, not reviewed by me). | Needs a decision: accept the open-set reconciliation for invoices, and decide on a daily replay window for the rest. |
| 7 | The Excel mapping workbook is a manual snapshot | 17 `silver_excel` tables and the business-unit and account mapping depend on it. A workbook change after 2026-10-09 does not reach the new warehouse. | No loader script for `reference_data.xlsx` in v1 or v2; `visma_bronze_excel` copied once. | Needs a decision: keep the manual load and document it, or build a loader. |
| 8 | Per-tenant endpoint capability handling is missing | Where a tenant lacks a Visma module (v1 saw HTTP 400 clusters on 13 tier-1 endpoints for some tenants), the endpoint fails on every pass and is retried every 15 min, 2 h or 12 h. | `vsv2.endpoint_capability` exists (migration 0004) and is used nowhere; `SCHEDULER_POLICY_AND_ENDPOINT_AUDIT.md`. | Needs a decision after the first live pass shows which tenant and endpoint pairs fail. |
| 9 | Quality checks that v1 had are not ported | Business-unit mapping coverage and overlaps, BU-name null rate, PO supplier coverage (4 views in `dw_1_silver_quality`), and the 60 % project-coverage floor of `monitor_pm_pipeline.py`. | No equivalent in `visma_assertions` or `dataform/definitions/assertions`. | Build: small SQL assertions. |
| 10 | 20 columns are filled in v1 and empty in v2, plus partial `dim_inventory_item` | 14 tables. 10 carry a "v2 does not fetch this field" marker in the bronze SQL (`businessUnitId` and `businessUnitName` on journal transactions, dunning letter date and level on credit and debit notes, `errorInfo`, `exportType`, `referenceNumber`, and `lines`, which `orderLines` replaces); 2 are `deletedAt` columns filled in one v1 row each; 8 are derived columns (price-class evidence, `source_warehouse_key`, sample customers) whose cause is not established. Separately, the `dim_inventory_item` availability columns are filled for 58 % of rows against 98 % in v1, in three layers. | Section 1.4; `compare_columns.py` run in this session. | Needs a decision; the 8 derived columns and the availability fill need a value investigation, which this report does not do. |
| 11 | Operations hygiene for v2 | No CI, migrations applied by hand, no state backup, no secret manager, and work not in git. | No workflow mentions v2; `migrations/` has no runner (idempotent SQL, applied through `make migrate` or by hand); `git status` in `visma_service_v2`: 199 modified and 263 untracked entries, including the whole `dataform/` project and 54 endpoint configs. | Build CI (`make validate-config`, `cargo test`, `pytest`); decide on committing (roadmap decision 2) and on state backup. |
| 12 | v2 still points at the old warehouse in a few places | The MCP warehouse tools would query the wrong project: their default dataset list was changed to the `visma_*` names while this was written, but the launcher still defaults the project to `prj-dw-dev`, where those datasets do not exist. | `vsv2-mcp/src/warehouse/mod.rs` `DEFAULT_DATASETS` (`visma_*`, uncommitted) against `scripts/run_mcp.sh` (`VSV2_MCP_BQ_PROJECT` defaults to `prj-dw-dev`). The parity and gate tools read v1 on purpose. | Build: change the launcher default at cutover (or now). |
| 13 | Old objects with no v2 equivalent (26, plus 10 relocated copies) | None is needed by the running warehouse: 2 backups, the retired `visma_sales_orders_v2` (7 rows, which people read until 2026-10-01), and 19 stale silver tables and 4 quality views written once on 2026-04-16. The 3 MERGE chunk tables v1 leaves in bronze are not counted. | Sections 1.3 and 1.6; reads from `INFORMATION_SCHEMA.JOBS_BY_PROJECT`. | Not needed. Confirm with the readers of `visma_sales_orders_v2` before v1 is archived. |
| 14 | v1 functions deliberately not carried over | Interactive OAuth flow and stored tokens, the ODS builder, the API cache, live pass-through routes, the Shopify module, repair one-offs, the 117 per-endpoint validators. | Part 2 rows A2, A6, A7, A10, D3, D6, E7. | Not needed (reasons in the rows). |

## 4. What was not verified

* Value differences. Row counts are shown for orientation only; they are not compared or explained (see `docs/dataform_v1_comparison.md`, `dataform_validation.md`).
* Whether the deployed `Datawarehouse` Dataform repository in GCP matches the local `Dataform/` copy. The BigQuery state has 72 objects in `dw_1_silver_visma` and 29 in `dw_1_gold_common`; the local copy defines 53 and 19. "In v1 repo" below refers to the local copy.
* Fill is counted on the whole table, with no tenant or date filter, so it answers "empty or not", not "how full". For `dw_1_bronze_visma.visma_supplier_invoices` (4.4 GB) only the six columns that were empty in the new table were counted on the old side. Views were counted through the view.
* Who reads a table: `INFORMATION_SCHEMA.JOBS_BY_PROJECT` (45 days, SELECT statements without a Dataform label, region europe-north2). "People" means any non-service-account user, so Power BI under a user login cannot be told from analysts or from Superset (the docs name Superset as a consumer of `dw_1_gold_bi`; there is no Superset configuration in the repo). Reads in europe-west1 and in other projects are not covered.
* Partitioning, clustering, descriptions and labels are not compared. Dataset ACLs were read only for `visma_datamart` and `dw_1_gold_bi` (roles and kinds of principal); project-level IAM was not read. Nullability is identical in all 220 pairs.
* GCP resources outside BigQuery were not queried: Cloud Scheduler, Cloud Functions, Dataform workflow configs, the VPS crontab and containers, Power BI. v1 runtime facts come from the repo, `docs/` and the notes of earlier sessions; the scheduled trigger is described in one place as a VPS cron and in another as a local crontab (memory note of 2026-07-08), so which one is live is not verified.
* No tests were run (they need Postgres and Redis), no Visma call was made, no BigQuery table was written.
* The tree changed while this was written, by other work: the `open_set` reconciliation in `vsv2-orchestrator`, gate-e (`verify_endpoint_counts.py`), `docker-compose.prod.yml`, the MCP default datasets, more assertions in Dataform, and a `scanPeriod` column in `visma_project_transactions`. Each affected row says so; the numbers are as of the snapshot times given.
