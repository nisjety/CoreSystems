# Decisions log

## 2026-04-29 — v2 scaffold

| # | Decision | Rationale |
|---|---|---|
| 1 | Rust core + Python tooling. No Go. | Hot path (HTTP, concurrency, state) → Rust. Data-engineering glue (graphql-core, bigquery client) → Python ecosystem fit. Two languages cover the surface; a third adds cost without ROI. |
| 2 | Full skeleton across phases 0–6 now. | Stable contracts (types, traits, configs) reduce later rework cost more than empty crates risk it. Stubs return `Phase N` errors so wiring is testable. |
| 3 | Separate Postgres database (`visma_v2`). | Clean rollback. v1 keeps running untouched. Cross-DB joins not needed during pilot. |
| 4 | Reuse Visma Connect OAuth scope. | Same `client_credentials` flow + `tenant_id` form param as v1. Branch on `Scope::Datamart` only if Phase 0 spike returns 401. |
| 5 | Pilot tenant: Aquatiq AS only (`33f319b9-…`). | 30-day clean window before any second tenant. Avoids `tenant_id`↔`companyId` bridge complexity. |
| 6 | Cutover unit: flag-per-endpoint. | `config/endpoints/*.json:datamart.tenant_overrides` per tenant. Net-new DM-only endpoints allowed where DM exposes data REST doesn't. |
| 7 | Bronze additive columns: hold until Phase 8b. | "Wait until perfect" — don't widen bronze contract until pilot is 30-day clean. Normalizers preserve byte-stable legacy shape. |
| 8 | Tier 1 hourly endpoints stay on REST. | DM `updatedInDataMartDateTime` has lag; unfit for live operational reads. |

## 2026-04-30 — Phases 7–11

| # | Decision | Rationale |
|---|---|---|
| 9  | Expense API as a third `Source` variant. | Separate OAuth scope, separate rate-limit quota, separate watermark field (`approvedDate`). Modelled identically to ERP REST — same `FetchAdapter` trait, no shared code. |
| 10 | `enrich()` stamps `_source` + `_synced_at` in Rust before the Python subprocess runs. | Guarantees lineage even if the normalizer rewrites the payload. Rust-stamped values always win; Python `_ORCHESTRATOR_FIELDS` re-stamps over any normalizer output. |
| 11 | Config validation split: JSON Schema for structure, Python for invariants. | JSON Schema Draft-7 cannot express cross-field rules (e.g. Invariant #4: tier=1 ∧ datamart.ready=true is illegal). Python post-validator covers what schema cannot. |
| 12 | Scheduler uses three `tokio::time::interval` streams in a `select!` loop, not a cron library. | No external scheduler dependency. `MissedTickBehavior::Skip` prevents burst catch-up after a stall. Shutdown on `tokio::signal::ctrl_c()`. |

## 2026-05-01 — Tenant-scoped BQ identity

| # | Decision | Rationale |
|---|---|---|
| 13 | `enrich()` stamps `tenant_id`, and live BQ MERGE uses `tenant_id` plus the endpoint primary key. | Visma natural keys such as account number, customer number, order number, and reference number are tenant-local. Without `tenant_id`, multi-tenant bronze tables can overwrite rows across tenants. |
| 14 | DataMart normalizer validation is enforced only when a route is enabled. | Many configs contain future DataMart placeholders. Keeping them inactive is safe, but enabling one without a registered normalizer must fail before runtime to avoid field loss or write failure. |
| 15 | Sync checkpoints advance only after validation and write success. Fetches replay overlap windows, while stored watermarks remain monotonic. | Overlap prevents late-arriving rows from being missed; monotonic checkpoints prevent empty/old overlap results from erasing progress. |
| 16 | Live MERGE writes append-only raw stage rows and quarantines invalid rows. | Staging gives replay/audit evidence; quarantine prevents bad data from entering live bronze while preserving the rejected payload for repair. |

## Open

- **DataMart OAuth access** — `VISMA_OAUTH_SCOPE_DATAMART` rejected with `invalid_scope` as of 2026-04-30. Must be provisioned in Visma Developer Portal before any DM cutover.
- **Reconciliation tolerances** — 0% PK / 1% numeric / 0.1% string agreed in design; not yet tested under real drift.
- **Pilot batch size** — first DM flip deferred to Phase 12 (shadow validation). One endpoint at a time.
