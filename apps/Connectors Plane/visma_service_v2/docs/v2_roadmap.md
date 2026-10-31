# v2 roadmap: replace v1 without losing what v1 gets right

Written 2026-10-09 after the Dataform port, revised the same day with the result of the data audit
(next sections). The goal, in the user's words: v2 replaces v1 in every way; v1's results are right and
must not be lost; v2 adds more and fuller data (ERP API together with Visma DataMart) and builds
structured gold and datamart layers on it, with no errors and no gaps.

**Refreshed 2026-10-13.** Everything that can be done on one machine without going live has been
done, and the rest is written down in [PRODUCTION_READINESS.md](PRODUCTION_READINESS.md): the verdict,
the scorecard with the gate results, what is left and the order to do it in. The audit text below is
kept as it was written; "Where things stand", the status lines under each phase and "Decisions needed"
are current.

The v1 side of this plan, with the defects and the retirement steps, is in
`../../docs/V1_STATUS_AND_ROADMAP.md`.

## The principle

v1 is two things, and they are kept apart:

* **The business logic** (silver, gold, the `BI_*` tables): tuned over a long time, in daily use,
  right. It is carried over unchanged (`dataform/definitions/silver`, `gold`). It runs on v2's data
  through the v1-shaped bronze layer (`visma_bronze`, 74 tables with v1's columns, built from the raw
  staging JSON). One exception: where the audit below proved a gap in the logic itself (it gives the
  same answer on any feed), the fix is listed in phase 3, one decision at a time, because each one moves
  a number somebody reads.
* **The data feed** (v1's sync into `dw_1_bronze_visma`): the part v2 replaces. It had holes the logic
  could not see: customer invoices that stayed open after Visma closed them, general-ledger lines that
  never arrived (batches that do not net to zero), open balances that stayed stale because a payment
  does not move an invoice's `lastModifiedDateTime` (`docs/dataform_validation.md`), GL balances of 0 for
  accounts with postings, four tenants not synced since 2025, duplicated bronze rows. v2's feed is
  complete, fresh, and knows about deletes, measured on the snapshot of 2026-10-01/02; proving it on live
  data is phases 1 and 2.

So "keep v1's results" means: **the same logic on a better feed, with gates that prove every difference
from v1 is one where v2 is right.** It does not mean copying v1's numbers where v1's feed was stale or
incomplete.

New data is added **beside** the v1 core, never by changing the source of a model v1 already has:
ERP stays the source of the core (it carries more fields per record; `docs/field_parity.md`), DataMart
supplements it with the rows ERP hides and the fields ERP lacks (`docs/reconciliation.md`: DataMart is
the superset for rows and deletes, ERP for fields).

## What the audit of 2026-10-09 showed

Another session checked the warehouse against Visma and found four problems: open receivables
overstated, ledger batches that do not net, a merger move that is only partly flagged, and sales figures
to read with care. This session located each one. The same Dataform SQL was run on v1's feed and on
v2's, and v2's raw staging was compared with live Visma key by key. A number that is wrong on v1's feed
and right on v2's is a feed fault; a number that is the same on both is the logic or the source.

| # | Finding | v1 feed | v2 staging | Dataform logic | Fails in |
|---|---|---|---|---|---|
| 1 | Open customer invoices, Aquatiq AS: warehouse 1,183 / 69.46 M, Visma 951 / 61.39 M | 232 invoices (8.07 M) still Open that Visma has Closed | all 232 Closed; every difference to live Visma is the age of the snapshot | right | **v1 feed** |
| 2 | Ledger batches that do not net: v1 has 1,072 batches and 246 company-periods | lines missing in bronze | 0 unbalanced batches and 0 company-periods in all 12 tenants; batch 014427 equals Visma's header | right (a typed copy) | **v1 feed** |
| 3 | Merger move flagged only on accounts 3999/4999: 29.7 M income and 24.2 M cost sit on ordinary accounts | same rows as v2 | same rows as v1 | the flag rule is incomplete | **Dataform logic**, identical in v1 and v2 |
| 4 | Sales: 39 % of value has no cost; does `ic` mean intercompany | same as v2 | same as v1 | cost is empty on two line types in Visma's data; `ic` is right; the department fallback depends on the tenant | **source and logic**, not the feed |

### 1. Open customer invoices (v1 feed)

* Visma live (REST, `status=Open`): 951 invoices, 61,387,472.26. v1 silver: 1,183, 69,457,786. The gap is
  exactly 232 invoices and 8,070,313.
* v2 staging has all 232 as Closed. v2's snapshot is a week older than v1's data, so "Closed in v2, Open
  in v1" cannot be a recent payment: v1 is stale. In staging their `lastModifiedDateTime` falls between
  2026-03-25 and 2026-04-15 (109 in March, 123 in April); v1 carries earlier timestamps for most of
  them (58 in February, 159 in March, only 6 in April), so it never read the change.
* v2 against live Visma, by key: of the 951 open invoices staging holds 707; the other 244 were created
  or changed after the snapshot (documents dated 2026-09-30 to 2026-10-09). 211 of staging's 917 open
  invoices have been paid since. Nothing is wrong inside staging.
* All tenants, customer and supplier invoices: only Aquatiq AS has stale open documents in v1 (customer
  232, supplier none). This is a lower bound, see the limits below.
* Mechanism: v1's bronze for Aquatiq AS has a full refresh on 03-25 (16,350 rows), then almost nothing on
  03-26 to 04-01 and sparse writes after. The status changes of that window were never read back. Why
  the sync wrote so little is not established; v1's logs were not read (v1 is hands-off).

### 2. Ledger batches (v1 feed)

* v1 has 1,072 batches whose debit and credit differ (Aquatiq AS 60, the other seven active tenants 484,
  the four historical tenants 528) and 246 company-periods that do not net to zero. v2 has 0 and 0.
* Aquatiq AS: v1 and v2 share 67,636 batches. In 64 of them v1 has fewer lines (457 lines missing; v1
  never has more). v1 lacks 43 batches entirely. (v1 also has 932 batches v2 lacks, in periods 202608 to
  202611; all were first modified on or after 2026-10-01, at or after v2's snapshot.)
* Batch 014427 (DR, 202408): v1 holds 4 of its 10 lines, debit 22,836.50 against credit 987,080.70, out
  by -964,244.20. v2 holds all 10, 1,005,885.78 on both sides, equal to Visma's header (`debitTotal`,
  `creditTotal`, `controlTotal`).
* The seven periods the audit named: v1's net debit minus credit is -384,013 (202402), +385,126
  (202403), +407,913 (202405), -398,586 (202406), -964,244 (202408), +560,164 (202409) and +397,681
  (202412). v2: 0.00 in all seven.
* Broader loss, Aquatiq AS: 1,277 of 397,757 lines last modified before 2026-03-24 (0.32 %) are in
  staging and not in v1's bronze.
* Mechanism: not established. One partial signal: lines are lost more often where many share the same
  last-modified second (1.15 % in groups of 100 to 999 lines, 0.07 to 0.11 % in groups of 2 to 99), as
  expected when a lastModified cursor cuts inside a group. Single-line groups lose 0.92 % (12 of 1,310),
  so ties are at most one of the mechanisms.

### 3. The merger move (Dataform logic)

* v1 and v2 give identical numbers: Aquatiq AS has 233 project lines dated 2025-12-31 on 25 `HS-`
  projects, income 70.1 M (flagged 40.4 M), cost 60.6 M (flagged 36.5 M). The flag is
  `debit_account_cd IN ('3999', '4999')` in `fact_project_portfolio_line`, and
  `BI_ProjectPortfolioPnL` builds historical income and cost from it.
* The unflagged remainder, 29.7 M income and 24.2 M cost, is two kinds of line: "IB prosjektsaldo ... pr
  31.12.25" (195 lines in module GL, income 24.9 M, cost 24.8 M) and "Periodisert inntekt / kostnad AFF"
  (17 lines, income 4.84 M, cost -0.6 M). They are project transactions from Visma's project ledger, on
  ordinary accounts, so no feed change can touch them.
* The other tenants have no "IB prosjektsaldo" lines. Hygiene Systems posts "Prosjektvurdering" reversals
  on the same date (income -6.59 M, cost -7.85 M) that no flag covers.

### 4. Sales (source and logic)

* Raw staging, order lines of `sales_orders_v3`, all tenants, 90,161 lines: GoodsForInventory 73,205
  lines (1.0 % without cost), NonInventoryGoods 13,302 (97.9 %), MiscCharge 3,654 (99.4 %). Visma returns
  no cost on the last two types. In `BI_SalesOrder` 19.4 % of the lines and 39 % of the value (v1 39.2 % of
  1.29 bn NOK, v2 39.3 % of 1.27 bn) sit on lines with cost 0 or empty, the same on both feeds. A margin
  computed over all lines is therefore overstated.
* `ic` is subaccount segment 1, named "IC" or "Intercompany" in all 12 tenants. In 2025, 13.0 % of the
  427.0 M NOK in `BI_SalesOrder` has an ic code other than `00`, the same on both feeds.
* `BI_SalesOrder` falls back to subaccount segment 3 for the department. Segment 3 is "Avdeling" in 6
  tenants, "Prosjekt" in 3, "Lokasjon" in 1 and "Ikke i bruk" in 2. The fallback is right only where it
  is a department; the effect was not measured.

### What the audit does not prove

* The cause of v1's sync gap (2026-03-25 to 04-15) and of its lost ledger lines. v1's logs were not read.
* That v2 is right live. Staging is the snapshot of 2026-10-01/02; "right" above means right at that
  time. The scheduler has never run, so a week of changes, deletes and the weekly full pass are untested.
* The check for other tenants and for supplier invoices uses v2's snapshot as the reference, so it is a
  lower bound: an invoice that v1 holds as stale and that was paid between 10-02 and 10-09 looks open on
  both sides.
* Sales cost against live Visma. Staging shows what Visma returned when the orders were fetched.
* Whether "IB prosjektsaldo" and "Periodisert" lines are merger moves to exclude is a business question.

## Where things stand

State on 2026-10-13. The column on the right is what changed since 2026-10-09; the verdict and the
evidence are in [PRODUCTION_READINESS.md](PRODUCTION_READINESS.md).

| | State |
|---|---|
| Staging `dw_staging_visma` | 135 endpoints, 4.6 M rows, a snapshot last written 2026-10-09. The scheduler that refreshes it is built and has run in its container against a scratch dataset (one tenant, three tiers, 0 failed endpoints); it is not running against the real dataset |
| Bronze `visma_bronze` | 74 tables = every v1 bronze table, from 74 of the 135 endpoints |
| Silver, gold, datamart | v1's SQL with the fixes below; the whole project runs in the Dev workspace, 258 of 258 actions succeed (218 tables and views, 40 assertions); `scripts/audit_silver_completeness.py` exits 0 over 109 models (21 class A defects found on 2026-10-13, all fixed) |
| Not built | 61 endpoints: 38 DataMart `dm_*` (32 with rows, 2.07 M), 21 ERP without a v1 table (14 with rows), 2 Expense. Unchanged |
| Inputs | Excel: `dataform/tools/excel_inputs.py check` and `load` against the workbooks in `dataform/inputs/excel/`, the tables equal them. Currency: the service writes to whatever `GCP_PROJECT_ID`, `GCP_DATASET_ID` and `GCP_BQ_LOCATION` name; repointed to `aquatiq-datavarehus.dw_staging_currency` in the template and tested on a scratch dataset, not switched on the server |
| Scheduling | vsv2: built, container and production override (`docker-compose.prod.yml`), embedded migrations, CI; no host chosen, so not running. Dataform: no workflow config (on purpose, until go-live) |
| Known defects, v1 feed | 232 stale open invoices (8.07 M), 1,072 unbalanced ledger batches, zero GL balances, see above. v2 has none at the snapshot, and the population gate fails v1 on the same data (negative control) |
| Known defects, logic | the four gaps below are decided and fixed (2026-10-10 and 2026-10-13, `dataform_validation.md`); 49 journal batches where Visma's header and lines disagree are recorded and guarded |
| Checks that exist | gate-b population (all 12 tenants), gate-c ledger integrity (assertions), gate-e endpoint counts (every ERP list endpoint that reports a total, all 12 tenants), gate-s sales orders per company, the 40 assertions, the silver and gold completeness audit; `scripts/run_gates.py` runs the gates |
| Git | committed on a local branch, not pushed; the Dataform `main` branch in GCP is untouched |

## The plan

### 1. Make v2 live (prerequisite for everything else)

**Status 2026-10-13: prepared, not started.** Built and tested locally: the open-document re-read
(`docs/DEPLOYMENT.md` 4b), the production compose file, the migration runner (the binary applies the
state schema itself), CI (`.github/workflows/visma-service-v2.yml`), and a dress rehearsal of the
container against Visma and a scratch dataset (4c). The Excel workbooks load with a script, and the
currency service points at the new project through three variables. What is left needs a decision or
access: the host, the service accounts, the Dataform workflow config, the switch of the currency
service on its server. See PRODUCTION_READINESS.md, "Go-live sequence".

Nothing can be judged against a week-old snapshot.

* Run the vsv2 scheduler on a machine that stays up: the VPS beside v1 (compose stack exists, needs an
  explicit go) or a small GCP VM. The first pass is a full pass for 12 tenants (hours of read-only
  requests). The laptop is not a place for this.
* Add a daily Dataform workflow config on the `production` release config, timed after vsv2's daily
  tier. A full Dataform run is 2 to 4 minutes.
* Add an open-document re-read to the daily tier: fetch again every document whose status is still open
  (customer and supplier invoices, purchase and sales orders). A payment can change a balance without
  moving `lastModifiedDateTime`, so a watermark alone leaves open balances stale until the weekly full
  pass. The open set is small (about 950 customer invoices for the largest tenant). Not built.
* Point the currency service at `aquatiq-datavarehus` and load the Excel workbook there, so the two
  snapshot inputs stop being snapshots.
* Housekeeping while the scheduler is still stopped: commit the `dataform/` project, the scripts and
  these docs; decide whether `dw_staging_visma` becomes `visma_staging` (cheap now, a copy and one env
  setting).

### 2. The gates

**Status 2026-10-13: all built, run once on the snapshot.** B is `scripts/verify_population_vs_visma.py`,
C is the assertion set in `visma_assertions`, A is `dataform/tools/parity_gate.py`; two more were added
because the first run showed what the three could not see: gate-e (every ERP list endpoint, Visma's total
against the live rows of staging, all 12 tenants) and gate-s (sales orders per company).
`scripts/run_gates.py` runs all five and writes `docs/gates/RESULTS.md`. Results and the rules each gate
applies: `docs/gates/README.md`, PRODUCTION_READINESS.md. Gates B, C, E and S compare with Visma and
pass on the snapshot (B fails only on the age of the data); A (parity with v1) is red, 539 of 579
findings unexplained on 2026-10-13, and cannot be made green while v1 and v2 hold feeds of different
ages (`docs/gates/README.md`, "gate-a"). What remains is running them on fresh data every day for two
weeks, and giving A's remaining findings their allow-list entries then.

Three checks, run after every Dataform run, failing loudly. The first exists; the other two are what
would have caught findings 1 and 2.

* **A. Parity with v1.** Turn the four comparison tools (`dataform/tools/compare_*.py`) into one check:
  row counts, column fill, measure totals and bronze record by record, each with a tolerance, and an
  allow-list of the differences where v2 is right, each with its evidence (stale balances, GL balances,
  historical tenants, blank-tenant currencies, duplicate rows, the findings above). Anything not on the
  list is a finding to resolve, not to tolerate.
* **B. Population check against live Visma.** Per tenant, the count and balance of open customer
  invoices, supplier invoices and purchase orders in silver against Visma's own list for the open status.
  Zero tolerance except documents modified after the snapshot time. Extend
  `scripts/verify_bq_vs_visma.py`, which today samples a few rows per tenant by key and would find a
  population gap only by luck.
* **C. Ledger integrity**, as assertions in `visma_assertions` so the run fails: every GL batch nets to
  zero; every company-period nets to zero; for journal batches the line sums equal the journal header's
  `debitTotal` and `creditTotal` (Visma's control totals, as in batch 014427). It needs no API call.

Target: A, B and C green on fresh data for two weeks before anything downstream moves.

### 3. Decide and fix the logic gaps (each needs the user)

**Status 2026-10-13: decided and fixed, all four** (Dev workspace; evidence in `dataform_validation.md`,
"Fixed 2026-10-10" and "Fixed 2026-10-13"). The proposals in the table were taken further where the
finance workbooks and the ledger gave evidence: the merger-move flag is a seed rule (account 3999/4999,
or opening-balance wording together with the other lines of the same journal batch) that also finds
Hygiene Systems' 2023 opening balances and the move to Visma.net of AFF and Consult, and it leaves the
year-end accruals and valuations as activity; the department is the segment called "Avdeling" on every
tenant, not segment 3; the margin is computed on costed lines only (`has_cost`, with the 68 charge
products of `Omkostninger.xlsx` marked); the project-transaction grain was fixed on 2026-10-10 in the
raw staging key. Not yet recorded back in the v1 doc.

These change results someone reads, so each is a decision, made once, in v2's copy of the logic and
recorded back in the v1 doc.

| Gap | Today | Proposed | Moves |
|---|---|---|---|
| Merger-move flag | `debit_account_cd IN ('3999','4999')` | also flag module GL lines dated 2025-12-31 whose description starts "IB prosjektsaldo" (195 lines for Aquatiq AS: income 24.9 M, cost 24.8 M). Decide on "Periodisert inntekt / kostnad" (17 lines: 4.84 M / -0.6 M) and on Hygiene Systems' "Prosjektvurdering" reversals (-6.59 M / -7.85 M) | historical income and cost in `BI_ProjectPortfolioPnL`, so the P&L of the 25 `HS-` projects |
| Project-transaction grain | one record per (tenant, module, refNbr); 644 records dropped per run, a different set each time | keep `_scan_period` in bronze and in the rule (the staging key) | historical tenants only; 0 for active tenants |
| Sales cost and margin | margin over all lines; 39 % of value sits on lines with cost 0 | compute margin only on costed line types (GoodsForInventory) and add a `has_cost` flag, or define a cost for the other two types | margin measures in `BI_SalesOrder` |
| Segment meaning | department falls back to segment 3 in every tenant | a per-tenant segment map from `subaccounts.segments[].segmentDescription`, used for ic, department and project | department columns for tenants where segment 3 is not a department |

### 4. Full coverage in bronze

**Status 2026-10-13: not started, and not needed for the cutover.** Every v1 table is covered (74
bronze tables). The 48 endpoints below are new data, so they follow the first live month; the order
and the DataMart-first choice are still the open decision 7.

Build the 48 unbuilt endpoints that have rows, so that every staging table has a typed table:

* the 32 DataMart endpoints as `visma_bronze.visma_dm_*` (typed from the staging profile; the
  tombstone flag `deletedDatabaseRecord` becomes `isDeleted`);
* the 14 ERP endpoints without a v1 table (`project_tasks`, `customer_balances_v2`,
  `supplier_balances_v2`, `supplier_classes`, `discounts`, ...) as `visma_*`;
* the 2 Expense endpoints (`visma_expense_receipts`, `visma_expense_transactions`).

The generator already does this for the v1 tables; the extension is a type-inference rule for tables
with no v1 contract. Coverage becomes a gate too: 135 of 135.

### 5. Supplement the core with DataMart, where it is proven to add something

Each item is a new silver model or additive columns; the v1 model it extends is left as it is, and the
gold dimension gets the extra rows with a `source` flag:

* contacts: the 25,055 business-account contacts ERP hides (name, phone, email), `dm_contacts`;
* sales prices: the 2,048 expired and superseded prices, `dm_sales_prices`;
* supplier payments (31 unreleased), locations (113 of accounts that are neither customer nor
  supplier), addresses;
* journal transaction lines: the 39 fields only DataMart carries;
* new domains with no v1 counterpart: Expense (receipts, transactions), customer and supplier
  balances, project tasks, DataMart's historical AR.

Order by what reporting needs first; contacts and sales prices are the two with measured weight.

### 6. The structured gold and datamart layers, then the cutover

**Status 2026-10-13:** the cutover conditions and the order of the steps are in PRODUCTION_READINESS.md.
The new star schema waits for phases 4 and 5. The key changes that Power BI will see on the switch
(`item_key`, `location_key`, `fixed_asset_key`, `purchase_order_key` re-keyed, the GL balance
`business_unit_key` STRING to INT64) are listed there too.

* Design the star schema for Power BI on top of the full bronze: conformed dimensions
  (`dim_contact`, `dim_address`, ... beside the existing ones) and the facts the new domains give.
  `visma_datamart` keeps the five v1 tables with their contract; new tables come beside them.
* Switch Power BI to `aquatiq-datavarehus` only when all of this holds: gates A, B and C green for two
  weeks on fresh data; the four logic gaps decided; every difference to v1 on the allow-list with
  evidence. Keep `prj-dw-dev` and v1's sync running as the fallback through the first month.
* Then retire v1: stop its sync, archive the project, and rotate the service-account key that is
  committed in the `Datawarehouse` repository (`memory: visma-v1-dataform-committed-key`). The rotation
  itself does not have to wait for the cutover.

## Decisions needed

Status 2026-10-13 at the start of each line (the original wording follows it).

1. **Open.** Where the vsv2 scheduler runs (VPS beside v1, or GCP). Recommendation in
   PRODUCTION_READINESS.md: a small GCP VM with an attached service account (no key file to guard).
2. **Done, as far as it goes.** Lifting "do not commit" for the `dataform/` project, scripts and docs:
   committed on a local branch, not pushed (the user's instruction: do the work locally, commit, do not
   push). Dataform `main` stays as it is until go-live.
3. **Done.** The gate's rule: where v1 and v2 differ and Visma confirms v2, v2 wins (so "keep v1's
   results" is "keep v1's logic", not "copy v1's numbers").
4. **Done.** The four logic gaps in phase 3, above all the rule for the merger-move flag (decided from the
   finance workbooks and the ledger).
5. **Done.** Go-ahead to build gates B and C (SQL assertions and an extension of an existing script; no
   change to v1) and the open-document re-read; built, together with gate-e and gate-s.
6. Decided 2026-10-09: v1 is not repaired (no interim refresh of its feed). All fixes go into v2; v1 stays
   frozen until the cutover (`../../docs/V1_STATUS_AND_ROADMAP.md`).
7. **Open, after go-live.** Whether to build the 48 endpoints (phase 4) and whether `dw_staging_visma` is
   renamed (left as it is: a rename is a copy and one setting, and can be done at any quiet moment).
8. Done 2026-10-10 (Dev workspace, approved): silver sales orders are keyed by (tenant, order type,
   order id). `fact_sales_order` had kept one row per (tenant, orderId), so a quote and the order made
   from it collapsed to one row. Now 27,377 headers and 90,161 lines, every staged order and line
   (was 26,410 and 85,980). Restored in `BI_SalesOrder` for Aquatiq AS: 259 sales orders in 2023
   (10.7M NOK) and 159 in 2024 (3.5M), plus 125 quotes in 2026 (4.5M). Freight is now joined on order
   type too, so it is not counted on both the quote and the order. Uniqueness assertions on the new
   keys, and gate-s (`scripts/verify_sales_order_counts.py`) checks Visma's order count per company
   against silver: 12 of 12 pass, and the old rule fails for the 3 affected companies. Not on Dataform
   `main` yet: a run from `main` would bring the old rule back until it is committed (decision 2).
   v1 has the same SQL and stays as it is. `memory: visma-sales-order-number-collision`.
