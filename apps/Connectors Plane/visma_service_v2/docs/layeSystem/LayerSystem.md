# Visma Server Layer System

This document defines the target layered architecture for `visma_service_v2`
and links to the current status of each layer.

## Layer Overview

| Layer | Name | Purpose | Current status | Detail |
|---|---|---|---|---|
| 1 | Datamart warehouse layer | Bulk read/reporting extraction from Visma Net DataMart GraphQL | Implemented; broad cutover blocked on parity and coverage | `docs/layeSystem/Layer1/layer1_datamart_audit.md` |
| 2 | ERP API read layer | Direct reads from Visma ERP, SalesOrder, Journal/transaction APIs, and related endpoints | Structurally fixed; observability metrics persisted | `docs/layeSystem/Layer2/Layer2_api_audit.md` |
| 3 | Interactive API layer | Transformations, mutations, write-back, voucher import, SalesOrder changes, JournalTx workflows | Foundation (2026-10-09) and three SalesOrders v3 operations (patch, create on hold, hold/release, 2026-10-14); no write has been run against a live tenant | `core/crates/vsv2-interactive/` |
| 4 | MCP/AI capability layer | AI tooling and MCP access over all lower layers | Implemented (stdio): live reads, warehouse SQL, offline API reference, 23 per-tool skills, gated write tools, and a per-person Visma sign-in (`vsv2-mcp login`, built and tested against a mock; not yet through the real consent screen) | `docs/layeSystem/Layer4/layer4_mcp.md`, `docs/layeSystem/Layer4/sign-in.md`, `core/crates/vsv2-mcp/`, `core/crates/vsv2-signin/` |

## Layer 1 — Datamart Warehouse

Layer 1 should be the preferred source for warehouse/reporting reads where
Datamart latency is acceptable.

Responsibilities:

- GraphQL extraction from `https://datamart.visma.net/graphql`.
- Schema-driven query generation.
- ETL activation checks.
- Natural/composite key handling.
- Shadow comparison before cutover.

Current status (Phase 11):

- Rust DataMart client, cursor paginator, and orchestrator adapter all implemented.
- Shadow writer and comparison tooling are available separately from normal
  live sync. Normal sync keeps using ERP while `datamart.ready=false`.
- Per-tenant, per-endpoint source override flag implemented (`endpoint_flags` table).
- Normalizers (DM→bronze shape) implemented for the pilot batch and wired through the BQ write path.
- Config validation blocks enabling a DataMart route with an unregistered normalizer.
- Live DataMart introspection and sample pulls succeeded in the May 2026 audit;
  current OAuth provisioning still needs rechecking before deployment.
- The pilot overrides for `budgets` and `general_ledger_balances_v2` are now
  disabled. An unready DataMart route cannot replace the live ERP sync, even
  when selected by a database source flag.
- No endpoint is currently cut over to DataMart.

Reference:

- `docs/layeSystem/Layer1/layer1_datamart_audit.md`
- `core/crates/vsv2-datamart/`
- `tooling/src/vsv2_tooling/discovery/`
- `config/queries/`

## Layer 2 — ERP API Read Layer

Layer 2 is the direct read layer for Visma ERP APIs. It is the fallback and
operational source when Datamart is unavailable, incomplete, or too delayed.

Responsibilities:

- Finance API reads from `https://api.finance.visma.net`.
- SalesOrder V3 reads from `https://salesorder.visma.net`.
- ERP journal transaction reads from Finance `/v2/journaltransaction`.
- Expense public/transaction API reads where relevant.
- Correct pagination, rate-limit, and required-query handling per API family.

Current status (Phase 12):

- ERP REST client, paginator, watermark, and rate limiter implemented.
- Multi-host read routing supports Finance relative paths and SalesOrder V3 absolute URLs.
- Deprecated `/v2/salesorder` config is removed; `sales_orders_v3` is the canonical Layer 2 sales-order endpoint.
- Per-endpoint pagination profiles implemented:
  `finance_page_number`, `salesorder_page_index`, `journaltx_page_index`, and `none`.
- Endpoint-specific `page_size` implemented; SalesOrder V3 uses smaller pages for expanded order data.
- Finance Background API support implemented; GL transactions use `erp-api-background:none` with polling.
- Dynamic rate-limit header observation implemented for `X-RateLimit-*` and `Retry-After`.
- `default_query` and `required_query` implemented for endpoint-specific Swagger requirements.
- Tenant-specific partition planning implemented for budget branch/ledger/year
  slices and GL transaction ledger slices.
- Durable per-partition checkpoints implemented with
  `tenant_id + endpoint + source + partition_key`.
- Durable page metrics implemented in `vsv2.sync_page_metric` for response
  status, elapsed time, page size, counts, rate-limit headers, and partition
  context.
- BQ writes are tenant-scoped: every row carries `tenant_id`, and MERGE uses
  `tenant_id` plus the endpoint primary key.
- Live writes are guarded by raw staging, validation, duplicate detection, and
  quarantine tables before checkpoint advancement.
- Incremental reads use watermark overlap and checkpoint monotonicity to avoid
  missing late-arriving rows.
- Delete reconciliation is tenant-filtered.
- HTTP 400 is no longer treated as empty data; upstream `429` now returns a typed cooldown error.
- Expense API client implemented as a separate source domain with dedicated rate limits.
- 90 endpoint configs (81 ERP REST, 1 Expense, 8 DataMart-native). `docs/endpoint_matrix.md` is generated from the configs and
  the live runs (`make endpoint-matrix`) and lists source, key, pagination, DataMart state and live status per endpoint.
  Every ERP REST config declares an `api_endpoint` and `pagination_profile` (enforced by `make validate-config`).
- Continuous scheduler running: tier1 hourly, tier2 daily, tier4 weekly.
- Remaining Layer 2 operational gap: dashboard/query views and alerting on top
  of persisted response/page metadata.
- Remaining Layer 1/2 cutover gap: 21 DataMart normalizers exist, but DataMart roots lack many ERP fields
  (suppliers: 22 of 35), so measured field parity (`scripts/normalizer_parity.py`), not normalizer count, decides
  what can flip. Several roots are still empty in DataMart (employees, inventoryItems, financialPeriods).
- Performance optimization plan lives in `docs/layeSystem/Layer2/Layer2_visma_performance_plan.md`.

Reference:

- `docs/layeSystem/Layer2/Layer2_api_audit.md`
- `docs/layeSystem/Layer2/Layer2_visma_performance_plan.md`
- `core/crates/vsv2-erp-rest/`
- `core/crates/vsv2-expense/`
- `config/endpoints/`

## Layer 3 — Interactive API Layer

Layer 3 will own workflows that change data or trigger ERP operations.

Examples:

- SalesOrder create/update/delete workflows.
- SalesOrder V3 line mutations requiring `If-Match` / ETag.
- JournalTx voucher import and attachment workflows.
- Customer invoice/payment release or action endpoints.
- Transformations that prepare data before write-back.

Current status (foundation, 2026-10-09):

- `vsv2-interactive` crate: operation registry, two-step `dry_run` -> `execute`, HMAC confirm
  token binding the reviewed plan, append-only audit (`vsv2.write_audit`, migration 0012),
  `HttpTransport` with its own `OAuthClient` and write scopes (`Scope::ErpWrite`,
  `Scope::SalesOrderWrite`), If-Match support and a precondition `Reader` for ETags.
- Guardrails are structural: no DELETE method exists, writes are deny-by-default per tenant AND
  operation (`VSV2_WRITE_ENABLED_TENANTS` / `_OPERATIONS`, no wildcard), plans expire (10 min),
  a plan executes at most once (a crashed attempt also blocks replay), and the audit row is
  written before the request is sent (no audit row, no write).
- Operation `salesorder.patch` (`operations/salesorder_patch.rs`): `PATCH /api/v3/SalesOrders/{type}/{orderId}`
  with the ETag from a fresh GET (never the `*****` bypass), field allow-list (status, removeLines,
  discounts, currency/tax, ROT/RUT etc. are deferred to their own operations), closed orders refused.
  Write scope `visma.net.erp.salesorder:write` confirmed from the public swagger.
- Operation `salesorder.create` (`operations/salesorder_create.rs`, 2026-10-14): `POST /api/v3/SalesOrders`, the body checked
  against the swagger's own request schema (`schemas/salesorder_v3_create.json`, unknown fields refused), the order always
  sent with `status: "Hold"`, the number series left to Visma.
- Operation `salesorder.set_status` (`operations/salesorder_status.rs`, 2026-10-14): `PATCH` with `{"status": "Hold"}` or
  `{"status": "Open"}` and the fresh ETag; only an `Open` order can be put on hold and only an order that is `On Hold`
  released; credit holds, back orders and closed orders are refused; cancel/complete are not offered.
- `PgAuditSink` + migration 0012 verified against a real Postgres (scratch DB).
- Hardened after review (2026-10-09): atomic execution claim (unique index), send + outcome row in a
  detached task (cancellation-safe), `OutcomeUnknown` for timeouts after send, token binds the summary,
  audit table blocks TRUNCATE.
- Not done: JournalTx voucher import and the Finance write scope (unconfirmed), a live-tenant
  dry-run check, and a CLI/HTTP surface. Layer 4 (MCP) calls `Executor` directly. The transports take a
  `vsv2_auth::Tokens` handle, so they run with the service credentials or, in the MCP, with a signed-in person's tokens.

## Layer 4 — MCP/AI Capability Layer

Layer 4 will expose controlled AI/MCP capabilities over Layers 1-3.

Responsibilities:

- Discover available data and operations.
- Explain lineage and sync status.
- Trigger approved read/sync/reconciliation actions.
- Assist with endpoint onboarding and validation.
- Add guardrails around write-capable workflows.

Current status (2026-10-09):

- `vsv2-mcp` (rmcp 3.5, stdio): warehouse (BigQuery) browse/describe/query with layered read-only
  enforcement, live Visma ERP/SalesOrder/DataMart reads, sync status, and gated write tools that
  call Layer 3. Every tool carries a skill list (19 tested usage guides, `visma_init` with known data
  issues, skill resources, `--print-manifest`). Details, safety model, configuration and tests:
  `docs/layeSystem/Layer4/layer4_mcp.md`; comparison with the Xero and official Visma MCPs:
  `docs/layeSystem/Layer4/parity.md`.
- Not done: remote (HTTP) transport with auth, JournalTx write operation, more warehouse sources.

## Global Rules

- Datamart is read-only.
- ERP REST remains the fallback and operational source.
- Writes and mutations must never route through Datamart.
- Each endpoint must declare its source, path, pagination profile, primary key,
  and required query parameters.
- No endpoint should be promoted until it passes source-specific validation and
  shadow comparison where applicable.
