---
id: warehouse-navigation
title: Navigating the warehouse
description: What each readable BigQuery dataset is for, how tables are named, which layer to prefer, and the explore sequence (overview, list, describe) before writing any SQL.
tags: [warehouse, bigquery, discovery]
tools: [warehouse_list_tables, warehouse_describe_table, warehouse_overview, warehouse_query]
primary: [warehouse_list_tables, warehouse_describe_table, warehouse_overview]
prerequisites: [start-here]
difficulty: beginner
steps: 3
---
# Navigating the warehouse

The warehouse is BigQuery project `aquatiq-datavarehus`. Only the datasets below are readable (`visma_init` shows the
configured list); anything else is refused. **Prefer the highest layer that has what you need.**

## Layers and datasets

| Dataset | What it is | Use it for |
|---|---|---|
| `visma_datamart` | `BI_*` flat reporting models, the Power BI layer: `BI_FinanceTable`, `BI_SalesOrder`, `BI_ProjectPortfolioPnL`, `BI_ProjectBudgetRecon`. `RLS_sales_person` is the report-access mapping (user names), not business data: leave it alone unless the user asks about report access | Management reporting, NOK amounts, ready-made categories |
| `visma_gold_finance` | Finance facts and dimensions: general ledger, GL balances, customer/supplier invoices, payments, credit notes, project lines, inventory documents | Accounting analysis, AR/AP, drill-down |
| `visma_gold_sales` | Sales and purchasing: `dim_sales_order`, `fact_sales_order_line`, `dim_shipment`, `fact_shipment_detail_line`, `dim_purchase_order`, `fact_purchase_order_line` | Order, shipment and purchase analysis |
| `visma_gold_common` | Shared dimensions and bridges: `dim_company`, `dim_business_unit`, `dim_date`, `dim_project`, `dim_currency*`, `dim_employee`, `dim_department`, `dim_warehouse`, ... | Joins, names, calendar, currency rates |
| `visma_silver` | Cleaned, typed, line-level layer that gold is built from (more tables, finer grain, for example `fact_customer_invoice_line`) | When gold lacks a column or grain |
| `visma_silver_common` | Reconciliation and gap tables: `qa_gl_project_gap_*`, `source_gap_*`, price-class gap reports | Explaining differences (skill `project-profitability`) |
| `visma_bronze` | The synced API data, typed, one table per endpoint (`visma_customers`, `visma_sales_orders_v3`, ...), built from staging. One row per record (checked on the main tables); a record Visma stopped returning is kept and flagged `isDeleted` | Debugging, or a field gold does not carry. Filter `NOT IFNULL(isDeleted, FALSE)`. |
| `dw_staging_visma` | The landing layer the v2 sync writes: one table per endpoint with `tenant_id`, `record_key`, `payload` (the record as JSON), `updated_at`, `deleted_at`. Bronze is built from it, and it also holds endpoints bronze does not (the DataMart `dm_*` tables) | The raw record (`JSON_VALUE(payload, '$.field')`, live rows only: `deleted_at IS NULL`) and the sync's own recency (skill `data-freshness`) |

Not readable here: `visma_assertions` (the Dataform assertion views, `assert_*`), `visma_silver_excel` and
`visma_bronze_excel` (Excel mapping inputs) and everything else in the project (Xero, currency staging). Listing or
describing them, or reading a table in them, is refused (skill `error-triage`).

## Naming conventions

- `dim_*` descriptive entities; `fact_*` events and documents; `bridge_*` mappings; `BI_*` flat reporting models;
  `assert_*` quality checks; `qa_*` and `source_gap_*` reconciliation and coverage.
- `*_key` columns are hashed surrogate keys (large integers; `account_key` is a string). Join on them; do not try
  to read meaning from them. `date_key` joins to `dim_date` (skill `warehouse-sql-basics`).
- Amounts carry suffixes for their currency (skill `multi-currency`).

## The explore sequence

1. **How big and how recent?** `warehouse_overview`, no argument: one line per dataset (tables, rows, newest write).
2. **What is in a dataset?** `warehouse_list_tables` with `dataset`, or `warehouse_overview` with `dataset` for row
   counts and last-modified per table.
3. **What are the columns?** `warehouse_describe_table`: names, types, descriptions, row count. Do this before
   writing SQL against a table you have not used in this conversation. Column names differ between tables
   (`financial_period` vs `application_period` vs `post_period`).
4. **Look at a few rows** with an explicit column list and a small `max_rows`.

```tool-call
warehouse_overview
```

```tool-call
warehouse_overview
{"dataset": "visma_datamart"}
```

```tool-call
warehouse_list_tables
{"dataset": "visma_gold_finance"}
```

```tool-call
warehouse_describe_table
{"dataset": "visma_gold_finance", "table": "fact_general_ledger"}
```

A first look at a few rows (never `SELECT *` on a wide table):

```tool-call
warehouse_query
{"sql": "SELECT tenant_id, financial_period, account_cd, account_type, debit_amount, credit_amount FROM `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger` WHERE financial_period = '202509' ORDER BY transaction_date LIMIT 5", "max_rows": 5}
```

## Pitfalls

- A view and a table look the same to a query; `warehouse_overview` marks views and gives no row count for them.
- Tables hold *all* companies: a missing company filter silently mixes them (skill `tenants-and-entities`).
- `warehouse_overview` shows when a table was last *written*, not whether its contents are correct or complete. All gold
  tables are rebuilt together (within a few hours of each other), so their write time says nothing about how fresh the
  source data is. Staging and bronze times are not the data's age either: staging was copied into this project after
  it was filled, bronze is rebuilt from it, and the data itself is a snapshot (skill `data-freshness`).
