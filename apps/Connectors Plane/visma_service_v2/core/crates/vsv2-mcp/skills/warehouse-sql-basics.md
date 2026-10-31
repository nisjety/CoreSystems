---
id: warehouse-sql-basics
title: Writing correct warehouse SQL
description: The rules every warehouse_query must follow (qualified names, company and period filters, typed results, hashed keys and the dim_date join, limits) with validated patterns.
tags: [warehouse, sql, bigquery, joins]
tools: [warehouse_query, warehouse_describe_table]
primary: [warehouse_query]
prerequisites: [warehouse-navigation, tenants-and-entities]
difficulty: beginner
steps: 4
---
# Writing correct warehouse SQL

`warehouse_query` runs **one** read-only BigQuery Standard SQL statement (`SELECT` or `WITH`). Everything else
(DML, DDL, scripts, `;` followed by more SQL, other datasets, other projects) is refused before it runs.

## Rules

1. **Fully qualify tables in backticks**: `` `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger` ``. Metadata is
   allowed only dataset-qualified, for example `` `aquatiq-datavarehus.visma_gold_finance`.INFORMATION_SCHEMA.COLUMNS ``.
2. **Always filter by company and time.** Tables hold every company and several years. Use `tenant_id` where the
   table has it (GL, GL balance, project lines, sales order lines, many dimensions) or join `dim_company` through
   `company_key` where it does not (invoices, payments).
3. **Periods are strings** in `YYYYMM` form: `financial_period = '202509'`, `BETWEEN '202501' AND '202512'`.
   Compare strings with strings.
4. **Describe before you query** a table you have not used yet (`warehouse_describe_table`). Never invent a column.
5. **Select only the columns you need.** `SELECT *` on a wide table (some have 90+ columns) scans far more than
   necessary. Each query is limited in bytes scanned (and there is an hourly budget); a company + period filter
   usually keeps a query in the megabytes.
6. **Make truncation deterministic**: a listing needs an `ORDER BY`, and `max_rows` (default 100, at most 500) cuts
   it. `truncated: true` means more rows matched (skill `honest-results`).
7. **Do not mix signs or currencies** between tables (skills `general-ledger-reporting`, `multi-currency`).

## Result types

`INT64` and `FLOAT64` come back as numbers, `BOOL` as booleans. `NUMERIC` and dates/timestamps come back as
strings (no precision is lost). Round money in SQL (`ROUND(x)`) rather than in your answer.

## Hashed keys

`*_key` columns (`company_key`, `customer_key`, `account_key`, `date_key`, ...) are hashed surrogate keys. They
join correctly and mean nothing on their own. In particular **`date_key` is not `YYYYMMDD`**: join
`visma_gold_common.dim_date` on it for calendar attributes (`date_day`, `year`, `month`, `year_month` as
`'2025-09'`, `quarter`, `week_of_year`, `is_weekend`). The date it points to is the fact's own date (invoice
document date, GL transaction date); this was checked to match on every row.

Monthly invoiced amount in NOK using the calendar dimension (voided invoices excluded):

```tool-call
warehouse_query
{"sql": "SELECT d.year_month, COUNT(*) AS invoices, ROUND(SUM(i.amount_in_currency_market_nok)) AS invoiced_nok FROM `aquatiq-datavarehus.visma_gold_finance.fact_customer_invoice` i JOIN `aquatiq-datavarehus.visma_gold_common.dim_date` d USING (date_key) WHERE d.year = 2025 AND i.status <> 'Voided' GROUP BY d.year_month ORDER BY d.year_month"}
```

Company names for a fact that has only `company_key`:

```tool-call
warehouse_query
{"sql": "SELECT c.entity_name, COUNT(*) AS open_invoices FROM `aquatiq-datavarehus.visma_gold_finance.fact_customer_invoice` i JOIN `aquatiq-datavarehus.visma_gold_common.dim_company` c USING (company_key) WHERE i.status = 'Open' GROUP BY c.entity_name ORDER BY open_invoices DESC"}
```

## Useful BigQuery patterns

- Conditional sums: `SUM(IF(cond, amount, 0))`; counts: `COUNTIF(cond)`.
- Safe ratios: `SAFE_DIVIDE(a, b)` returns `NULL` instead of an error.
- Dates: `DATE_DIFF(CURRENT_DATE(), due_date, DAY)`, `FORMAT_DATE('%Y%m', transaction_date)` (compare with
  `financial_period`), `DATE_TRUNC(d, MONTH)`.
- Text search: `REGEXP_CONTAINS(LOWER(description), r'fix|adjust')`.
- Running totals and differences: `SUM(x) OVER (PARTITION BY ... ORDER BY ...)`, `LAG(x) OVER (ORDER BY ...)`.
- A trailing `;` is tolerated; a second statement is not.

A listing is bounded by `max_rows` and an `ORDER BY`; the biggest open invoices with customer names:

```tool-call
warehouse_query
{"sql": "SELECT c.customer_name, i.reference_number, i.document_due_date, i.balance_in_currency_market_nok AS balance_nok FROM `aquatiq-datavarehus.visma_gold_finance.fact_customer_invoice` i JOIN `aquatiq-datavarehus.visma_gold_finance.dim_customer` c USING (customer_key) WHERE i.status = 'Open' ORDER BY i.balance_in_currency_market_nok DESC LIMIT 10", "max_rows": 10}
```

## Before you answer

State the company (name, not just `tenant_id`), the period, the currency and the table you used. If the answer
rests on a status filter (`status = 'Open'`), say so: statuses differ by table (skill `receivables-payables`).
