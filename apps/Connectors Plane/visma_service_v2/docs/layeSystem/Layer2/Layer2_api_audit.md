# Layer 2 - Visma API Read Layer Audit

> Checked and updated on 2026-05-01 against:
> - Finance API OpenAPI: `https://api.finance.visma.net/API-index/api/vismanet.erp.service.api-apim`
> - SalesOrder V3 Swagger: `https://salesorder.visma.net/swagger/v3/swagger.json`
> - JournalTx Swagger: `https://journaltx.visma.net/swagger/v1/swagger.json`
> - Visma pagination, SalesOrder V3, and rate-limit documentation
> - SalesOrder V1/V2 EOL notice and Finance base URL migration notice

## Verdict

Layer 2 is now structurally correct for read-only Visma retrieval.

The endpoint registry no longer depends on legacy logical method names for
runtime HTTP routing. Every `source=erp_rest` endpoint now declares:

- `api_endpoint`
- `pagination_profile`
- `primary_key`
- `source`
- merge/sync metadata

The remaining work is not basic endpoint wiring. The first tenant-specific
partition planner is implemented for the Finance endpoints that require
tenant-derived query dimensions, and checkpoint state is now partition-aware.
Durable response/page metadata is now persisted for runtime observability.

## Current Implementation Status

| Area | Status |
|---|---|
| Finance base URL | Correct: `https://api.finance.visma.net` |
| Finance endpoint paths | Fixed: 72/72 active `erp_rest` configs have Swagger-matched paths |
| SalesOrder V3 host | Fixed: absolute `https://salesorder.visma.net/api/v3/SalesOrders` supported |
| SalesOrder V1/V2 EOL | Fixed: `/v2/salesorder` config removed |
| Journal transaction reads | Fixed: uses Finance `/v2/journaltransaction`; JournalTx voucher API remains Layer 3 |
| Pagination profiles | Fixed: `finance_page_number`, `salesorder_page_index`, `journaltx_page_index`, `none` |
| Endpoint-specific page size | Fixed: `page_size` config supported; SalesOrder V3 uses `15` |
| Background API | Fixed: configured via `background_mode`; GL transactions use background polling |
| Query metadata | Fixed: `default_query` and `required_query` supported |
| Tenant-specific partitioning | Fixed: budget and GL transaction slices fan out from tenant ledgers, branches, and financial periods |
| Durable per-partition checkpoints | Fixed: checkpoint key is `tenant_id + endpoint + source + partition_key` |
| Response/page metadata | Fixed: page metrics are persisted per run, tenant, endpoint, source, and partition |
| 400 handling | Fixed: HTTP 400 is no longer treated as empty data |
| 429/rate-limit handling | Fixed: `Retry-After` and `X-RateLimit-*` headers are observed |
| Tenant-scoped uniqueness | Fixed: BQ rows carry `tenant_id`; MERGE keys are `tenant_id` + endpoint primary key |
| Raw staging | Fixed: live MERGE writes append-only raw payload rows with payload hashes to `__vsv2_raw_stage` |
| Row validation/quarantine | Fixed: missing keys, missing validation fields, and conflicting duplicate keys stop the run and write to `__vsv2_quarantine` |
| Overlap watermarks | Fixed: fetches replay a safe overlap window and checkpoints never move backward |
| Config validation | Fixed: validates endpoint name, `api_endpoint`, profile, DataMart query files, and active DataMart normalizers |
| Swagger path check | Verified: configured ERP/SalesOrder paths match current specs |

## Runtime Changes Made

### 1. Endpoint Config Contract

`config/endpoints/_schema.json` now supports:

- `enabled`
- `api_endpoint`
- `pagination_profile`
- `page_size`
- `background_mode`
- `background_poll_secs`
- `background_timeout_secs`
- `default_query`
- `required_query`
- `partitioning`

`source=erp_rest` configs are semantically validated by
`tooling/src/vsv2_tooling/config_validate.py`.

### 2. Multi-Host Read Support

`core/crates/vsv2-erp-rest/src/client.rs` now accepts:

- relative Finance paths, for example `/v1/customer`
- absolute adjacent-service paths, for example
  `https://salesorder.visma.net/api/v3/SalesOrders`

This keeps Layer 2 read-only while allowing SalesOrder V3 reads without a
separate client for this phase.

### 3. Profile-Aware Pagination

`core/crates/vsv2-erp-rest/src/paginate.rs` now supports:

| Profile | Page parameter | First page | Watermark parameter | Response shape |
|---|---:|---:|---|---|
| `finance_page_number` | `pageNumber` | 1 | `lastModifiedDateTime` + condition | array or metadata wrapper |
| `salesorder_page_index` | `pageIndex` | 0 | `modifiedSince` | `value` + `nextPage` + `totalCount` |
| `journaltx_page_index` | `pageIndex` | 0 | `modifiedSince` | wrapper/array |
| `none` | none | n/a | none | single non-paged request |

### 4. Endpoint Registry Cleanup

All ERP REST configs now have executable paths. Examples:

| Endpoint | Path |
|---|---|
| `accounts` | `/v1/account` |
| `customers` | `/v1/customer` |
| `customer_invoices` | `/v1/customerinvoice` |
| `supplier_invoices` | `/v1/supplierInvoice` |
| `journal_transactions` | `/v2/journaltransaction` |
| `general_ledger_transactions` | `/v1/GeneralLedgerTransactions` |
| `inventory_items` | `/v1/inventory` |
| `sales_orders_v3` | `https://salesorder.visma.net/api/v3/SalesOrders` |

`sales_orders_v2` has been removed:

- reason: SalesOrder V1/V2 endpoints are on Visma's EOL/throttling path
- replacement: `sales_orders_v3`
- validation rejects reintroduced `/v2/salesorder` configs

The `sales_orders_v3.json` registry bug was fixed:

- before: `endpoint_name = "sales_orders"`
- after: `endpoint_name = "sales_orders_v3"`

### 5. Data-Retrieval Defaults

Endpoint-specific query defaults were added where the Swagger contract and
configured validation fields require them:

- `general_ledger_transactions`
  - `ledger=ACTUAL`
  - `expandAccountInfo=true`
  - `expandBranchInfo=true`
  - `includeTransactionBalance=true`
  - `page_size=100`
  - `background_mode=none`
- `inventory_items`
  - `expandCrossReference=true`
  - `expandAttribute=true`
  - `expandWarehouseDetail=true`
  - `expandAccountInformation=true`
  - `expandInventoryUnits=true`
  - `expandSalesCategories=true`
- `sales_orders_v3`
  - `expand=Customer`
  - `expand=FinancialInformation`
  - `expand=LinesExcludingAllocations`
  - `page_size=15`

These defaults favor complete operational reads without using blanket
`expand=All`.

### 6. Tenant-Specific Partition Planning

Finance endpoints with required tenant-specific parameters now declare a
`partitioning` block. The ERP adapter discovers dimension values from tenant
source endpoints and runs the target endpoint once per stable partition.

Implemented partitioned endpoints:

- `budgets`
  - `branch` from `/v1/branch:number`
  - `ledger` from `/v1/ledger:number`
  - `financialYear` from `/v1/financialPeriod:year`
- `general_ledger_transactions`
  - `ledger` from `/v1/ledger:number`

Every partition uses stable query filters and page size for all pages. Runtime
rows receive `_partition_key`, and partitioned MERGE identity includes
`tenant_id`, `_partition_key`, and the endpoint primary key. This prevents
collisions when the same Visma natural key appears in more than one ledger,
branch, or year slice.

Config validation now requires every `required_query` parameter to be supplied
by either `default_query` or `partitioning.dimensions`.

### 7. Durable Per-Partition Checkpoints

Checkpoint persistence is now partition-aware. The checkpoint table stores:

```text
tenant_id + endpoint + source + partition_key
```

The executor asks each adapter for runtime partitions, loads the matching
checkpoint before fetching that partition, applies the endpoint overlap window
per partition, and advances each partition checkpoint only after validation and
BigQuery write success. This prevents one ledger, branch, or year slice from
forcing replay or checkpoint advancement for every other slice.
Partition checkpoint updates are written in one state-store transaction.

Existing non-partitioned endpoints use `partition_key=default`.

### 8. Durable Response/Page Metadata

The executor persists page-level runtime metadata to `vsv2.sync_page_metric`.
Each row is keyed by runtime context:

```text
run_id + tenant_id + endpoint + source + partition_key
```

Captured metadata includes:

- upstream path or DataMart root field
- page parameter and page value
- page size
- response status
- elapsed milliseconds
- records returned
- upstream total count when provided
- next-page signal when provided
- `X-RateLimit-*` and `Retry-After` values
- background mode for ERP REST background calls

Successful pages are recorded with the real upstream response metadata.
Rate-limited and non-success upstream fetch failures also emit a failed-page
metric with status and retry context when available.

This gives Layer 2 durable evidence for tuning page sizes, detecting slow
endpoints, observing quota pressure, and comparing partition behavior between
tenants.

### 9. Tenant-Scoped Storage Identity

Runtime records are now enriched with:

- `tenant_id`
- `_source`
- `_synced_at`

The BigQuery MERGE key is no longer only the endpoint `primary_key`; it is
`tenant_id` plus the endpoint primary key. This prevents collisions between
tenants that share natural ERP identifiers such as account numbers, customer
numbers, order numbers, or reference numbers.

Delete reconciliation is also tenant-filtered, so a reconciliation run for one
tenant cannot soft-delete another tenant's rows.

### 10. DTO / Normalizer Safety

ERP REST records are currently stored as raw JSON values after endpoint-specific
pagination and extraction, so no REST response fields are intentionally dropped
by a DTO layer.

DataMart uses endpoint normalizers only when a DataMart route is enabled. The
registered normalizers currently cover the pilot batch:

- `normalize_account`
- `normalize_customer_with_locations`
- `normalize_location`

Config validation now blocks enabling a DataMart endpoint when its configured
`join_normalizer` is not registered. This keeps inactive placeholder configs
safe while preventing a cutover that would lose fields or fail at write time.

### 11. Sync Correctness Guardrails

Layer 2 live writes now follow an idempotent, replayable flow:

1. Fetch with a watermark overlap instead of resuming exactly at the previous
   checkpoint.
2. Enrich every record with `tenant_id`, `_source`, and `_synced_at`.
3. Validate tenant-scoped primary keys and endpoint `validation_fields`.
4. Drop exact duplicate rows inside the same run.
5. Reject conflicting duplicate keys with different payloads.
6. Write rejected rows to `__vsv2_quarantine`.
7. Write append-only raw payload audit rows to `__vsv2_raw_stage`.
8. MERGE only validated rows into the live bronze table.
9. Advance the partition checkpoint only after the validated write succeeds.

Checkpoint safety changes:

- normal incremental endpoints replay a 10-minute overlap
- finance/journal/transaction endpoints replay a 24-hour overlap
- empty or old overlap results do not clear or move back existing checkpoints
- partitioned endpoints resume independently per `_partition_key`
- DataMart `lastId` is treated as page-local and is no longer persisted across
  runs

## Remaining Gaps Before Calling Layer 2 "Perfect"

### 1. OAuth Scope Split

Layer 2 should use only the read/service scope:

```text
vismanet_erp_service_api:read
```

Interactive/write scopes belong to Layer 3.

### 2. DataMart DTO Coverage Beyond Pilot

The active DataMart-safe DTO/normalizer set is still limited to accounts,
customers, and locations. More endpoint configs contain future normalizer names,
but those routes are not enabled. Before enabling any additional DataMart route,
generate or implement the endpoint-specific query and normalizer from the
DataMart schema and verify shadow output against ERP REST.

### 3. Full ERP Coverage Matrix

The Finance OpenAPI currently exposes 393 paths. This service has 73 endpoint
configs total, of which 72 are ERP REST and 1 is Expense.

That is enough for the current BI/source-of-truth model, but it is not yet a
complete "all Visma ERP paths" implementation. Full compatibility requires a
generated path decision matrix:

- sync now
- sync later
- Layer 3 write/action endpoint
- unsupported
- intentionally skipped

### 4. SalesOrder Detail Strategy

Layer 2 now uses SalesOrder V3 with targeted expands. If BI needs allocations,
attachments, discounts, tax details, payments, or shipment details, add those
as explicit endpoint configs or targeted expand profiles. Do not switch to
`expand=All` by default.

### 5. Performance Optimizations

The first performance pass is complete and documented in:

```text
docs/layeSystem/Layer2/Layer2_visma_performance_plan.md
```

Completed:

- dynamic capture of `X-RateLimit-*` headers
- Redis cooldown persistence on `429`
- endpoint-specific `page_size`
- background API support for v1/v2 Finance calls that run longer than 4 minutes
- tenant-specific runtime partition planning for budget and GL transaction
  slices
- durable per-partition checkpoints
- durable response/page metadata storage

### 6. JournalTx Boundary

JournalTx `/api/v1/vouchers...` endpoints are still intentionally outside
Layer 2. They are voucher/import workflows and belong in Layer 3 unless a
specific read-only audit use case is added.

## Verification

Commands run:

```bash
cargo fmt
cargo check
cargo test
uv run python -m vsv2_tooling.config_validate
uv run --extra dev pytest
```

Additional live-spec check:

- all 72 configured ERP REST paths matched the Finance/SalesOrder Swagger path sets
