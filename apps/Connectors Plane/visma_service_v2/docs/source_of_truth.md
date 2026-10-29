# Source of truth — v2

> **Status:** Locked. This file is the canonical reference for *which system
> answers which question* in v2. It does not change runtime behaviour — it
> locks the contract the orchestrator implements.
>
> Last reviewed: Phase 11 complete (2026-04-30). No endpoints have flipped to
> DataMart yet — DM OAuth access is not yet provisioned.

Cross-refs: [DECISIONS.md](DECISIONS.md), [architecture.md](architecture.md),
[Layer 2 API audit](layeSystem/Layer2/Layer2_api_audit.md).

## 1. Quick lookup

| Use | Source | Notes |
|---|---|---|
| Default warehouse read (per migrated endpoint, Aquatiq AS) | DataMart | flag-per-endpoint |
| Hourly Tier 1 (sales_orders, shipments, invoices, etc.) | ERP REST | DM lag breaks them |
| Manual full resync | ERP REST | `fallback_source` |
| Field gaps (e.g. price class fallback) | Finance Service REST | targeted per field |
| Delete reconciliation | ERP REST | weekly ID-only scan |
| Write-back (sales orders create, vouchers) | ERP REST + journaltx | DM is read-only |
| socrm_service operational reads | ERP REST V3 | unchanged |

The table above is the contract. Sections 2–7 explain *why* each row exists
and *how* the orchestrator routes accordingly.

## 2. ERP REST — primary roles

ERP REST (`vismanet.erp.service`, V3) is **never fully replaced**. It owns:

1. **Tier 1 hourly endpoints.** sales_orders, shipments, invoices,
   customer_payments, cash_transactions, journal_transactions. DataMart lag
   (typically 15–60 min, occasionally hours) breaks downstream SLAs, so
   these stay REST regardless of DM coverage. Enforced by Invariant #4.
2. **Write-back.** All mutations (create sales order, post voucher,
   journaltx) go to REST. DataMart is read-only by definition.
3. **OAuth + tenant context.** The shared `vsv2-auth` token manager talks
   to Visma.net OAuth; both REST and DataMart calls use the same token
   audience (see [DECISIONS.md](DECISIONS.md) — verification still open).
4. **Manual resync / backfill.** When an endpoint's DM flag is off, or an
   operator forces a resync, the orchestrator routes to REST via the
   `fallback_source` config field.
5. **Field gaps.** Endpoints where DM omits or stales a field (e.g. price
   class, certain customer attributes) call Finance Service REST for the
   missing field only — DM remains primary for the rest of the row.
6. **Delete reconciliation.** Weekly ID-only scan against REST detects
   rows missing from our state and soft-deletes them locally
   (Invariant #5). DataMart cannot be trusted for deletes.
7. **socrm_service operational reads.** Unchanged from v1 — V3 REST only.

## 3. DataMart — primary roles

DataMart is the intended bulk-read fast path, but no endpoint has passed the
production cutover gate yet. It is being evaluated for the pilot tenant
(`AQUATIQ_AS_TENANT` = `33f319b9-4d57-11ee-960c-025417856183`). It owns:

1. **Future warehouse read** for endpoints that pass shadow parity and an
   explicit per-tenant production gate. The current `datamart.ready=true`
   setting is global for an endpoint, not pilot-tenant-only.
2. **High-volume historical pulls.** Tier 2/3/4 endpoints where DM lag
   is acceptable (≥1h tolerance).
3. **Read-only.** No writes, ever. Mutations go to REST.

As of 2026-09-23, all endpoint JSONs have `datamart.ready=false`, and the
pilot overrides for `budgets` and `general_ledger_balances_v2` are disabled.
No endpoint has flipped. A normal sync resolves an unready DataMart request to
its configured non-DataMart fallback, including database flag requests.

## 4. Decision matrix per endpoint category

| Category | Read source | Write source | Reconcile source |
|---|---|---|---|
| Tier 1 operational (hourly) | REST | REST | REST (weekly ID scan) |
| Tier 2 daily | DM if `ready=true` else REST | REST | REST |
| Tier 3 weekly | DM if `ready=true` else REST | n/a | REST |
| Tier 4 financials | DM if `ready=true` else REST | REST (journaltx) | REST |
| Tier 5 reference | DM if `ready=true` else REST | REST | REST |
| Expense (Phase 7) | REST | REST | REST |
| socrm_service V3 | REST | REST | REST |

Routing is decided by `vsv2-orchestrator` from endpoint config and an optional
database source flag. `datamart.ready=true` currently selects DataMart for all
tenants without an explicit database override; it is not a safe pilot-only
switch. Per-tenant production promotion remains to be implemented.

## 5. Watermark + checkpoint behaviour

Composite checkpoint key (Invariant #2): `(tenant_id, endpoint, source)`.

- A flip from REST→DM creates a **new checkpoint row**, not a mutation
  of the existing one. Bronze remains byte-stable (Invariant #1).
- Flipping back (DM→REST fallback) reuses the original REST checkpoint —
  no data loss, no replay window.
- Operators can therefore A/B a source per endpoint without touching
  state semantics.

## 6. Invariants this document depends on

Repeated from [architecture.md](architecture.md) for clarity. Phase 6
does not modify them.

1. Bronze layer is byte-stable per `(tenant, endpoint, source, key)`.
2. Checkpoint key is composite: `(tenant_id, endpoint, source)`.
3. Per-tenant, per-endpoint DataMart cutover is the target. The current global
   `ready` flag must not be enabled until a safe promotion gate is implemented.
4. Tier 1 endpoints stay on REST regardless of DM coverage.
5. Deletes are soft, reconciled weekly via REST ID-only scan.
6. BigQuery row identity is tenant-scoped: live MERGE uses `tenant_id` plus
   the endpoint primary key, and reconciliation filters by `tenant_id`.
7. Live MERGE is gated by validation, quarantine, append-only raw staging, and
   monotonic checkpoint advancement.

Invariants #4 and #5 are the **non-negotiable** REST roles. Invariants
#1–#3 govern how DM coexists with REST safely.

## 7. Open items (tracked in DECISIONS.md)

- **DataMart OAuth** — live introspection and samples succeeded in May 2026;
  recheck the deployed credentials before promotion.
- **Reconciliation tolerances** — 0% PK / 1% numeric / 0.1% string agreed; not yet tested under real drift.
- **First DM flip** — blocked on endpoint parity and a per-tenant production
  routing gate; shadow validation alone is not enough.

## 8. What changes when an endpoint flips to DataMart

The production flip is blocked. Before enabling one endpoint:

1. Prove business-key, field, and completeness parity for that tenant using
   the separate shadow comparison workflow.
2. Implement and test a per-tenant production-ready gate. Do not use the
   current global `datamart.ready=true` as a pilot-only switch.
3. Verify the DataMart checkpoint starts independently and the REST checkpoint
   remains available for rollback.
4. Keep Tier 1 endpoints, reconciliation, and all writes on REST.
