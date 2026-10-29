# Layer 2 - Visma Performance Improvement Plan

> Created on 2026-05-01 from current Visma documentation:
> - Rate limits: `https://docs.vismasoftware.no/vismanetapi/ratelimits/`
> - Pagination: `https://docs.vismasoftware.no/vismanetapi/setting-up-your-integration/how-tos/pagination/`
> - Background API calls: `https://docs.vismasoftware.no/vismanetapi/setting-up-your-integration/how-tos/background/`
> - SalesOrder V3: `https://docs.vismasoftware.no/vismanetapi/how-tos/salesordersv3/`
> - SalesOrder V1/V2 EOL: `https://docs.vismasoftware.no/vismanetapi/end-of-life-notices/salesorder-v1-v2/`
> - Finance API base URL migration: `https://docs.vismasoftware.no/vismanetapi/end-of-life-notices/change-base-url/`

## Current Position

Layer 2 has correct basic endpoint wiring:

- Finance uses the new base URL: `https://api.finance.visma.net`.
- Sales order reads use SalesOrder Service API V3 as the active endpoint.
- Finance collection reads use `pageNumber` and `pageSize`.
- SalesOrder V3 reads use `pageIndex` and `pageSize`.
- The REST client handles upstream `429` with `Retry-After`.
- The REST client now retries transient ERP GET transport failures with short,
  bounded backoff before failing the run.

The first performance pass is implemented. Runtime tenant-specific partition
planning is also implemented for the first high-risk Finance endpoints.
Durable per-partition checkpoint storage is implemented. Remaining work is
primarily operational alerting and tuning from the persisted metrics.

## Completed Performance Items

### 1. Background API Optimization - Complete

Visma recommends background API calls for requests taking more than 4 minutes.
The mechanism uses the `erp-api-background` header and currently applies only
to v1/v2 Finance endpoints.

Implemented:

- endpoint config fields:
  - `background_mode`: `off`, `none`, or `subscription`
  - `background_poll_secs`
  - `background_timeout_secs`
- REST client sends `erp-api-background` for configured Finance v1/v2 endpoints
- REST client handles `202 Accepted`
- REST client polls `/v1/background/{requestId}`
- REST client fetches `/v1/background/{requestId}/content`
- config validation rejects background mode for non-Finance absolute URLs
- `general_ledger_transactions` is enabled with `background_mode=none`

Current configured candidate:

```text
general_ledger_transactions
background_mode=none
background_poll_secs=5
background_timeout_secs=1800
```

### 2. Dynamic Rate-Limit Optimization - Complete

Visma exposes current limits in response headers:

- `X-RateLimit-Limit`
- `X-RateLimit-Remaining`
- `X-RateLimit-Reset`
- `X-RateLimit-Policy`

Implemented:

- REST client captures and logs rate-limit headers from every response
- limiter observes upstream limit/remaining/reset/policy values
- when remaining quota reaches zero, limiter engages cooldown until reset
- upstream `429 Retry-After` now engages limiter cooldown before returning
  `RateLimited`
- sync/scheduler wiring passes the shared Redis pool into ERP REST, DataMart,
  and Expense limiters so cooldowns are process-shared

### 2a. Transient Transport Retry - Complete

Long-running backfills were still vulnerable to one-off transport send errors
from Finance and SalesOrder reads. Those failures surfaced as:

```text
HTTP error: erp_rest GET https://api.finance.visma.net/...: error sending request for url (...)
```

Implemented:

- ERP REST GET retries transient transport send failures twice before failing
  the request
- bounded backoff sequence: `250ms`, then `1s`
- applied to both standard GET reads and background follow-up GET reads
- warning logs now include tenant, URL, attempt number, and retry delay

### 2b. Persisted Async Control Plane - Complete

The HTTP control plane no longer holds `POST /v1/sync/backfill` open until the
full replay finishes.

Implemented:

- `POST /v1/sync/backfill` now returns `202 Accepted` immediately with:
  - `backfill_id`
  - `poll_url` (`GET /v1/backfills/{backfill_id}`)
  - `runs_url` (`GET /v1/runs?...`) for persisted endpoint-level follow-up
- `GET /v1/backfills/{backfill_id}` reports:
  - `pending`, `running`, `succeeded`, `failed`, or `partial_success`
  - created/started/finished timestamps
  - aggregate record and endpoint counts
  - failed endpoint summaries when present
- backfill handles are persisted in `vsv2.sync_operation`, so `GET /v1/backfills/{backfill_id}`
  survives server restarts
- `POST /v1/schedules/{schedule_id}/trigger` now returns `202 Accepted`
  immediately with:
  - `trigger_id`
  - `poll_url` (`GET /v1/schedules/{schedule_id}/triggers/{trigger_id}`)
  - `runs_url` (`GET /v1/runs?...`) for persisted endpoint-level follow-up
- `GET /v1/schedules/{schedule_id}/triggers/{trigger_id}` reports the same
  async operation status fields as backfills
- async backfill execution is no longer cut off by the HTTP request timeout
- startup now marks any `pending` or `running` async operation as `failed`
  with a restart message so clients never poll a handle that hangs forever
- only one async POST backfill is admitted at a time, and only one async
  schedule trigger per schedule is admitted at a time, to avoid overlapping
  control-plane replays

### 3. Endpoint-Specific Page Sizing - Complete

Visma documents that paging is mandatory for list endpoints and that
`pageSize` limits can vary by endpoint.

Implemented:

- optional `page_size` endpoint config field
- adapter uses endpoint `page_size` before the global adapter default
- keep filters, watermark, and page size stable across all pages in the same
  sync partition
- `sales_orders_v3` uses `page_size=15` because it expands order details/lines
- `general_ledger_transactions` explicitly uses `page_size=100`

Recommended initial values:

- Finance standard endpoints: `100`, then tune up only after observing
  endpoint metadata and response time
- SalesOrder V3 with lines expanded: `10` to `15` for large tenants
- Datamart remains separate and can keep larger GraphQL pages where tested

### 4. SalesOrder V3 Canonical Migration - Complete

Visma has placed SalesOrder V1/V2 endpoints on an EOL/throttling path and
recommends SalesOrder V3.

Implemented:

- `sales_orders_v3` is active and uses SalesOrder Service API V3
- `sales_orders_v2` config file was removed
- validation fails if `/v2/salesorder` is reintroduced
- registry regression test asserts `sales_orders_v2` is absent

Remaining downstream cleanup:

- delete or archive stale V2 BigQuery tables once consumers are moved

### 5. Partitioned Fetch Planning - Complete

Some endpoints require query parameters that are tenant-specific rather than
static.

Current examples:

- `/v1/budget` requires `branch`, `ledger`, and `financialYear`
- `/v1/GeneralLedgerTransactions` requires `ledger`

Implemented:

- `partitioning` endpoint config contract
- tenant dimension discovery from Finance source endpoints
- cartesian partition plans with `max_partitions` guard
- stable filters and page size across every page in a partition
- `_partition_key` on partitioned records
- partition-aware live MERGE identity:
  `tenant_id + _partition_key + endpoint primary key`
- config validation that requires every `required_query` to be supplied by
  either `default_query` or `partitioning.dimensions`

Current configured endpoints:

- `budgets`: branch × ledger × financialYear
- `general_ledger_transactions`: ledger

### 6. Durable Per-Partition Checkpoints - Complete

Runtime partition planning and checkpoint persistence are both partition-aware.
Checkpoints are keyed by:

```text
tenant_id + endpoint + source + partition_key
```

The executor plans runtime partitions, loads the matching checkpoint before each
partition fetch, applies overlap windows per partition, and only advances those
checkpoints after validation and write success. Non-partitioned endpoints use
`partition_key=default`. Partition checkpoint updates are written in one
state-store transaction.

### 7. Response Metadata Storage - Complete

Performance tuning needs observable data.

Implemented:

- `vsv2.sync_page_metric` stores one row per fetched page
- metrics are linked to `run_id`, tenant, endpoint, source, and partition
- persisted fields include response status, elapsed time, page parameter/value,
  page size, record count, total count, next-page signal, rate-limit headers,
  `Retry-After`, and ERP background mode
- upstream 429 and non-success fetch failures emit failed-page metrics with
  status and retry context when available
- indexes support tenant/endpoint inspection, run inspection, rate-limit
  pressure review, and slow-page review

Remaining operational follow-up:

- build dashboard/query views on top of `vsv2.sync_page_metric`
- add alerting for repeated 429s, repeated slow pages, and repeated empty pages
  on previously populated endpoints

## Implementation Status

| Item | Status |
|---|---|
| Remove deprecated SalesOrder V2 | Complete |
| Add Background API support | Complete |
| Capture and observe rate-limit headers | Complete |
| Engage cooldown from `429 Retry-After` | Complete |
| Add endpoint-level `page_size` | Complete |
| Add partition planning for budget and GL transaction slices | Complete |
| Store per-partition checkpoints | Complete |
| Store response metadata/metrics durably | Complete |
