---
id: honest-results
title: Reading results honestly
description: What the completeness, truncation and result-handle fields mean, and the rules for reporting partial, empty or failed results without overstating them.
tags: [meta, safety, pagination, results]
tools: [visma_read_result, visma_erp_query, visma_salesorder_get, visma_datamart_query, visma_sync_status, warehouse_query, warehouse_overview]
primary: [visma_read_result]
prerequisites: [start-here]
difficulty: beginner
steps: 3
---
# Reading results honestly

A wrong "all", "none" or "zero" is worse than no answer. These are the signals this server gives you and what
each one allows you to say.

## Every read carries `meta`

`meta.source` (`erp_rest`, `salesorder_v3`, `datamart_graphql`, `bigquery`, `sync_state_db`, `local_config`),
`meta.fetched_at`, `meta.tenant_id`, and `meta.coverage`:

| `coverage` | You may say |
|---|---|
| `single_page` | "the first N rows" (page, `page_size`, `maybe_more` tell you whether there is another page) |
| `single_record` | "this record, as of `fetched_at`" |
| `single_response` | "the answer to this one query", never "all" |
| `metadata_only` | "sizes and times", never anything about the content |

`meta.complete_collection` is `false` on live reads, always. Only a loop over every page can make a collection complete.

## Signals by tool

- **`visma_erp_query`**: `row_count`, `page`, `page_size`, `maybe_more` (true when the page was full or the API
  returned a `nextPage`). The size of the whole collection is `metadata.totalCount` on a Finance row and a
  top-level `totalCount` for SalesOrder: quote it as "total N, of which I read M". After filters, `filter_check`
  says whether they did anything: `no_effect_detected` means the page is **not** filtered (Visma ignores filter
  names it does not know), so do not call it filtered (skill `live-erp-queries`).
- **`warehouse_query`**: `truncated: true` means more rows matched than `max_rows`; the rows you have are the
  first ones in your `ORDER BY`. `bytes_scanned_estimate` is the cost. An aggregate query is complete by
  construction, a row listing is not.
- **`result_truncated: true`** with a `result_id` is *not an error*: the answer was too large to return at once
  and is parked for a few minutes. Page through it with `visma_read_result` (pointer `/rows`, then follow
  `next_offset` until it is null) before you summarise, or narrow the original query.
- **`visma_sync_status`**: reflects the sync state database this server is connected to, which is not the same
  thing as how fresh the warehouse is (skill `data-freshness`).

## Classify before you conclude

Use these words in your answer, exactly once per finding:

- `complete`: every page or an aggregate over the full filter, and the filter is the one the user asked for.
- `partial`: pages remain or the result was truncated; say how much you read.
- `sample`: you deliberately looked at a few rows to check a shape.
- `unknown`: the call failed, was refused, or you did not check. **Never present unknown as zero.**
- `blocked`: access was refused (see skill `error-triage`).

## Rules

1. Never say "all", "every" or "no X exist" unless the result is `complete` for that exact filter.
2. An empty result is a *question*, not an answer. Before telling the user nothing matches, check the usual
   causes: wrong period format (`'202509'`, a string), wrong company, a status filter that excludes the rows,
   and whether the data is fresh enough to contain them.
3. A failed call is `unknown`. Retry once only if the error says it is transient; otherwise report it.
4. Name the source: warehouse numbers lag the live ERP, and the two can legitimately differ.
5. Quote the basis of every figure: company, period, currency, sign convention.

## Worked examples

Page 2 of a live list (pages are 1-based for every endpoint):

```tool-call
visma_erp_query
{"endpoint": "customers", "page": 2, "page_size": 50}
```

Read part of a parked result (use the `result_id` from the truncated answer):

```tool-call
visma_read_result
{"result_id": "0123456789abcdef0123456789abcdef", "pointer": "/rows", "offset": 0, "limit": 100}
```

An aggregate is complete; a listing is bounded. Count first, then list:

```tool-call
warehouse_query
{"sql": "SELECT COUNT(*) AS open_invoices FROM `aquatiq-datavarehus.visma_gold_finance.fact_customer_invoice` WHERE status = 'Open'"}
```
