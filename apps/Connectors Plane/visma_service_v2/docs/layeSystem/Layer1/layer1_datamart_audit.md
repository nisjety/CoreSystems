# Layer 1 - DataMart Audit

Initially checked on 2026-05-13 and refreshed on 2026-09-29 against the live DataMart schema at
`https://datamart.visma.net/graphql` and the official docs at
`https://docs.datamart.visma.net`.

## Status

Layer 1 is now structurally much stronger, but it is still not ready for a
broad production cutover.

### Docs and live check (2026-10-13)

- Live introspection was refreshed with `vsv2-discover introspect`: 80 types and 54 root fields. The repo snapshot in `core/queries/_generated/` now matches live. Drift since 2026-09-29 is additive only: the root `apHistoricalData` and new fields on the supplier and AP types, from Visma's [2026-10-02 API release](https://docs.datamart.visma.net/release-notes/2026-10-02-api). Nothing was removed, retyped or newly deprecated.
- All 119 query files under `config/queries/` validate against the live schema (graphql-core in the project venv). The schema check was skipped because the live schema has `__`-prefixed type names that the spec reserves.
- Visma's [2026-10-08 ETL release](https://docs.datamart.visma.net/release-notes/2026-10-08-etl) changes how incremental fetches start, and our DataMart route does not follow it yet. Visma asks for the previous run's start time minus one minute, sent as `updatedInDataMart` on every page, with the new start saved only after all pages are processed. Our route starts from the newest `updatedInDataMartDateTime` in the stored checkpoint minus 10 minutes (`core/crates/vsv2-orchestrator/src/executor.rs`: `checkpoint_watermark`, `overlap_watermark`, `derive_checkpoint_from_values`). Visma says that pattern can skip changes. The 38 enabled `dm_*` routes are in the daily tier and feed the `visma_dm_*` raw tables. Not changed in this pass: the fix needs a full reimport of those routes after deployment.
- Deletions: every `dm_*` route requests `includeDeleted`, because its variable defaults to `true`, so deleted records reach the raw tables. From 2026-10-19, Visma returns records deleted before 2026-10-08 once, so the first fetch after that date can be large.
- Deprecated items still used by enabled routes (Visma has not set removal dates): `dm_general_ledger_balances` (root `generalLedgerBalances`, replaced by `generalLedgerBalancesV2`, which `dm_general_ledger_balances_v2` already uses), `dm_sales_order_lines_v2` (16 fields), `dm_locations` (4), `dm_journal_transactions` (3), `dm_customers` (2) and `dm_supplier_documents` (2). Replacing them changes bronze column names.
- Fifteen live roots have no config, for example `apHistoricalData`, the EXPERIMENTAL `generalLedgerBalances…V2` roots and the by-id queries. Placeholder configs without a live type or query file were not reviewed in this pass.

### Source-routing hold (2026-09-23)

- The Aquatiq pilot overrides for `budgets` and
  `general_ledger_balances_v2` are disabled. Budget still lacks a live sample;
  GL balances still lack key overlap in the recorded ERP/DataMart sample.
- A normal sync now uses the configured non-DataMart fallback whenever a
  DataMart source is requested but `datamart.ready=false`, including when a
  database `endpoint_flags` row requests DataMart. This prevents a shadow-only
  write from replacing the live ERP write.
- The current database flag values could not be read during this check because
  the state-store connection timed out. The guard above does not depend on
  those flag values, but their operator state still needs verification once
  the database is reachable.
- DataMart can still be sampled with the separate discovery/shadow tooling.
  No new live Visma parity run was performed in this change.

### Endpoint config validation (2026-09-29)

- The DataMart config schema now declares `compare_excludes`, which is used by
  the sample shadow comparator for known fields without direct DataMart parity.
- The schema requires unique, non-empty field names and still rejects unknown
  DataMart config properties. Excluding a field from a sample comparison does
  not establish semantic equivalence or authorize production cutover.

### Live schema refresh (2026-09-29)

- Authenticated introspection succeeded against the configured tenant. The
  current snapshot and SDL are now eligible for version control, and 53/53
  generated plus all endpoint-mapped GraphQL files validate against it.
- Live read-only samples returned the new `branchName`, Customer enum/payment
  fields, and GL deletion flag. This proves field deployment, not completion
  of Visma's historical backfills or per-ETL `importDone` status.
- The SalesOrder queries no longer select the 11 fields removed in Visma's
  [September 28 API release](https://docs.datamart.visma.net/release-notes/2026-09-28-api).
  Descriptive values formerly supplied by those fields still need same-company
  lookups before SalesOrder semantic parity can pass. The current normalizer
  emits nulls for those unavailable descriptions; `sales_orders_v3` remains
  ERP-routed and must not be activated on DataMart in this state.
- `generalLedgerBalancesV2` now requests `includeDeleted: true`; the loader
  translates DataMart `deletedDatabaseRecord` into the bronze `isDeleted`
  system field. This is covered by local tests, not a live BigQuery MERGE test.
- Branch and Customer normalizers consume the new fields. Customer backfill
  does **not** advance `updatedInDataMartDateTime`, so it requires a full
  re-extract after Visma confirms completion. Branch and GL cleanup backfills
  do advance that timestamp, but a full re-extract is safer if the saved
  watermark or completion point is uncertain. See the
  [ETL release notes](https://docs.datamart.visma.net/release-notes/2026-09-28-etl).
- No ETL activation, production full reimport, warehouse cutover, or BigQuery
  rewrite was performed. The GraphQL schema has no read-only ETL-status query,
  and the release notes do not confirm this tenant's backfill completion.

What is now true:

- Live DataMart introspection succeeds.
- The repo contains the current live GraphQL catalog.
- Safe local endpoint mappings have been synchronized with live `root_field`
  values.
- Missing query files for those safe mappings have been created.
- All generated DataMart query files validate against the live schema snapshot.
- Live smoke pulls succeeded for:
  - `supplier_invoices -> apInvoices`
  - `sales_orders_v3 -> salesOrders`
- The 19 synchronized mappings have now been reviewed endpoint by endpoint
  against live samples, bronze declarations, and the local normalizer layer.
- The previously missing bronze declaration schemas for the synchronized batch
  are now parseable.
- The requested bronze-parity normalizers are now implemented for:
  - `accounts`
  - `budgets`
  - `branches`
  - `contacts`
  - `credit_terms`
  - `customer_documents`
  - `customer_payments`
  - `customer_sales_prices`
  - `customers`
  - `general_ledger_balances_v2`
  - `sales_orders_v3`
  - `sales_persons`
  - `subaccounts`

What is still missing:

- 19 local endpoint configs have completed the mapping/bronze/normalizer review.
  Three further configs (`employees`, `financial_periods`, `inventory_items`)
  now match live roots by name but have not passed that review.
- 49 local DataMart placeholders still have no live DataMart equivalent or no
  mapping decision yet.
- 31 live DataMart roots still have no matching local endpoint config.
- Query shape is schema-valid, but broader endpoint coverage and semantic
  equivalence work is still incomplete.
- The review now shows real shadow candidates, but this is still not the same
  as a broad production cutover.
- A refreshed live shadow comparison report now exists for the 18 current
  candidates.

## Current Coverage

- Live query roots: 53
- Generated query files: 53
- Schema-valid generated queries: 53
- Paginated-compatible with the current runtime: 43
- Point-lookup only: 5
- Page-wrapper roots requiring a different adapter: 3
- Require extra runtime parameters: 2
- Safe local configs synchronized from live schema: 19
- Additional direct-name matches, not yet reviewed: 3
- Unmapped local placeholders: 49
- Live roots without matching local config: 31
- Missing config query files created: 16
- Synchronized mappings reviewed against bronze + normalizers: 19
- Candidate-for-shadow after review: 18
- Remaining synchronized blockers after review: 1
- Live sample shadow comparison runs completed for: 18
- Clean live shadow samples: 1 (`branches`, one row; excluded fields remain)
- Field-mismatch samples: 8
- No-key-overlap samples: 7
- Key-gap samples: 1 (`accounts`)
- Probe-error samples: 1 (`journal_transactions`, ERP six-month filter)

See:

- `docs/layeSystem/Layer1/datamart_graphql_catalog.md`
- `docs/layeSystem/Layer1/datamart_query_validation.md`
- `docs/layeSystem/Layer1/datamart_candidate_parity_review.md`
- `docs/layeSystem/Layer1/datamart_shadow_compare.md`
- `core/queries/_generated/datamart.introspection.json`
- `core/queries/_generated/datamart.sdl`

## Relevant Local Folders

| Area | Path |
|---|---|
| DataMart Rust client | `core/crates/vsv2-datamart/` |
| DataMart adapter | `core/crates/vsv2-orchestrator/src/dm_adapter.rs` |
| Generated live query catalog | `config/queries/datamart/` |
| Endpoint-mapped query files | `config/queries/` |
| Endpoint routing configs | `config/endpoints/` |
| Discovery tooling | `tooling/src/vsv2_tooling/discovery/` |
| Live schema snapshot | `core/queries/_generated/` |

## What Is Correct

- DataMart uses a dedicated source type: `datamart_graphql`.
- The runtime paginator matches the live DataMart bulk pattern:
  `pageSize`, `lastId`, optional `updatedInDataMart`.
- The live schema is now checked into the repo, not inferred from stale files.
- Safe endpoint configs now carry explicit `datamart.root_field` values, which
  the runtime needs for camelCase GraphQL roots.
- Safe mapped configs now also have real query files at their configured
  locations.
- Generated queries are validated against the live schema snapshot, not only
  parsed as text.

## What Is Still Blocking "Perfect" Layer 1

### 1. The synchronized batch is materially improved, but not complete

The candidate review now shows 18 synchronized mappings as
`candidate_for_shadow`:

- `accounts`
- `branches`
- `contacts`
- `credit_terms`
- `customer_documents`
- `customer_payments`
- `customer_sales_prices`
- `customers`
- `general_ledger_balances_v2`
- `journal_transactions`
- `locations`
- `payment_methods`
- `sales_orders_v3`
- `sales_persons`
- `subaccounts`
- `supplier_classes`
- `supplier_documents`
- `supplier_invoices`

This is a large step forward from the previous `0`.

### 2. One synchronized mapping is still blocked

The current blocker is explicit:

- `budgets`: the last recorded tenant sample was unavailable; this was not
  re-probed in the September 29 run.

This is now more specific than before: on 2026-05-13 the live DataMart
`budgets` query returned zero rows for the tenant, and the live ERP endpoint
`/v1/budget` also returned zero rows for every valid
`branch x ledger x financialYear` combination that exists on the tenant. The
normalizer is implemented, but tenant data is currently absent.

### 3. Candidate-for-shadow is contract proof, not full semantic equivalence

The new candidate count proves field-contract coverage at the bronze layer.
It does not by itself prove that every candidate is semantically identical to
the legacy REST payload.

Important examples:

- `branches` now supplies `branchName` for `name` and `branchCode` for `number`.
  The business-account name may still differ from the ERP legal name; the
  organization and VAT registration fields remain unavailable.
- `customer_documents` now satisfies the bronze contract, but several
  downstream object-style fields such as `project`, `customer`, `branch`,
  `paymentMethod`, and `creditTerms` are synthesized from DataMart scalar IDs.
- `customer_payments` now satisfies the bronze contract, but legacy aliases
  like `refNbr`, `type`, `customer`, `branch`, and `paymentMethod` are rebuilt
  from the DataMart row, and the legacy in-currency fields remain unavailable.
- `customer_sales_prices` now satisfies the bronze contract, but the live
  `SalesPrice` type still does not expose the legacy `priceType` discriminator
  that downstream SQL uses for `Customer` vs `CustomerPriceClass`.
- `sales_orders_v3` now satisfies the bronze contract, but the current
  DataMart root still does not expose native order-line arrays, so the
  normalizer currently emits empty `lines` / `orderLines` placeholders.
- `general_ledger_balances_v2` now satisfies the bronze contract, but some
  legacy code-oriented fields are reconstructed from available DataMart IDs.
- `budgets` now has a normalizer and a corrected bronze declaration shape, but
  the live DataMart `Budget` type only exposes numeric branch/account/subaccount
  IDs. If budget data appears later, correct ERP-style business keys will still
  require enrichment.

Those endpoints are now good shadow candidates, but they still need shadow
comparison before any live cutover decision.

### 4. The refreshed live shadow comparison still blocks broad cutover

The refreshed shadow report compares ERP REST against normalized DataMart
samples for the 18 current candidates.

Current live result after the September 29 schema refresh:

- `clean_sample`: 1
- `field_mismatch`: 8
- `no_key_overlap`: 7
- `key_gap`: 1
- `probe_error`: 1

The report shows two distinct problem classes:

- structural field mismatches on overlapping business keys
- sample/key misalignment where ERP and DataMart did not overlap at all in the
  fetched windows

The highest-signal current findings are:

- `accounts`: overlapping keys exist, but field gaps remain around
  `allowManualEntry`, `controlAccountModule`, `postOption`, and metadata/timestamp
  behavior.
- `branches`: the one-row sample matches on compared fields, including `name`,
  but unavailable legal/registration fields are still excluded.
- `customers`, `customer_payments`, `customer_sales_prices`,
  `sales_persons`, `subaccounts`, `supplier_documents`, and
  `supplier_invoices`: overlapping keys exist, but material field mismatches
  remain.
- `contacts`, `customer_documents`, `general_ledger_balances_v2`,
  `locations`, `payment_methods`, `sales_orders_v3`, and `supplier_classes`:
  no overlap was found in the current live sample windows. `journal_transactions`
  could not be compared because the ERP probe violates its six-month filter.

### 5. Local endpoint coverage is still partial

Today 22 local endpoint configs have a direct live-root name match, but only 19
have been through the synchronized bronze/normalizer review.

The remaining 49 placeholders need one of these outcomes:

- map to a real live root
- be explicitly marked as ERP-only
- be replaced by a new DataMart-native endpoint config

### 6. Some live DataMart roots need adapter work before scheduled ingestion

The validation report shows:

- 43 roots are paginated-compatible with the current adapter
- 3 page-wrapper roots need an `items`/cursor-aware adapter
- 2 list roots require extra non-null runtime parameters:
  - `aggregatedSales`
  - `generalLedgerBalancesByPeriod`
- 5 roots are point-lookup only and are not bulk-ingestion queries

### 7. Broad cutover still needs endpoint-specific proof

The new review report is the proof step, and it currently says broader cutover
should not happen yet.

Before enabling wider DataMart routing, each candidate endpoint still needs:

- a real bronze declaration schema
- field-level bronze parity review
- key and dedupe review
- tenant/company scope verification
- shadow comparison against ERP REST where both exist

## Recommended Next Steps

1. Confirm Visma's per-environment September 28 release and backfill-completion
   notifications, then verify each required ETL is active with `importDone=true`.
   Read-only introspection and sample rows cannot establish either condition.
2. After completion, re-extract Customers and other entities whose backfill
   does not advance `updatedInDataMartDateTime`; stage by tenant and DataMart
   company ID, reconcile natural keys/counts, and only then advance checkpoints.
3. Treat `budgets` as tenant-data-blocked until either ERP or DataMart exposes
   live rows for the tenant.
4. Use `docs/layeSystem/Layer1/datamart_shadow_compare.md` as the active
   endpoint-by-endpoint remediation list.
5. For the 8 `field_mismatch` endpoints, close the concrete field gaps before
   any live cutover discussion.
6. For the 7 `no_key_overlap` endpoints, improve comparison scoping or
   key-alignment so that the shadow run compares the same logical slices.
7. Repair the journal ERP probe's six-month filter and re-run it; the current
   `probe_error` is not a parity result.
8. For `customer_sales_prices`, decide whether the current DataMart root is
   acceptable without `priceType`, or whether another root or enrichment step
   is required before any live cutover.
9. For `branches`, validate the new `branchName` on more than the one-row pilot
   sample and resolve legal/registration fields separately.
10. For `sales_orders_v3`, resolve removed descriptive fields through
   company-scoped lookups; also decide whether empty bronze line placeholders are
   acceptable for shadow-only use, or whether a `salesOrderLines` enrichment
   step is required before live cutover.
11. For `general_ledger_balances_v2`, verify that the ID-based fallbacks for
   `account.number` and `subAccountCd` are acceptable in downstream finance
   joins, and test tombstone MERGE in a non-production BigQuery table.
12. Review the 3 new direct-name matches, then decide the fate of the 49
   unmapped local placeholders: map, remove, or mark
   ERP-only.
13. Add local configs for the 31 live DataMart roots that are useful for the
   warehouse but do not exist locally yet.
14. Extend the adapter for the 3 page-wrapper roots and only where the validation
   report proves extra parameters are required.
15. Promote only the endpoints that pass real shadow comparison, not the whole
   layer at once.

## Gate

Layer 1 is on the right path now.

It becomes broadly production-ready only when:

- the remaining synchronized blocker is closed
- the 18 current candidates pass real shadow comparison
- semantic gaps like `sales_orders_v3` line coverage are resolved explicitly
- semantic gaps like `customer_sales_prices.priceType` are resolved explicitly
- unmapped placeholders are resolved deliberately
- only then are per-endpoint DataMart cutovers enabled
