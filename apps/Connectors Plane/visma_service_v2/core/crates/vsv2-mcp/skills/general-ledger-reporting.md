---
id: general-ledger-reporting
title: General ledger reporting (P&L, balance sheet, trends)
description: Which of the three GL sources to use for which report, their different sign conventions and known gaps, and validated queries for monthly P&L, closing balances and reconciliation between sources.
tags: [finance, gl, reporting, profit-loss, balance-sheet, warehouse]
tools: [warehouse_query, warehouse_describe_table]
prerequisites: [warehouse-sql-basics, multi-currency, tenants-and-entities]
difficulty: advanced
steps: 5
---
# General ledger reporting

Three tables describe the ledger. They are **not interchangeable**: different grain, different signs, and totals
that agree except in the newest periods (fact 6). Choose deliberately and name your choice in the answer.

| Source | Grain | Sign convention | Currency columns | Use it for |
|---|---|---|---|---|
| `visma_datamart.BI_FinanceTable` | one row per ledger line, with reporting categories `fsli_1..3` | **debit minus credit**: revenue is *negative*, costs positive | `amount_local_currency` (base), `amount_nok` | Management P&L by category, NOK, trends |
| `visma_gold_finance.fact_general_ledger` | one row per ledger line | `debit_amount`, `credit_amount` kept apart | base, `*_currency`, `*_market_nok` | Drill-down to the entries behind a number |
| `visma_gold_finance.fact_general_ledger_balance` | account x subaccount x period | **natural sign**: income and liabilities are *credit-positive*, assets and expenses debit-positive | base, `*_currency`, `*_market_nok` | The ERP's own balances (closing balance, trial balance) |

## Facts you must respect

1. **Filters**: `balance_type = 'Actual'` and `ledger = '1'` on the balance table; periods are `'YYYYMM'` strings.
2. **Never use `ptd_debit` / `ptd_credit`** in the balance table: they are NULL on 81% of the rows (filled on
   87,585 of 450,798). Period movement is the **difference of `ytd_balance`** between consecutive periods (YTD of
   the first period is the movement for January).
3. **P&L balances restart at zero every January** (balance sheet accounts carry over). A YTD balance is a
   year-to-date figure, not a cumulative one.
4. **Classify through `dim_account`.** `fact_general_ledger.account_type` is filled on every row and agrees with
   `dim_account`, but `bs_pl` and the reporting categories `fsli_1..3` exist only there. Join `dim_account` on
   `account_key` (every row matches) and use `a.bs_pl` (`'P&L'` or `'BS'`), `a.account_type`, `a.fsli_1`.
5. **Personnel costs have two spellings** in `fsli_1`: `'Personnel expenses'` *and* `'Personnell expenses'`
   (double n). Include both, or you drop most of them. List `fsli_1` values before you filter on them.
6. **The two feeds can differ in the newest periods.** `BI_FinanceTable` follows the transaction GL; the balance
   table is the ERP's own balance. Checked 2026-10-12 for P&L accounts in all 12 companies, they agree to within
   one unit of currency in every period through 202608 (Aquatiq AB, 2025 income: 9,442,751 SEK in both). For Aquatiq AS they differ in
   202609 and 202610 (income by 0.07% and 0.10%): the balances were read about 1-5 hours before the ledger lines,
   which fits. For the newest periods name the basis, and show both when it matters (query below).
7. **Future periods exist** (postings up to 202612). Cap by period when you mean "to date".
8. **Business units**: totals per company group by `tenant_id`; the GL tables also carry `entity_code` and
   `business_unit_name` (skill `tenants-and-entities`).

## Reports

### Categories and their exact spellings

```tool-call
warehouse_query
{"sql": "SELECT fsli_1, COUNT(*) AS ledger_lines FROM `aquatiq-datavarehus.visma_datamart.BI_FinanceTable` WHERE tenant_id = '2b6e5cd6-6ce0-11ee-970e-025417856183' AND financial_year = 2025 GROUP BY fsli_1 ORDER BY fsli_1"}
```

### Monthly P&L in NOK (revenue shown positive)

`amount_nok` is debit minus credit, so revenue is multiplied by -1.

```tool-call
warehouse_query
{"sql": "SELECT financial_period, ROUND(-SUM(IF(fsli_1 = 'Revenue', amount_nok, 0))) AS revenue_nok, ROUND(SUM(IF(fsli_1 = 'Cost of goods sold', amount_nok, 0))) AS cost_of_goods_nok, ROUND(SUM(IF(fsli_1 IN ('Personnel expenses', 'Personnell expenses'), amount_nok, 0))) AS personnel_nok, ROUND(SUM(IF(fsli_1 = 'Other operating expenses', amount_nok, 0))) AS other_operating_nok, ROUND(SUM(IF(fsli_1 = 'D&A', amount_nok, 0))) AS depreciation_nok, ROUND(-SUM(IF(fsli_1 IN ('Revenue', 'Cost of goods sold', 'Personnel expenses', 'Personnell expenses', 'Other operating expenses', 'D&A'), amount_nok, 0))) AS result_before_financial_items_nok FROM `aquatiq-datavarehus.visma_datamart.BI_FinanceTable` WHERE tenant_id = '2b6e5cd6-6ce0-11ee-970e-025417856183' AND financial_year = 2025 GROUP BY financial_period ORDER BY financial_period"}
```

### Closing balances at a period end (the ERP's balances)

Natural sign: assets and expenses are positive when debit, liabilities and income positive when credit. Do not add
assets to liabilities.

```tool-call
warehouse_query
{"sql": "SELECT a.bs_pl, a.account_type, COUNT(*) AS balance_rows, ROUND(SUM(b.ytd_balance)) AS closing_balance_base_currency FROM `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger_balance` b JOIN `aquatiq-datavarehus.visma_gold_finance.dim_account` a USING (account_key) WHERE b.tenant_id = '2b6e5cd6-6ce0-11ee-970e-025417856183' AND b.balance_type = 'Actual' AND b.ledger = '1' AND b.financial_period = '202509' GROUP BY a.bs_pl, a.account_type ORDER BY a.bs_pl, a.account_type"}
```

### Monthly movement from YTD balances (P&L accounts)

January restarts the year, so its movement equals its YTD; every other month is the YTD difference.

```tool-call
warehouse_query
{"sql": "WITH balances AS (SELECT b.financial_period, a.account_type, SUM(b.ytd_balance) AS ytd FROM `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger_balance` b JOIN `aquatiq-datavarehus.visma_gold_finance.dim_account` a USING (account_key) WHERE b.tenant_id = '2b6e5cd6-6ce0-11ee-970e-025417856183' AND b.balance_type = 'Actual' AND b.ledger = '1' AND a.bs_pl = 'P&L' AND b.financial_period BETWEEN '202501' AND '202512' GROUP BY b.financial_period, a.account_type) SELECT financial_period, account_type, ROUND(ytd) AS ytd_balance, ROUND(ytd - IF(RIGHT(financial_period, 2) = '01', 0, LAG(ytd) OVER (PARTITION BY account_type ORDER BY financial_period))) AS month_movement FROM balances ORDER BY account_type, financial_period"}
```

(A CTE must not share its name with one of its columns: BigQuery reads `ytd` as the table alias, a struct, and the query fails.)

### Do the sources agree? (report the gap, do not hide it)

Year-to-date P&L per account type from the balance table against the sum of the transaction lines, both in the
natural sign. Aquatiq AS to the newest period (202610) shows the small gap of fact 6; swap in a closed year (for
example Aquatiq AB, balance period `202512`, lines `202501` to `202512`) and the two agree.

```tool-call
warehouse_query
{"sql": "WITH bal AS (SELECT a.account_type, SUM(b.ytd_balance) AS balance_ytd FROM `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger_balance` b JOIN `aquatiq-datavarehus.visma_gold_finance.dim_account` a USING (account_key) WHERE b.tenant_id = '33f319b9-4d57-11ee-960c-025417856183' AND b.balance_type = 'Actual' AND b.ledger = '1' AND a.bs_pl = 'P&L' AND b.financial_period = '202610' GROUP BY a.account_type), gl AS (SELECT a.account_type, SUM(IF(a.account_type IN ('Income', 'Liability'), g.credit_amount - g.debit_amount, g.debit_amount - g.credit_amount)) AS transactions_total FROM `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger` g JOIN `aquatiq-datavarehus.visma_gold_finance.dim_account` a USING (account_key) WHERE g.tenant_id = '33f319b9-4d57-11ee-960c-025417856183' AND g.financial_period BETWEEN '202601' AND '202610' AND a.bs_pl = 'P&L' GROUP BY a.account_type) SELECT bal.account_type, ROUND(bal.balance_ytd) AS balance_table, ROUND(gl.transactions_total) AS transaction_lines, ROUND(bal.balance_ytd - gl.transactions_total) AS difference, ROUND(100 * SAFE_DIVIDE(bal.balance_ytd - gl.transactions_total, gl.transactions_total), 2) AS difference_pct FROM bal JOIN gl USING (account_type) ORDER BY bal.account_type"}
```

### The entries behind a number (drill-down with account names)

```tool-call
warehouse_query
{"sql": "SELECT g.account_cd, a.account_description, a.fsli_1, ROUND(SUM(g.debit_amount - g.credit_amount)) AS net_debit_minus_credit, COUNT(*) AS lines FROM `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger` g JOIN `aquatiq-datavarehus.visma_gold_finance.dim_account` a USING (account_key) WHERE g.tenant_id = '2b6e5cd6-6ce0-11ee-970e-025417856183' AND g.financial_period = '202509' GROUP BY g.account_cd, a.account_description, a.fsli_1 ORDER BY ABS(SUM(g.debit_amount - g.credit_amount)) DESC LIMIT 15", "max_rows": 15}
```

## Before you answer

State: company, period, which of the three sources, the sign convention you presented (convert to the usual
"revenue positive" form and say so), the currency (base or NOK at market rate), and, if you compared sources, the
difference you found. Do not call a figure "the official report" unless it came from the ERP's own balances and
the user's report definition matches.
