# The Dataform project against v1

The Dataform project in `dataform/` was run on the data in `dw_staging_visma` and every table it
builds was compared with the same table in v1 (`prj-dw-dev`, `dw_1_...`). This is what the four
checks found. Run on 2026-10-09; the staging data is the load of 2026-10-01/02, so v1, which kept
syncing, is a week ahead.

| Check | Tool | Report |
|---|---|---|
| Row counts of every table and view | `dataform/tools/compare_with_v1.py` | `dataform_v1_comparison.md` |
| How full each column is | `dataform/tools/compare_columns.py` | `dataform_v1_columns.md` |
| Totals of the money and quantity columns | `dataform/tools/compare_values.py` | `dataform_v1_values.md` |
| Bronze, record by record, column by column | `dataform/tools/compare_bronze_rows.py` | `dataform_v1_bronze_rows.md` |

A successful run proves little: all 214 tables built the first time, with the order number, order
totals and addresses of every sales order empty. The column and total checks are what found the
three bronze mappings below.

## What the checks led to (fixed)

1. **`visma_customers`** lacked 13 columns (address, postal code, country, customer class, sales
   accounts). v1 merged the Sales Order API customer list into the ERP list; bronze now does the
   same, on customer number.
2. **`visma_inventory_items`**: the item attributes (`WEBVARE`, `LAGLOK*`, ...) and availability per
   warehouse came from the Sales Order API too, and as `{id: value}`, not the ERP's `[{id, value}]`.
   Attributes are now converted; availability is joined on the item number.
3. **`visma_sales_orders_v3`**: v1 added the Sales Order v3 data a second time under the names of
   the retired v2 API (`orderNo`, `canceled`, `soBillingAddress`, `vatTaxableTotal`, 58 columns).
   Silver reads some of them only under those names, so the order number, order type, totals,
   addresses and `last_modified_at` of every sales order were empty. They are now built from the
   nested v3 fields. This also brought `shipment_freight_amount` back.

4. **`fact_sales_order` and `fact_sales_order_line`** (2026-10-10): the grain was one row per
   (tenant, orderId), but Visma numbers orders per order type, so a quote and the order confirmed
   from it share a number, and silver kept only the one changed last. 967 orders were dropped (Aquatiq
   AS: 418 sales orders from 2023-2024; Hygiene Systems 404). The grain is now (tenant, order type,
   order id) through silver, gold (`sales_order_key`) and `BI_SalesOrder`; freight joins on the type
   too. v1 has the same rule, so the comparison with v1 could not show this; Visma's order counts
   did (`scripts/verify_sales_order_counts.py`).

After that, 110 of the 140 silver, gold and datamart tables are within 5 % of v1's row count and
the columns that are emptier than in v1 are in `dim_inventory_item` (and its gold copies), where
about a fifth of the items have no Sales Order counterpart, and in a few price class reports.

## Where v2 differs from v1, and why v2 is right

* **Four historical tenants** (Hygiene Systems, Food Factory, Sense, Consult). v1 stopped syncing
  them in 2025 and holds a handful of rows; v2 fetched their whole history. This is most of the
  difference in supplier invoices (v1 186 against 5,821 for Hygiene Systems), supplier payments,
  GL balances and inventory. For the active tenants the supplier invoices, lines and payments, the GL balances and the
  customers are within 4 %.
* **General ledger balances** (`fact_general_ledger_balance`): v1 has period debit 0 for many
  accounts that have postings. For the pilot in period 202506, debit on the ledger transactions is
  136.4 M; for account 2920 the transactions and v2 say 25,000,001 and v1 says 0. v1's incremental
  fetch missed balances that changed without their timestamp moving; v2 walks every period.
  Period debit and credit sum to 1.4 to 3.4 times v1's for that reason.
* **Open balances** (`balance` on customer invoices, `open_balance` on purchase orders, ...): v1 is
  stale, because paying an invoice does not change the invoice's `lastModifiedDateTime`. Checked
  against Visma itself: customer invoices 018684, 150026, 018178, 018566, 018977 and 017087 are 0 in
  Visma and in v2 and between 2 thousand and 300 thousand in v1; purchase orders 001658, 001771, 001813, 001702 and
  001674 the same. The supplier invoice difference is the week's new, still unpaid invoices: among
  records v1 has not changed since 10-01 there is none where the two disagree.
* **Ledger batches** (`fact_general_ledger`): v1 has 1,072 batches whose debit and credit differ and 246
  company-periods that do not net to zero; v2 has none, in any of the 12 tenants. Batch 014427 (Aquatiq
  AS, 2024-08): v1 holds 4 of its 10 lines, v2 holds all 10 and matches Visma's header (1,005,885.78 on
  each side). A ledger nets to zero, so v1's lines are the incomplete ones (see `v2_roadmap.md`).
* **Open customer invoices, Aquatiq AS**: v1 has 232 invoices (8.07 M NOK) as Open that Visma and v2 have
  as Closed. Their status changes fall between 2026-03-25 and 2026-04-15 and v1 never read them.
* **Inventory items**: the pilot has 7,257 items in v2 and 5,828 in v1; 1,444 are only in v2
  (1,135 active). The Sales Order API lists 5,812 of them; v1 only had those.
* **Currencies**: v1 has 11 currency rows with an empty tenant id; v2 has none.
* **Gap and quality reports** (`source_gap_*`, `qa_gl_project_gap_ap`): far fewer rows than v1's
  (35 thousand against 712 thousand) because they were inflated by v1's duplicate bronze rows.
* **Bronze, record by record**: for 67 tables, fields that differ are mostly NULL in v1 and filled
  in v2 (supplier `corporateId`, contact phone numbers, account `taxCategory`, shipment freight).
  The JSON columns (`lines`, `taxDetails`, ...) show as different only because v1 wrote
  `0.0` where v2 writes `0`.
* **`begBalance` / `endingBalance` on general ledger lines** are computed by the API for each
  request and differ between v1's and v2's fetches (11 % equal). Nothing measures from them;
  use `fact_general_ledger_balance`.
* The sum of `BI_FinanceTable.amount_nok` is about 0 (-1,535) in v2 and 15.0 M in v1. A ledger
  nets to 0, so v1's is the one that is off.

## Left as it was

* `lines` and `shipments` of `visma_sales_orders_v3` are NULL: they are the retired API's line
  list; silver takes `orderLines` first. `visma_sales_orders_v2` is not built.
* v2 payload fields v1 never had are added to bronze but no silver model reads them.
* The remaining total differences of 2 to 3 % on the sales and supplier facts are the week of
  new transactions in v1. Rerun the three comparison tools after the next staging refresh to see
  them close.

## Fixed 2026-10-10: project transactions dropped lines

`fact_project_transaction_line` (silver, and `BI_ProjectPortfolioPnL` built on it) kept one record per
`(tenant, module, refNbr)`:

```sql
ROW_NUMBER() OVER (PARTITION BY tenantId, module, refNbr
                   ORDER BY refreshedAt DESC, firstSyncedAt DESC, syncedAt DESC)
```

* Staging has 38,843 project transaction records, unique per `(module, refNbr, _scan_period)`. A
  document that has lines in several periods is fetched once per period, each time with only that
  period's lines: 205 documents, all in the four historical tenants (`ModulePM` 1403823 is one
  allocation with lines in each of 16 periods).
* Bronze dropped `_scan_period`, so those records looked like duplicates. Their timestamps tie, so the
  rule kept an arbitrary one per document: 644 records and 11,095 lines were dropped, a different set
  each run (`fact_project_transaction_line` gave 198,791 rows in one run and 197,805 in the next from
  identical inputs). Sense lost 9,997 of its 47,851 lines, Consult 807, Hygiene Systems 263, Food
  Factory 28; the active tenants lost none.
* Fix: bronze keeps the period as `scanPeriod` (the generator does it for any endpoint whose payload has
  `_scan_period`), and the silver line model keeps one record per `(document, period)`. Line identity is
  `tran_id`: present on all 209,886 lines, never in more than one period, never shared between documents.
  The header model stays one row per document (38,199) and now aggregates every period's lines.
* Result: 209,886 lines, exactly what staging holds, and the same on a second build. Income is
  unchanged; the restored lines add 14.2 M of expense (historical tenants, base currencies, not
  converted). `fact_general_ledger` keeps its 1,108,577 rows; PM lines with a project rise by 318.
* Guard: `assert_project_transaction_lines_complete` compares the lines in bronze with silver per tenant
  (it flags 11,095 on the old table), and `assert_sales_order_rows_complete` does the same for sales
  orders (it flags 561, 404 and 2 headers on the old rule).
* v1 has the same rule and is not changed.


## Fixed 2026-10-13: the logic gaps of the audit, from the workbooks and the ledger itself

The four logic gaps of `v2_roadmap.md` (merger move, sales margin, segment meaning, journal totals) were
closed with the finance workbooks (`dataform/inputs/excel/`) and the warehouse as evidence. Row counts and
money totals of every table below are identical before and after; only flags, business units and
department fields change. Each change has a guard in `visma_assertions` or `visma_silver_common`.

### Project history: the merger move and the migrated opening balances

`fact_project_portfolio_line` flagged a project line as "historical" when its account was 3999 or 4999,
and `BI_ProjectPortfolioPnL` built `historical_*` and `*_excluding_historical_*` from that. The accounts are
named "Historisk inntekt prosjekt" and "Historisk utgift/prosjektkostnad" in the charts of accounts of
Aquatiq AS, AFF and Consult. They hold project history that was migrated, and finance booked the same kind of
balance on ordinary accounts in two cases the flag missed. The rule is now a seed,
`visma_silver_excel.seed_project_history_rule`, read by `fact_project_transaction_line`:

| Rule | Matches | Why |
|---|---|---|
| `historical_account` | debit account 3999 or 4999 | the accounts' names |
| `opening_balance_wording` | module GL, text with "IB prosjektsaldo", "inngående saldo" or "før systembytte"; **and the other lines of the same journal batch** | "IB" is *inngående balanse*, an opening balance |

What the rule finds, one row per tenant, date and rule in `visma_silver_common.qa_project_carry_over_events`:

| Event | Tenant, date | Flagged lines | Income / cost (NOK) |
|---|---|---|---|
| Move to Visma.net ("før systembytte") | AFF 2023-09 (one line of the 11th, the rest the 30th) | 64 | 59.8 M / 45.7 M |
| | Consult 2023-09-30 (and a 2024 periodisation pair that nets to 0) | 1,518 | 13.3 M / 9.8 M |
| | AHS 2023-09-30 ("Inngående saldo prosjekt AHS", batches 000915 and 001054-001056) | 119 | 43.5 M / 35.5 M |
| Merger into Aquatiq AS | AS 2025-12-31, accounts 3999/4999 ("AFF klient") | 21 | 40.4 M / 36.5 M |
| | AS 2025-12-31, "IB prosjektsaldo AHS pr 31.12.25" on ordinary accounts | 195 | 24.9 M / 24.8 M |

* The merger move was 233 lines of Aquatiq AS on 25 `HS-` projects (70.1 M income, 60.6 M cost). Account 3999/4999
  caught 21 of them; the "IB" lines were the largest part. The GL batches of the IB lines net to zero on every
  account (debit and credit of the same amount): they only put the amounts on the project, so they are
  project history and not activity of 2025 or 2026.
* AHS had the same problem in 2023: 43.5 M of opening income and 35.5 M of opening cost on ordinary accounts,
  which the flag never covered. Its corrections ("Reversering av ...") sit in sibling lines of the same batches,
  which is why the rule takes batch companions for wording matches: flagging only the first half would have
  left −43.5 M of income in the activity.
* **Left as activity on purpose:** the 17 "Periodisert inntekt/kostnad AFF klient" lines of 2025-12-31 (4.84 M
  income, −0.60 M cost). They share a batch with account-rule lines, but Aquatiq AS reverses each of them in
  2026 on the same project (net exactly 0 on all ten projects), so they are year-end accruals. The batch
  companion rule applies to the wording rule only for this reason. Hygiene Systems' "Prosjektvurdering"
  entries (valuations) are activity too: Aquatiq AS books +6.59 M income and +7.85 M cost in 2026, the
  exact negative of AHS's entries of 2025-12-31.
* Effect on `BI_ProjectPortfolioPnL` (historical income / cost; income / cost without history):

  | Tenant | Before | After |
  |---|---|---|
  | Aquatiq AS | 40.4 M / 36.5 M; 140.8 M / 133.5 M | 65.3 M / 61.2 M; 115.9 M / 108.8 M |
  | Hygiene Systems | 0 / 0; 173.3 M / 150.4 M | 43.5 M / 35.5 M; 129.7 M / 114.8 M |
  | AFF | 65.7 M / 43.3 M; 93.9 M / 78.9 M | 59.8 M / 45.7 M; 99.8 M / 76.5 M |
  | Consult | unchanged | unchanged |

  Income, cost and line counts of every table are the same before and after.
* **Business unit of the moved lines.** Before 2026 the tenant of Aquatiq AS stands for Chemistry in
  `business_unit_mapping`, and segment 2 of its subaccounts was a department. The merger move was posted on
  2025-12-31 with the new structure (segment 2 = business unit; all 233 lines carry 2), so the date window gave all
  of it to Chemistry and left every moved project with two business units ("Mixed" in Power BI). On the day before a
  tenant's structure changes, an exact segment-2 match of the new structure now wins
  (`business_unit_match_rule = cutover_day_segment2_match`). 70.1 M income and 60.6 M cost move from Chemistry to
  Hygiene Systems; 23 of the 25 projects have one business unit (the other two have zero-value lines with segment
  2 = 0), and the projects with mixed units in Aquatiq AS fall from 88 to 65.
* Guards: `assert_cutover_day_project_lines_resolve_by_segment2` (a cutover-day line that does not match the new
  structure is a new kind of posting), and `qa_project_carry_over_events` (a new row is a new carry-over event
  that reached the project P&L as history, or a posting that matched the wording by accident).
* Not changed: the warehouse calls business unit 02 "AHS" and the workbook (`reference_data.xlsx`) calls it
  "Hygiene Systems". Power BI may depend on the label.

### Department: segment 3 only on the new structure

`BI_FinanceTable`, `BI_SalesOrder`, the project lines and the ledger took the department from subaccount
segment 3. That is right for the six tenants on the new structure (IC / BU / Avdeling: Aquatiq AS, AB, Aps,
Group, Food Automation, Sensilist) and wrong for the others, where segment 2 is the department and segment 3
is a project (AHS, Aqua Pharma, Aqua Pharma Group), a location (Consult) or not in use (AFF, Sense). Visma
names every segment (`segmentDescription`), so `dim_subaccount.department_code` is now the value of the segment
called "Avdeling", and the models read that. The check is `assert_department_segment_known`.

| | Before | After |
|---|---|---|
| Hygiene Systems, distinct departments in the finance table | 1 | 6 |
| AFF, Sense | 1 and 1 | 10 and 5 |
| Consult | 680 (location codes) | 28 |
| `BI_SalesOrder.department_name` | empty on every line | named on 98 to 100 % of the lines |

The department master of the old-structure tenants numbers departments '1'...'5' while the segment values are
'01'...'05', and names them differently, so `dim_department` now also holds the departments that exist as a
segment and not in the master (named by the segment). Every `department_key` of `BI_FinanceTable` resolves to a
row (it did not for the new codes), and every project line with a department has a name.

### Sales: charges and margin

Visma returns no cost price on `NonInventoryGoods` and `MiscCharge` lines (97.9 % and 99.4 % of them): 514 M of
1,320 M NOK. A margin over all lines is 71.4 %; over the lines that have a cost it is 53.1 %. `BI_SalesOrder`
now has `has_cost`, `margin_amount_nok` (empty without a cost), `amount_price_with_cost_nok` (margin % =
`SUM(margin_amount_nok) / SUM(amount_price_with_cost_nok)`) and `is_charge_line`. `Omkostninger.xlsx` lists 68
product numbers that are charges (freight, travel time, driving, lodging, diet, packaging, lubricant and
environmental fees): 9,336 lines, 11.6 M NOK. They explain only 2 % of the value without cost; the rest is services
and project deliveries that Visma costs elsewhere, so the margin of those lines is not on the sales order at all.
`sales_order_line_business_unit_strategy` named 12 of the 68 charge products; all 68 now follow the strategy (the
other 58 used to keep their own business unit in the header rollup): 948 more lines, 3.5 M NOK, and no change
in the business unit of any header.

### Excel inputs

`dataform/tools/excel_inputs.py check` compares `visma_bronze_excel` with the workbooks in
`dataform/inputs/excel/`; `load` replaces the tables from them. On 2026-10-13 the two snapshot tables equalled
the workbook exactly (6 and 113 rows), and `raw_cost_products` (68) was created from `Omkostninger.xlsx`.
`silver/excel/entity_mapping` gave six entities another company's tenant (AHS got Consult's, Consult got Aqua
Pharma's, ...): the workbook and `seed_entity_mapping` number the companies differently and the join was on the
company id. It joins on the entity code now. Nothing reads the table yet. The workbooks behind
`seed_company_account_mapping` and the two global account mappings ("Group mapping (kun NO, SE, DK ...).xlsx") are not in the
repository; the seeds are the only record of them.

### Journal batches that Visma's header and lines disagree on

`assert_journal_batch_control_totals` returned 49 posted AR and AP batches (Aquatiq AS 23, Hygiene Systems 18,
Aquatiq Group 5, Sense 2, Sensilist 1). In 47 the header totals agree and exceed the lines of the batch; the lines
balance. In 2 the header's own debit and credit differ. Against live Visma, the lines staged equal Visma's
`totalCount` for the period and module in the four periods checked (1,528, 1,557, 8,814 and 567 lines), so nothing is
lost on the way; Visma's header claims more than its listing returns. The 49 are recorded in
`qa_journal_batch_known_mismatch` with their differences, and the assertion leaves out a recorded batch only
while its difference is exactly the recorded one: a new mismatch, or a batch that changes, fails the run.
