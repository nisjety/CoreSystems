---
id: error-triage
title: Error triage
description: What each error message from this server means, whether to retry, and the exact fix. Use when a tool call fails or is refused.
tags: [meta, errors, troubleshooting, safety]
tools: [warehouse_query, warehouse_describe_table, warehouse_list_tables, warehouse_overview, visma_erp_query, visma_salesorder_get, visma_datamart_query, visma_datamart_schema, visma_read_result, visma_list_tenants, visma_sync_status, visma_salesorder_patch_plan, visma_salesorder_patch_execute]
primary: []
prerequisites: [honest-results]
difficulty: intermediate
steps: 3
---
# Error triage

Answer a failure with: what failed, the likely cause, the exact fix, and whether a retry is useful. Then
classify it (`blocked`, `unknown`, `partial`; see skill `honest-results`). Never turn a failure into "no data".

## Warehouse (BigQuery)

| Message contains | Cause | Fix | Retry? |
|---|---|---|---|
| `query rejected: only SELECT / WITH queries are allowed` | The SQL is not a read query | Write one `SELECT` or `WITH` statement | After fixing |
| `query rejected: only a single statement is allowed` | A `;` before the end | Remove it, one statement only | After fixing |
| `` `X` is not allowed in a read-only query (quote it with backticks...) `` | A forbidden word (`DELETE`, `CREATE`, `ML.`/`AI.` calls...) appears outside a string | If it is a column name, wrap it in backticks; otherwise the request is out of scope | After fixing |
| `dataset X is not readable here (allowed: ...)` | Dataset outside the allow-list (for example Xero or Excel data) | Use an allowed dataset, see `visma_init` | No |
| `table P.D.T is outside project ...` | Cross-project reference | Use `aquatiq-datavarehus.<dataset>.<table>` | After fixing |
| `query would scan N bytes, over the M byte limit` | Too much data | Filter on company and period, select only the columns you need | After narrowing |
| `hourly scan budget used up` | Many large queries in the last hour | Wait, or narrow the queries | Later |
| `BigQuery reports statement type X, only SELECT is allowed` | The SQL is a script or DML | One plain `SELECT` | After fixing |
| `BigQuery error: Unrecognized name: X` | Unknown column (or a name that differs between tables) | `warehouse_describe_table`, then use the exact column name | After describing |
| `BigQuery error: Not found: Table ...` | Wrong table or dataset, or a table that is not created yet (`dw_staging_visma._sync_heartbeat` before the first sync, skill `data-freshness`) | `warehouse_list_tables` | After listing |
| `BigQuery error: ... No matching signature` | Type mismatch, for example a string compared with a number | Check the column type in `warehouse_describe_table` (periods are strings: `'202509'`) | After fixing |
| `table D.T not found or has no columns` | Name typo | `warehouse_list_tables` | After fixing |

## Live Visma and DataMart

| Message contains | Cause | Fix | Retry? |
|---|---|---|---|
| `unknown tenant "X"` | The company is not configured here | `visma_list_tenants`, use the id or exact name | After fixing |
| `unknown endpoint "X"` | Name not in the catalog | `visma_list_endpoints` with `search` | After fixing |
| `endpoint X does not declare Y` | A filter name the API reference does not declare; refused before any read | Skill `api-reference` lists the names the endpoint accepts | After fixing |
| `only erp_rest endpoints can be queried live here` | You asked for a DataMart endpoint | Use `visma_datamart_query` | After switching |
| `endpoint X requires query parameter` | A mandatory filter is missing (for example `ledger` for general ledger transactions) | Add it in `query` | After fixing |
| `` `pageSize` is controlled by the `page`/`page_size` arguments `` | Paging parameters belong to the tool | Use `page` and `page_size` | After fixing |
| `sales order T/ID not found` | Wrong type or number (the key is the pair) | Check both parts; list orders to find it | After checking |
| `upstream returned non-success status 401` | Token rejected | Transient token problem: one retry | Once |
| `... status 403` | The integration lacks the scope or role | Report as `blocked`; do not infer the data is empty | No |
| `... status 404` | Record or endpoint absent for this company | Report as not found, not as zero | No |
| `... status 429` or `rate limit exceeded ... cooldown until` | Visma rate limit | Wait until the stated time; use fewer, larger-filtered calls | Later |
| `... status 5xx` / `Execution Timeout Expired` | Visma-side failure or a query that is too broad | Narrow the filter, retry once | Once |
| `query rejected: ... only queries` (DataMart) | A mutation or subscription was sent | DataMart is read-only | No |
| `The argument X does not exist` (DataMart, status 400) | GraphQL validates arguments; `first`/`after`/`where` do not exist | Page with `pageSize` and `lastId`, window with `updatedInDataMart`; skill `datamart-graphql` | After fixing |
| `SalesOrder API returned 400 ... InvalidValue` on a `expand` | An `expand` section that does not exist | Use `lines`, `customer`, `payment` or `linefees`; skill `sales-orders-live` | After fixing |
| `filter_check.verdict` is `no_effect_detected` (not an error) | Visma silently ignored a filter name it does not know | Do not present the page as filtered; use a confirmed filter name; skill `live-erp-queries` | After fixing |

## Results and handles

| Message contains | Cause | Fix |
|---|---|---|
| `unknown or expired result_id` | Handles live about ten minutes, and only in memory | Repeat the original query |
| `too large to hold for paging` | More than 5 MiB | Aggregate in SQL or add filters |
| `does not exist in this result` | Wrong JSON pointer | Read the `shape` of the truncated answer; the rows are under `/rows` |

## Writes

| Message contains | Cause | Fix |
|---|---|---|
| `write denied: writes are not enabled for tenant` / `operation X is not enabled` | Default deny: tenant and operation must be allow-listed | The operator must enable them; do not work around it |
| `invalid input: field X is not supported by salesorder.patch` | A field outside the supported set | Skill `edit-sales-order` lists what is supported |
| `invalid input:` with a `$.` path, e.g. `$.customr: unknown field` | The new order does not match Visma's ERP schema | Fix the named field and plan again; skill `create-sales-order` |
| `invalid input: sales order T/ID is Completed; closed orders are not patched` | The order is closed | Tell the user; do not try another route |
| `confirm token does not match this plan` | The plan was changed after planning | Plan again, pass the plan unchanged |
| `plan expired` | More than ten minutes since planning | Plan again |
| `already executed` | A plan runs at most once | Do not retry; read the order to see what happened |
| `not executed: the user did not approve` | The user declined | Stop; do not ask again unless the user does |
| `not executed: this connection cannot ask the user` | The client cannot show an approval prompt | Report it; do not look for a way around the approval |
| `upstream refused the write with status 412` | The order changed since the plan | Read the order again, plan again |
| `outcome unknown` | No answer arrived after sending; the write may or may not have applied | **Do not retry.** Read the order from Visma to see the real state first |

## Worked examples

Find the exact column before retrying a failed query:

```tool-call
warehouse_describe_table
{"dataset": "visma_gold_finance", "table": "fact_customer_invoice"}
```

Find the right endpoint name:

```tool-call
visma_list_endpoints
{"search": "supplier"}
```

See which companies are configured:

```tool-call
visma_list_tenants
```
