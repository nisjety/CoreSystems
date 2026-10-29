# visma_service_v2 — Architecture

## Goal

Layered, source-aware sync platform. ERP REST stays for hourly ops + write-back. DataMart GraphQL adds bulk warehouse reads per-flag. Expense API is a parallel domain.

## Status

**Phase 11 complete.** Continuous scheduler running. No endpoint has flipped to DataMart yet — DataMart OAuth access not yet provisioned. All 74+ endpoints sync via ERP REST.

## Module map

```
core/crates/
  vsv2-common         shared types, errors, Source enum, TenantId, AppConfig
  vsv2-auth           Visma Connect OAuth client, scope routing, token cache
  vsv2-ratelimit      per-Source token bucket + Redis cooldown
  vsv2-state          Postgres + Redis: checkpoints, leases, run history,
                      tenant_company_map, endpoint_flags
  vsv2-erp-rest       ERP REST client (api.finance.visma.net)
  vsv2-datamart       GraphQL client (datamart.visma.net), cursor paginator,
                      query loader, company filter
  vsv2-expense        Expense Public API client
  vsv2-orchestrator   registry + dispatch + executor + bq_writer + shadow writer
  vsv2-reconcile      weekly soft-delete reconciliation
  vsv2-cli            `vsv2` operator binary (sync, schedule, reconcile, flag, …)

tooling/src/vsv2_tooling/
  discovery/          Phase 0 — introspect, companies, coverage, probe
  normalizers/        DM record → bronze legacy shape (REGISTRY)
  bigquery/           chunked MERGE loader, shadow diff, _apply_normalizer
  dataform/           bronze schema assertions (CI guard)
  config_validate.py  JSON Schema + Invariant #4 enforcement
```

## Data flow per sync run

```
operator                                           BigQuery
    │                                                 ▲
    ▼                                                 │
vsv2-cli sync --tenant T --endpoint E                 │
    │                                                 │
    ▼                                                 │
SyncExecutor.sync_one(T, E)                           │
    │                                                 │
    ├── EndpointRegistry.get(E)                       │
    │       └── EndpointDefinition.effective_source(T)│
    │                                                 │
    ├── DispatchTable.pick(source)                    │
    │       ├── Source::ErpRest         → erp adapter │
    │       ├── Source::DatamartGraphql → dm adapter  │
    │       └── Source::ExpenseApi      → expense     │
    │                                                 │
    ├── adapter.fetch(T, def, watermark, cursor)      │
    │       ├── OAuthClient.token_for(T, scope)       │
    │       ├── Limiter.acquire()                     │
    │       └── HTTP / GraphQL → Vec<Value>           │
    │                                                 │
    ├── enrich() — stamps tenant_id + lineage         │
    │                                                 │
    ├── validate_records()                            │
    │       └── invalid/conflicting rows → quarantine │
    │                                                 │
    ├── BqWriter.merge / write_shadow                 │
    │       └── Python subprocess (vsv2-bq-merge)     │
    │               ├── _apply_normalizer (optional)  │
    │               ├── append __vsv2_raw_stage       │
    │               └── tenant-scoped MERGE           │
    │                                                 │
    └── StateStore.upsert_checkpoint ─────────────────┘
```

## Scheduler (Phase 11)

`vsv2 schedule --tenant T` runs three tokio intervals in a `select!` loop:

| Tier | Endpoints | Default interval |
|---|---|---|
| 1 | Hourly operational (ERP REST only) | 3600 s (`VSV2_SCHEDULE_TIER1_SECS`) |
| 2/3 | Daily sync | 86400 s (`VSV2_SCHEDULE_TIER2_SECS`) |
| 4/5 | Weekly reference data | 604800 s (`VSV2_SCHEDULE_TIER4_SECS`) |

Graceful shutdown on Ctrl-C. `MissedTickBehavior::Skip` prevents burst catch-up.

## Key invariants

1. **Bronze schema is byte-stable through pilot.** Normalizers fill every legacy field; DM-only fields stay null until a formal schema widening.
2. **Composite checkpoint key** `(tenant_id, endpoint, source)` lets REST and DM coexist on the same endpoint without watermark collision.
3. **DataMart production gate.** Tenant overrides and database flags can request DataMart, but normal sync stays on the non-DataMart fallback while `ready=false`. Setting `ready=true` currently affects all tenants for that endpoint; pilot-only production cutover is not yet implemented.
4. **Hourly Tier 1 endpoints stay on REST.** DataMart `updatedInDataMartDateTime` has lag — unsuitable for live ops. Enforced by `config_validate.py`.
5. **Soft delete only.** Phase 5 reconciliation marks `_deleted_at`; never hard-deletes.
6. **Tenant and lineage columns always present.** `tenant_id`, `_source`, and `_synced_at` (RFC-3339 UTC) are stamped by Rust's `enrich()` before any Python normalizer runs. Normalizers cannot strip them.
7. **BigQuery identity is tenant-scoped.** Live MERGE uses `tenant_id` plus the endpoint primary key. Delete reconciliation filters by tenant before marking rows deleted.
8. **Checkpoints are monotonic.** Sync fetches replay a safe overlap, but stored watermarks never move backward and are advanced only after validated writes succeed.
9. **Invalid rows do not enter live bronze.** Missing keys, missing validation fields, and conflicting duplicate keys are quarantined for inspection.

## Phase → code map

| Phase | Description | Crate / Module |
|---|---|---|
| 0  Spike            | Discovery, introspection, coverage matrix | `tooling/.../discovery/` |
| 1  Foundation       | OAuth, clients, rate limiter, cursor paginator | `vsv2-auth`, `vsv2-datamart`, `vsv2-ratelimit`, `vsv2-erp-rest` |
| 2  Config schema    | Endpoint JSON registry, primary key, source | `config/endpoints/*.json`, `vsv2-orchestrator::registry` |
| 3  Dispatch         | State store, checkpoint, run history | `vsv2-orchestrator::dispatch`, `vsv2-state` |
| 4  Pilot endpoints  | BQ MERGE + shadow write path | `vsv2-orchestrator::bq_writer`, `vsv2-orchestrator::shadow` |
| 5  Delete recon     | Soft-delete reconciliation (weekly) | `vsv2-reconcile` |
| 6  Source-of-truth  | Architectural contract doc | `docs/source_of_truth.md` |
| 7  Expense API      | Third source domain, paginator, adapter | `vsv2-expense`, `vsv2-orchestrator::expense_adapter` |
| 8  Record enrichment | `tenant_id`, `_source`, and `_synced_at` stamped in Rust | `vsv2-orchestrator::executor::enrich()` |
| 9  Config hardening | JSON Schema + Invariant #4 validator | `tooling/config_validate.py`, `config/endpoints/_schema.json` |
| 10 Normalizer wiring | DM→bronze shape, lineage preservation | `tooling/.../normalizers/`, `vsv2-orchestrator::bq_writer` |
| 11 Scheduler        | Cron-based sync loop, graceful shutdown | `vsv2-cli::Schedule` |
| 12 Sync correctness | raw staging, quarantine, duplicate detection, overlap watermarks | `vsv2-orchestrator::validation`, `tooling/.../bigquery/loader.py` |
