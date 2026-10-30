# Visma Dataform project

The warehouse layers on top of the raw JSON v2 lands in BigQuery: typed **bronze**, **silver**,
**gold** and the **datamart** that Power BI reads. It is the v1 Visma Dataform project
(`apps/visma_service/Dataform`, deployed as `Datawarehouse` in `prj-dw-dev`) carried over to the
`aquatiq-datavarehus` project, with bronze rebuilt from `dw_staging_visma` instead of read from
tables v1's sync wrote.

```
Visma APIs ──vsv2──▶ dw_staging_visma (raw JSON, one table per endpoint)
                         │  bronze   typed per endpoint, same columns as v1's bronze
                         ▼
                    visma_bronze ──▶ visma_silver / _common / _excel ──▶ visma_gold_common / _finance / _sales
                                                                          └──▶ visma_datamart  (Power BI)
```

## Where it runs

| | |
|---|---|
| GCP project | `aquatiq-datavarehus` (BigQuery data in `europe-north2`) |
| Dataform repository | `Visma` ("Visma warehouse"), `europe-north1` (Dataform does not exist in europe-north2) |
| Runs as | `visma-dataform@aquatiq-datavarehus.iam.gserviceaccount.com` (jobUser on the project, writer on the layer datasets, reader on the inputs) |
| Workspace | `Dev` (the working copy in the Dataform UI) |
| Release config | `production`, compiles `main`, no schedule |
| Schedule | none: nothing runs by itself. Add a workflow config when staging is refreshed regularly |

The Xero repository in the same project is set up the same way; the two do not share anything.

## Layers and datasets

| Dataset | What | Objects |
|---|---|---|
| `dw_staging_visma` | raw JSON from vsv2, input only (the name is the vsv2 pipeline's setting) | 135 tables |
| `dw_staging_currency` | exchange rates, input only (snapshot, see below) | 2 tables, 2 views |
| `visma_bronze` | typed copy of v1's bronze tables, built from staging, plus `visma_price_classes` and `visma_tenants` | 74 tables |
| `visma_bronze_excel` | the finance workbooks as tables, input (`tools/excel_inputs.py`) | 3 tables |
| `visma_silver` | cleaned dimensions and facts | 45 tables, 8 views |
| `visma_silver_common` | cross-source bridges, quality and gap reports | 16 tables |
| `visma_silver_excel` | mapping seeds and rules from the workbooks | 19 tables |
| `visma_gold_common` | shared dimensions | 19 tables |
| `visma_gold_finance` | finance facts and dimensions | 17 tables, 9 views |
| `visma_gold_sales` | sales and purchasing facts and dimensions | 6 tables |
| `visma_datamart` | the Power BI layer: `BI_FinanceTable`, `BI_SalesOrder`, `BI_ProjectPortfolioPnL`, `BI_ProjectBudgetRecon`, `RLS_sales_person` | 5 tables (assertion output is never written here) |
| `visma_assertions` | assertion results (each returns no rows when it passes) | 40 views |

Names follow `visma_<layer>[_<area>]` so the Visma datasets sort together. Xero's datasets are named
`dw_<layer>_xero` and are not part of this project.

A full run builds 258 actions (218 tables and views, 40 assertions) and takes a few minutes. Dataset names are in the `schema` of
every definition; `tools/setup_gcp.py` creates the datasets.

## How bronze works

`definitions/bronze/visma/*.sqlx` read `payload` (JSON) from staging and give every v1 bronze
column back with its v1 name, order and type, so the silver and gold SQL written for v1 runs
unchanged. The helpers are in `includes/bronze.js`.

* The 13 v1 sync columns come from the staging row: `tenantId`, `syncedAt` (= `updated_at`),
  `firstSyncedAt`, `isDeleted`/`deletedAt` (= `deleted_at`), `organizationName` (from
  `visma_tenants`). The rest (`seenInFullScopeRuns`, `syncBatch`, ...) are NULL.
* Fields v2 fetches that v1 never had are added after the v1 columns.
* Three kinds of table are more than a field-for-field copy (all in `OVERRIDES`, `COLUMNS` and
  `RENAMES` of `tools/generate_bronze.py`):
  * `visma_customers` and `visma_inventory_items` merge the Sales Order API list into the ERP
    list (customer address and class, item availability), as v1 did.
  * `visma_sales_orders_v3` has v1's flattened columns (`orderNo`, `canceled`, `soBillingAddress`,
    the `totals`) built from the nested v3 payload.
  * `visma_price_classes` is derived from customers and customer locations
    (`definitions/bronze/visma/visma_price_classes.sqlx`, written by hand).
* A bronze table is rebuilt in full on every run (`type: "table"`); an incremental version can
  come when the run time matters.

Regenerate after staging changes (needs only the files in `tools/contract/`, or BigQuery for the
first step): `tools/profile_staging.py` then `tools/generate_bronze.py`. The bronze files are meant
to be edited by hand afterwards; running the generator overwrites them.

## Working on it

```bash
cd dataform
echo '{"projectId": "aquatiq-datavarehus", "location": "europe-north2"}' > .df-credentials.json  # git-ignored
dataform compile --timeout=180s          # syntax; does NOT check the SQL against BigQuery
dataform run --dry-run --tags=bronze     # checks every query (needs the upstream tables to exist)
dataform run --tags=bronze               # builds with your own gcloud login (ADC)

tools/deploy.py all                      # push to the Dev workspace, compile and run it in GCP
tools/deploy.py commit --message "..."   # put the project on main (a release config builds main)
```

A workspace in a standalone Dataform repository (no git remote) is a draft: `main` only moves with
`deploy.py commit`. Always confirm a change with a run, not only a compile.

| Tool | Does |
|---|---|
| `tools/setup_gcp.py` | creates the service account, datasets, access, repository and workspace; safe to run again |
| `tools/excel_inputs.py` | loads the finance workbooks of `inputs/excel/` into `visma_bronze_excel`, or checks that the tables still equal them (`check`, `load`) |
| `tools/copy_external_inputs.py` | one-time snapshot of the Excel tables and the currency rates from the old project (done 2026-10-09) |
| `tools/port_v1_definitions.py` | the copy of silver, gold and the Excel definitions from v1 with the new dataset names |
| `tools/profile_staging.py`, `tools/generate_bronze.py` | the bronze layer, see above |
| `tools/deploy.py` | push, compile, run, commit against the `Visma` repository |
| `tools/compare_with_v1.py`, `compare_columns.py`, `compare_values.py`, `compare_bronze_rows.py` | the checks against v1, see `docs/dataform_validation.md` |

## Inputs that are not Visma endpoints

* **The finance workbooks** (`inputs/excel/`): they carry business logic the warehouse follows.
  `reference_data.xlsx` (entity to business unit, the group chart of accounts) and `Omkostninger.xlsx` (the 68
  product numbers that are charges, not goods) are loaded into `visma_bronze_excel` by `tools/excel_inputs.py load`;
  `check` says whether the tables still equal the workbooks and which rows differ. Edit the workbook, `load`, run
  Dataform. The seeds in `definitions/silver/excel/` come from workbooks that are not in this folder:
  `Selgere 20260420.xlsx` and `RLS SalesOrder.xlsx` (people and their Power BI user names, so they stay where they
  are for now), and "Group mapping (kun NO, SE, DK ...).xlsx" (not in the repository; the seeds are its only record).
  `seed_project_history_rule` is the rule for carried-over project history, written from the ledger itself (see
  [dataform_validation.md](../docs/dataform_validation.md#fixed-2026-10-13-the-logic-gaps-of-the-audit-from-the-workbooks-and-the-ledger-itself)).
* **Currency rates** (`dw_staging_currency.Historyrates`, `Rate` and their PowerBI views): written by the
  currency service, an own server in the monorepo (`apps/currency_service`). Its BigQuery target is set by
  `GCP_PROJECT_ID`, `GCP_DATASET_ID` and `GCP_BQ_LOCATION`; with `aquatiq-datavarehus`, `dw_staging_currency` and
  `europe-north2` it writes the same tables, with the same rates, as it did into the old project (checked
  2026-10-12 on 372 rates: no difference). Until that is switched on the server, the copy here is the snapshot of
  2026-10-09.

## Not done

* **Coverage.** Bronze reads 74 of the 135 staging endpoints. Not built: all 38 DataMart endpoints
  (`dm_*`, 2.07 M rows in staging, 6 of them empty), 21 ERP endpoints with no v1 table (14 with rows,
  e.g. `project_tasks`, `customer_balances_v2`, `supplier_balances_v2`, `supplier_classes`,
  `discounts`, `languages`, `supplier_inbox_rules`), and 2 Expense endpoints (`expense_receipts`,
  `expense_transactions`). DataMart tables would go in `visma_bronze` as `visma_dm_*`, because
  `visma_datamart` is the Power BI layer.
* **Fixed 2026-10-10: project transactions.** The silver line model dropped 11,095 lines of the four
  historical tenants, a different set each run. It now keeps one record per period slice and holds every
  staged line. See [dataform_validation.md](../docs/dataform_validation.md#fixed-2026-10-10-project-transactions-dropped-lines).
* **Fixed 2026-10-13: the logic gaps of the audit** (merger move and migrated opening balances, the department
  that is segment 3 only on the new structure, sales margin over lines without cost, the journal batches Visma's
  own header and lines disagree on). Same section of [dataform_validation.md](../docs/dataform_validation.md).
  Open from there: the label of business unit 02 ("AHS" in the warehouse, "Hygiene Systems" in the workbook).
* **Checks that exist:** the assertions of `visma_assertions` (ledger batches and company-periods net to zero,
  every typed table holds what its bronze holds, journal control totals, cutover and department checks) and the
  gates of `scripts/run_gates.py` against live Visma ([docs/gates/README.md](../docs/gates/README.md)).
* Nothing is scheduled. After vsv2's scheduler refreshes staging, the workflow has to run again (a daily
  workflow config is the obvious start).
* `visma_sales_orders_v2` (the retired sales order API) is not built; nothing reads it.
* The check against v1 is in [dataform_validation.md](../docs/dataform_validation.md).
