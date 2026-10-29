# visma_service_v2: production readiness (2026-10-13)

This replaces `CUTOVER_READINESS.md` of 2026-06-03, which described a v2 that had never run against live
data. What follows is the state after the work of 2026-10-09 to 2026-10-13. All of it was done on one
machine: nothing is deployed, nothing is pushed, the Dataform `main` branch in GCP and the old project
`prj-dw-dev` are untouched, and every request to Visma was a GET.

## Verdict

**v2's data is complete and right wherever it can be measured today, the pipeline runs end to end in its
container, and the open work is the work that needs a decision, an account or time. It is not live.**

| Question | Answer |
|---|---|
| Is the data right? | Yes, against Visma itself, on the snapshot. Population of open invoices: 0 defects in all 12 tenants (the gate fails only on the age of the data); the same gate finds 232 + 3 defects in v1 on the same day. Sales orders 12 of 12. Endpoint counts: of 756 cells 752 pass (656 exact, 96 within what changed since the last write); the other 4 were traced by key to changes made in Visma after the fetch. Ledger: 0 unbalanced batches of 146,151. |
| Is the logic right? | The four logic gaps of the audit are decided and fixed from the finance workbooks and the ledger. The completeness audit found 21 defects in silver and gold (12 root causes); all are fixed and the audit exits 0. The whole Dataform project runs: 258 of 258 actions, 40 of them assertions, all clean. |
| Does it run? | The scheduler, in its container image, ran all three tiers for one tenant against real Visma and a scratch dataset: 3 passes, 0 failed endpoints, 29 minutes, and the count gate on what it wrote was 64 of 64 exact. It is not running continuously: there is no host. |
| Has it run long enough? | No, and it cannot be shown on a laptop: the plan asks for two weeks of fresh data with the gates green, month-end included. |
| What is red? | One gate, parity with v1 (gate-a): 539 of 579 findings unexplained. It cannot be green while v1 and v2 hold feeds of different ages, and it is not the gate that decides correctness; see "Scorecard". |

## The four conditions

| Condition | State |
|---|---|
| **Complete**: everything v1 has | Yes. Every v1 table exists (74 bronze tables, 220 of 256 v1 objects with a same-name equivalent, the rest listed in `v1_coverage.md`), every v1 endpoint is fetched or derived. Not complete in the other direction: 48 endpoints with rows (32 DataMart, 14 ERP, 2 Expense) have raw staging but no typed bronze table. That is new data, not part of v1. |
| **Correct**: right against Visma, not against v1 | Yes at the snapshot of 2026-10-01/02 (and 2026-10-09 for the endpoints reloaded then). Re-proving it on fresh data is part of the soak. |
| **Running**: a scheduler keeps it fresh | Built and rehearsed, not started: no host, no service account, no Dataform workflow config. |
| **Soaked**: it has been right for two weeks | Not started. |

## Scorecard

| Area | State | Evidence |
|---|---|---|
| Open invoices against Visma (gate-b) | v2: 0 DEFECT in 12 tenants; verdict FAIL only for staleness (186 to 202 h since the last sync: nothing refreshes the snapshot). v1, same day: 232 defects (Aquatiq AS customer invoices still open that Visma closed) and 3 (Food Factory) | `gates/population_v2_2026-10-09.md`, `population_v1_2026-10-09.md` |
| Sales orders per company (gate-s) | 12 of 12 PASS (Aquatiq AS 209 missing, within the 336 changed since the last sync) | `gates/sales_order_counts_2026-10-10.md` |
| Endpoint counts (gate-e) | 63 endpoints, 12 tenants, 756 cells: PASS 752 (656 exact, 96 within what changed since the last write), WARN 1, FAIL 3. The 3 FAIL are `sales_order_customers` (Visma's list ignores the modified filter): each missing customer was created, re-activated or inactivated after the fetch, so class B (v2 right). 33 endpoints cannot be counted (no total, or a parent key is needed); the reasons are listed per endpoint | `gates/endpoint_counts_2026-10-12.md` |
| The pipeline in its container (rehearsal) | One tenant, three tiers, scratch dataset: 64 of 64 cells exact, heartbeat as the last write | `gates/endpoint_counts_2026-10-13_rehearsal_aps.md`, `DEPLOYMENT.md` 4c |
| Ledger integrity (gate-c) | The 39 assertion views in `visma_assertions` return 0 rows. Every one of the 146,151 batches (12 tenants) nets to zero, and every company-period | run 2026-10-13 |
| Silver and gold completeness | 109 models audited: 72 exact, 14 intended differences, 2 derived; 21 defects found (12 root causes) and fixed; 119 key pairs, 0 unresolved; the audit script exits 0 | `silver_completeness.md` |
| Dataform | 258 of 258 actions succeed (218 tables and views, 40 assertions), in the Dev workspace | `dataform_validation.md` |
| Business rules from the workbooks | Merger move and migrated opening balances (seed rule), department per tenant structure, margin on costed lines with the 68 charge products, journal batch exceptions | `dataform_validation.md` "Fixed 2026-10-13" |
| Parity with v1 (gate-a) | **Red**: 579 findings, 40 explained, 539 not, in 99 tables. Never green. Why it is not the deciding gate: `gates/README.md` "gate-a" | `gates/README.md` |
| Open-document re-read | Built, reviewed, and mutation-tested (15 mutations, each caught): customer and supplier invoices, once a day per tenant, three kinds of document fetched by key (open in staging only, open in Visma only, part-paid by `balance`), never deletes, 4-minute bound. Both invoice endpoints succeeded in the rehearsal, but that image predates the balance compare; the compare itself is proven by tests with real Postgres and fake staging, not yet end to end against BigQuery. Visma's open list rows carry `balance` (checked live) | `DEPLOYMENT.md` 4b |
| Monitoring and alerting | `vsv2 monitor` (state-database checks per tenant and tier, exit codes 0/1/2, optional chat webhook). Its Rust tests pass on the final tree. It was run against the rehearsal's state database on 2026-10-13: all 12 tenants gave exit 1, the rehearsal tenant alone gave exit 1 (its hourly pass was 7 h old), and an unknown database gave exit 2. Its code was written by a helper that was stopped before its review, so it has had no independent review. **Not scheduled:** nothing runs it yet, and there is no alerting channel (open item 2) | `core/crates/vsv2-cli/src/monitor*`, `DEPLOYMENT.md` 4d |
| State schema | `vsv2 schedule` and `vsv2-http` apply the 12 migrations from files embedded in the binary; a recorded file that is edited afterwards is refused | `core/crates/vsv2-state/src/migrate.rs` |
| Image and compose | The image builds from the final tree (checked 2026-10-13; the planner copies all of `core/`, and the build fails if a `vsv2-bq-*` command is missing). It contains the four `vsv2-bq-*` commands and `vsv2 monitor`. `docker-compose.prod.yml` removes password defaults and published ports, rotates logs, gives a 120 s stop | `DEPLOYMENT.md` 4a |
| CI | `.github/workflows/visma-service-v2.yml`: Rust (fmt, clippy `-D warnings`, test against Postgres and Redis), Python (ruff, pytest), Dataform compile, image build. Written, never run on GitHub (nothing is pushed) | the file |
| Tests | Run 2026-10-13 on the committed tree. Rust: fmt and `clippy -D warnings` clean; `cargo test --workspace` 753 passed, 0 failed, 14 ignored (each of the 14 reads live Visma or BigQuery and runs by hand). Python: 561 passed, ruff and format clean, 135 endpoint configs valid. mypy (strict, configured in `pyproject.toml`, but no gate runs it): 34 errors, all already at HEAD; this work added none and fixed one. Dataform: compiles locally, 218 tables and views, 40 assertions, 0 errors. Currency service: the 4 tests of the new client pass | see "How to re-prove it" |
| MCP server | 17 tools, 19 skills, known issues listed per tool; reads verified live; writes verified on a scratch database | `core/crates/vsv2-mcp` |
| Currency service | Env-driven target (`GCP_PROJECT_ID`, `GCP_DATASET_ID`, `GCP_BQ_LOCATION`); template points at `aquatiq-datavarehus.dw_staging_currency`; tested on a scratch dataset in the new project (372 rates, 0 differences against the old service, a second run appended nothing); 4 tests | `apps/currency_service` |
| Secrets | `.env` is ignored and never in the image; the Google key is mounted, not baked. Two secrets in git need rotating, see "Open items" | |

## What changed since the audit of 2026-10-09

The audit left four logic questions. They are decided, with the finance workbooks (`dataform/inputs/excel/`)
and the ledger as evidence, and each has a guard that fails the Dataform run if it breaks again. The effects,
in numbers somebody reads, are in `dataform_validation.md`; here is what moved.

* **The merger move.** Project history is now found by a seed rule: account 3999 or 4999, or opening-balance
  wording ("IB prosjektsaldo", "inngående saldo", "før systembytte") together with the other lines of the
  same journal batch. It also finds Hygiene Systems' 2023 opening balances and the move to Visma.net of AFF
  and Consult. Year-end accruals ("Periodisert") that Aquatiq AS reverses in 2026, and the valuations
  ("Prosjektvurdering"), stay as activity. Aquatiq AS: historical income 40.4 M becomes 65.3 M, and 70.1 M
  income and 60.6 M cost move from Chemistry to Hygiene Systems because the move was posted with the new
  business-unit structure. Row counts and totals are unchanged; only the flag and the business unit move.
* **Department.** The department is the subaccount segment called "Avdeling", not always segment 3. It is
  segment 3 on the six tenants with the new structure and segment 2 on the six with the old one.
* **Margin.** `BI_SalesOrder` gets `has_cost`, `margin_amount_nok`, `amount_price_with_cost_nok` and
  `is_charge_line`. A margin over all lines was 71.4 %; over the lines that have a cost it is 53.1 %.
* **Journal batches.** 49 posted batches where Visma's header totals and lines disagree (47 where the header
  totals agree with each other and exceed the lines, which balance; 2 where the header's own debit and credit
  differ) are recorded with their differences and no longer fail the assertion; a new mismatch does. The lines
  staged equal Visma's own count for the periods checked, so nothing is lost on the way.
* **21 defects in silver and gold**, found by comparing every model with its source and fixed the same day:
  item, location, fixed-asset and purchase-order keys that never joined, inventory movement and shipment
  lines without item, payments and supplier invoices collapsed by a key collision, NULL attributes.
* **Business-unit and quality checks that v1 had**, ported as assertions.
* **Currency and Excel inputs** no longer depend on a copy: the service writes where its three variables say,
  and `dataform/tools/excel_inputs.py` checks and loads the workbooks.

## What Power BI will see on the switch

The five tables in `visma_datamart` keep every column and type of v1; `BI_SalesOrder` has four more. These
change, and a report that stored or filtered on the old values needs a refresh:

* `item_key`, `location_key`, `fixed_asset_key`, `purchase_order_key` take new values (before the fixes none of
  them joined, so nothing worked on them), and `fact_general_ledger_balance.business_unit_key` changes type from
  STRING to INT64.
* `dim_location` has about 35,700 rows instead of 149 and a location key that includes the business account; `dim_inventory_item`
  is keyed on the item code (12,934 rows, the placeholders are gone); `dim_department` holds the departments
  that exist as a segment and not in the department master.
* `BI_ProjectPortfolioPnL`: the historical and "excluding historical" columns move (figures above), the
  business unit of the moved projects changes, and mixed-unit projects in Aquatiq AS fall from 88 to 65.
* Payments: customer payments gain 134 rows and supplier payments 108 that a key collision used to drop (the
  Payment and Void pairs, each pair nets to 0). A report that filters on `status = 'Closed'` counts the voids
  without their originals.
* The warehouse labels business unit 02 "AHS", the workbook "Hygiene Systems". Unchanged; a report may depend on it.

## Open items

**Needs a decision from you**

1. **Where the scheduler runs.** Recommendation: a small GCP VM in the warehouse's region with an attached
   service account, so there is no key file to guard (one of the two secrets found in git is such a key file). The
   alternative is the VPS beside v1, with a key mounted read-only. v1 and v2 share Visma's request quota while
   both run, so v2 starts with a low budget (`VSV2_SCHEDULER_ERP_RPM`).
2. **Alerting channel.** `vsv2 monitor` needs a place to send its verdict: a chat webhook (`VSV2_MONITOR_WEBHOOK_URL`), or its exit code to cron, a systemd timer or Cloud Scheduler. Which one is yours to choose.
3. **Finance endpoints hourly or daily.** v1 refreshed general-ledger transactions, expense claims,
   fixed-asset transactions, customer credit notes and stocktakes hourly; v2 has them daily. With a daily
   Dataform run the finance data is up to a day old instead of about five hours. It is one line per endpoint
   (`tier: 1`); for ledger transactions a pass walks every period, about 94 period requests per tenant.
4. **Orders and other documents without an open-set re-read.** A payment moves a balance without moving
   `lastModifiedDateTime`. Invoices are covered; purchase and sales orders wait for the weekly full pass. Add a
   daily replay window, or accept up to a week.
5. **The 48 endpoints without a typed bronze table** (roadmap phase 4), and the order: DataMart first
   (contacts +25,055, sales prices +2,048) is the recommendation.
6. **The business-unit label** (AHS or Hygiene Systems).

**Needs access, yours or the cloud's**

7. A service account for vsv2 (BigQuery Data Editor on `dw_staging_visma`, Job User on the project) and one for
   the currency service (Data Editor on `dw_staging_currency`, Job User). Only `visma-dataform` and
   `xero-dataform` exist in the new project.
8. A Dataform workflow config (daily, after vsv2's daily tier) and a commit of the Dev workspace to `main`
   (`deploy.py commit`). Until then a run from `main` brings back the old rules.
9. Power BI: read access on `visma_datamart` for whoever refreshes the model, and the new connection.
10. The Excel workbooks behind `seed_company_account_mapping` and the two global account mappings ("Group
    mapping (kun NO, SE, DK ...).xlsx") are not in the repository; the seeds are their only record.

**Security, needs you**

11. `apps/currency_service/.env.production` is tracked in git with real-looking values. Rotate them and
    untrack the file. Nothing was changed there.
12. A service-account key of `prj-dw-dev-bq` is committed in the v1 Dataform repository (`Datawarehouse`,
    `df-credentials-object.json`) and is active. Rotation does not have to wait for the cutover.

**Known and accepted**

* NOK on voids applied on another date than their payment do not net exactly (3 of 134 customer pairs, 9 of
  108 supplier pairs); in base currency all net to 0.
* No bin dimension (`location_key` is NULL on inventory movements) and no supplier locations: nothing in Power BI reads them.
* Endpoint capability per tenant (`vsv2.endpoint_capability` exists and is unused): an endpoint a tenant
  lacks fails on every pass and retries; look at the first live pass before deciding.
* The Postgres state is not backed up. Losing it means one full pass to rebuild checkpoints; staging is the
  data and lives in BigQuery.

## Go-live sequence

1. **Decide** items 1 to 3 above, and create the service accounts (7).
2. **Host.** Create it, install Docker, copy the repository folder and `.env` (never the key into the image),
   `docker compose -f docker-compose.yml -f docker-compose.prod.yml up -d postgres redis`.
3. **Rehearse once more, there.** `DEPLOYMENT.md` 4c on the new host with one small tenant and a scratch dataset.
4. **First pass, one tenant** against the real staging dataset, then all 12. The first full pass of a large
   tenant takes hours. Watch `vsv2.scheduler_pass`, the monitor, and the request budget (shared quota with v1).
5. **Gates after the first pass:** gate-e, gate-s, gate-c, then gate-b on fresh data (it should now pass, not
   just fail on age), then gate-a, now that the feeds are the same age: give each remaining finding an entry or fix it.
6. **Dataform:** commit to `main`, add the workflow config. Run the assertions on fresh data.
7. **Currency:** on the server that runs the currency service, set `GCP_PROJECT_ID`, `GCP_DATASET_ID`,
   `GCP_BQ_LOCATION` and mount the new key; check `Rate` and `Historyrates` in `dw_staging_currency` after the
   16:10 run.
8. **Power BI:** repoint the model (see "What Power BI will see"), refresh, compare the key reports with v1's.
9. **Soak two weeks**, month-end included, gates daily, v1 still running as the fallback.
10. **Retire v1:** stop its sync, archive `prj-dw-dev`, rotate the committed key (12) and the tracked
    secrets (11).

## How to re-prove it

```bash
cd core && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings \
  && cargo test --workspace                      # needs `make up` (Postgres and Redis)
uv run ruff check . && uv run ruff format --check . && uv run pytest
make validate-config
python3 dataform/tools/deploy.py compile         # 258 actions
.venv/bin/python scripts/run_gates.py --log-dir /tmp/gates   # five gates; gate-b makes live Visma GET calls
.venv/bin/python scripts/audit_silver_completeness.py
```

## What this does not prove

* That v2 stays right while it runs: no live pass, no delete reconciliation in anger, no month-end close.
* How long the first full pass of the large tenants takes. The rehearsal tenant is the smallest active one.
* Anything about Power BI itself: the model is not in the repository.
* The DataMart tier beyond one small tenant. The rehearsal ran all 38 DataMart endpoints without a failure (31 with rows), but no gate counts them against Visma: `verify_endpoint_counts.py` covers the ERP REST endpoints, and DataMart rests on `reconciliation.md`.
* CI on GitHub. The workflow was written and its steps run by hand.
* The 48 endpoints without a typed table, and the cause of v1's lost ledger lines (v1's logs were not read; v1 is hands-off).
