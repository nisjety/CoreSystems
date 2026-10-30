---
id: data-freshness
title: Is the data current? (freshness, snapshot, what each signal measures)
description: Which signals say how fresh warehouse data is, what each one really measures (rebuild time, copy time, last change, newest business date, sync state, heartbeat), that the data is currently a snapshot until the scheduler runs, which companies are quiet by design, and how to state the "as of" of an answer.
tags: [freshness, sync, monitoring, warehouse, meta]
tools: [warehouse_overview, visma_sync_status, warehouse_query]
primary: [visma_sync_status]
prerequisites: [start-here, honest-results]
difficulty: intermediate
steps: 4
---
# Is the data current?

"Last modified" is easy to read and easy to misread. Several signals exist; each answers a different question.

## The data is a snapshot

Checked 2026-10-12. Visma was read in one load on 2026-10-01 and 2026-10-02 (staging rows were first seen from
2026-10-01 10:47 UTC, and apart from one 6-row table re-synced on 2026-10-09 nothing is newer than 2026-10-02).
Dataform builds bronze, silver and gold from that staging, so they hold the same data however recently they were
rebuilt. Every company's rows carry the load time, the newest business dates are 2026-10-01 and 2026-10-02 (query
3 below), and the sync that keeps staging current (the scheduler) has not run in production yet. Everything that
depends on "now" is therefore as of the snapshot: an invoice paid or an order shipped after 2026-10-02 still looks
open here, and "overdue" is counted against today's date but over the snapshot's open set.
Until the scheduler has run, confirm a figure that drives a decision live (skill `live-erp-queries`), and check
`visma_sync_status` (and the heartbeat table below, once it exists) before you call anything "current".

## What each signal measures

| Signal | Where | It tells you | It does **not** tell you |
|---|---|---|---|
| Table `last_modified` in **gold, silver, datamart** | `warehouse_overview` | When Dataform last rebuilt the table. A run rewrites the gold tables within hours of each other. | Whether the source data is current. A fresh rebuild of stale data looks fresh. |
| Table `last_modified` in **bronze** (`visma_bronze`) | `warehouse_overview` with `dataset` | The Dataform run that last rebuilt the table: bronze is rebuilt from staging, so this is a build time. | Per-endpoint recency: bronze has none. |
| Table `last_modified` in **staging** (`dw_staging_visma`) | `warehouse_overview` with `dataset` | When the table was written into this project (a copy on 2026-10-09). | When Visma was read: the rows are from 2026-10-01/02. |
| `MAX(updated_at)` per company in a staging table, or `MAX(syncedAt)` in the matching bronze table | `warehouse_query` | When that company's rows last changed in the feed: the sync's own recency. | A quiet company and a broken sync look the same. |
| Newest **business date** in a gold fact | `warehouse_query` | The last day a report can claim ("through 2026-10-02"). | How recently it was loaded. Future-dated postings exist (GL up to December): cap at today. |
| `visma_sync_status` | tool | The sync **state database this server is connected to**: checkpoints per company and endpoint, runs, errors. That is v2's own pipeline, the one that writes staging. | The Dataform rebuild of bronze, silver and gold, which is a separate run. An empty answer means "nothing in this database", not "healthy" and not "broken". |
| `_sync_heartbeat` in `dw_staging_visma` | `warehouse_query` | One row per successful staging write (`tenant_id`, `endpoint`, `finished_at`, `status`, `rows_written`); `MAX(finished_at)` per company and endpoint is the last sync. | It does not exist yet: nothing has written it (checked 2026-10-12). A query on it fails until the sync runs. |

## How the data is refreshed

The scheduler runs three tiers: hourly for the ERP operational endpoints, daily for the standard endpoints, and a
weekly full pass over every enabled endpoint that also reconciles deletions (rows Visma no longer returns get
`deleted_at`). It refreshes staging only: bronze, silver and gold change when Dataform runs again, so a refreshed
staging does not reach the gold tables until then. The incremental path, the deletion pass and the rebuild after a
refresh have not been exercised on this warehouse yet: do not assume a record that changed after the snapshot is
right here.

**Quiet by design**: Aquatiq Food Factory AS, Hygiene Systems AS, Sense AS and Consult AS were merged into Aquatiq
AS. Their newest ledger postings are from 2026-02-09 (query 3b); their period close flags, open items and order
backlog are frozen there. That is the merger, not a failure (skill `tenants-and-entities`).

Staging also holds soft-deleted rows: a record Visma stopped returning, or a DataMart row flagged deleted, keeps its
row with `deleted_at` set (44,456 rows, all in the `dm_*` tables and one ERP table when checked). Read live rows with
`deleted_at IS NULL`.

## 1. How big and how recent is each dataset

```tool-call
warehouse_overview
```

Per endpoint table in staging (row counts show which endpoints returned nothing; the write times are the copy into
this project, not the age of the data):

```tool-call
warehouse_overview
{"dataset": "dw_staging_visma"}
```

## 2. When did each company's data last change

Customer invoices as the example (small tables). First the staging table: `first_loaded` and `newest_change` are
the sync's own times for the rows of each company:

```tool-call
warehouse_query
{"sql": "SELECT c.entity_name, MIN(s.first_seen_at) AS first_loaded, MAX(s.updated_at) AS newest_change, COUNT(*) AS rows_in_table, COUNTIF(s.deleted_at IS NOT NULL) AS soft_deleted FROM `aquatiq-datavarehus.dw_staging_visma.customer_invoices` s JOIN `aquatiq-datavarehus.visma_gold_common.dim_company` c USING (tenant_id) GROUP BY c.entity_name ORDER BY newest_change DESC", "max_rows": 20}
```

The same from the typed bronze table (swap in `visma_general_ledger_transactions` for the ledger, which scans about
100 MB):

```tool-call
warehouse_query
{"sql": "SELECT organizationName AS company, MAX(syncedAt) AS last_changed, COUNT(*) AS rows_in_table FROM `aquatiq-datavarehus.visma_bronze.visma_customer_invoices` GROUP BY organizationName ORDER BY last_changed DESC", "max_rows": 20}
```

## 3. What is the newest business date

Capped at today, with the uncapped value next to it so a future-dated posting is visible:

```tool-call
warehouse_query
{"sql": "SELECT 'fact_general_ledger' AS source, MAX(IF(transaction_date <= CURRENT_DATE(), transaction_date, NULL)) AS newest_business_date, MAX(transaction_date) AS newest_including_future FROM `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger` UNION ALL SELECT 'fact_customer_invoice', MAX(IF(document_date <= CURRENT_DATE(), document_date, NULL)), MAX(document_date) FROM `aquatiq-datavarehus.visma_gold_finance.fact_customer_invoice` UNION ALL SELECT 'BI_SalesOrder', MAX(IF(date <= CURRENT_DATE(), date, NULL)), MAX(date) FROM `aquatiq-datavarehus.visma_datamart.BI_SalesOrder` UNION ALL SELECT 'fact_project_portfolio_line', MAX(IF(transaction_date <= CURRENT_DATE(), transaction_date, NULL)), MAX(transaction_date) FROM `aquatiq-datavarehus.visma_gold_finance.fact_project_portfolio_line`", "max_rows": 10}
```

3b. Newest ledger posting per company. A current company a week behind the others is worth a question:

```tool-call
warehouse_query
{"sql": "SELECT c.entity_name, MAX(IF(g.transaction_date <= CURRENT_DATE(), g.transaction_date, NULL)) AS newest_posting FROM `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger` g JOIN `aquatiq-datavarehus.visma_gold_common.dim_company` c ON c.tenant_id = g.tenant_id GROUP BY c.entity_name ORDER BY newest_posting DESC", "max_rows": 20}
```

## 4. Is the sync itself healthy

Endpoints whose last sync is more than 36 hours old, in the state database this server reads:

```tool-call
visma_sync_status
{"stale_hours": 36, "limit": 20}
```

Failed runs and their errors:

```tool-call
visma_sync_status
{"run_status": "failed", "limit": 10}
```

Read the result for what it is: a failed run is a fact, an empty list is "nothing found in this database", not
proof that everything is fine.

## How to state it

Close every time-sensitive answer with an "as of": the newest business date you saw, that the data is a snapshot of
2026-10-01/02 until the scheduler has run, and the company's last change if it matters ("through 2026-10-02;
Aquatiq Aps last changed 2026-10-01"). If the question needs the present moment (an invoice paid this morning, an
order just edited), read the live ERP instead (skill `live-erp-queries`).
