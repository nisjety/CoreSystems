---
id: multi-currency
title: Currencies and NOK conversion
description: Which amount columns are in the company's base currency, the document currency or NOK, how the NOK conversion works and how to qualify it, and how to total across companies safely.
tags: [warehouse, currency, nok, finance]
tools: [warehouse_query, warehouse_describe_table]
prerequisites: [warehouse-sql-basics, tenants-and-entities]
difficulty: intermediate
steps: 3
---
# Currencies and NOK conversion

The group has three base currencies: **NOK** (most companies), **SEK** (Aquatiq AB) and **DKK** (Aquatiq Aps,
Aquatiq Food Automation ApS). Documents can also be in a foreign currency (USD, EUR, AUD, GBP, DKK, SEK...).
Adding amounts without checking which currency each column is in is the most common way to get a wrong total.

## Three kinds of amount column

| Column pattern | Currency | Example |
|---|---|---|
| no suffix | the **company's base currency** | `amount`, `balance`, `debit_amount`, `ytd_balance`, `income_amount` |
| `*_in_currency` / `*_currency` | the **document / transaction currency** | `balance_in_currency`, `debit_amount_currency` |
| `*_market_nok` | **NOK**, converted at the daily market rate | `balance_in_currency_market_nok`, `amount_nok`, `debit_amount_currency_market_nok` |

Check on open customer invoices: for a NOK company, a USD invoice shows `balance` (NOK at the booked rate),
`balance_in_currency` (USD) and `balance_in_currency_market_nok` (NOK at the market rate). The two NOK figures
are close but not identical, and for a SEK or DKK company `balance` is in SEK or DKK, not NOK.

## Rules

1. **Never sum a no-suffix column across companies.** It mixes NOK, SEK and DKK. For group totals use the
   `*_market_nok` column (or `amount_nok` in the BI models).
2. **Per company, the base currency is `dim_company.base_currency`**; group by company when you use no-suffix columns.
3. **NOK figures are market-rate approximations.** They are converted at a daily market rate, not the rate the
   company booked and not a group consolidation rate. Good for management overviews; say so, and do not call
   them statutory or consolidated figures.
4. **Qualify the rate**: `has_market_rate_to_nok` is false when no rate exists (the NOK figure is then missing or
   unreliable); `is_market_rate_filled_forward` is true when the day had no rate and the latest earlier rate was
   used. `market_exchange_rate_to_nok` and `market_rate_date` show the rate and its date. The rates are a copy of
   the currency feed: the newest real rate is 2026-10-09 (2026-10-08 for BRL and CLP; 15 currencies, from
   2023-01-02), and the daily table runs on to 2027-01-01 by filling the last rate forward, so a conversion dated
   after that uses the last real rate.
5. **Name the currency in every figure**: "NOK (market rate)", "SEK (base)", "USD (document)".

## Worked examples

Open customer invoices by document currency, with all three views of the amount:

```tool-call
warehouse_query
{"sql": "SELECT cu.currency_code, COUNT(*) AS invoices, ROUND(SUM(i.balance)) AS balance_base_currency, ROUND(SUM(i.balance_in_currency)) AS balance_document_currency, ROUND(SUM(i.balance_in_currency_market_nok)) AS balance_nok FROM `aquatiq-datavarehus.visma_gold_finance.fact_customer_invoice` i JOIN `aquatiq-datavarehus.visma_gold_common.dim_currency` cu USING (currency_key) WHERE i.status = 'Open' GROUP BY cu.currency_code ORDER BY invoices DESC"}
```

Is the NOK conversion trustworthy for this selection? Count rows by rate quality:

```tool-call
warehouse_query
{"sql": "SELECT has_market_rate_to_nok, is_market_rate_filled_forward, COUNT(*) AS invoices FROM `aquatiq-datavarehus.visma_gold_finance.fact_customer_invoice` WHERE status = 'Open' GROUP BY has_market_rate_to_nok, is_market_rate_filled_forward ORDER BY invoices DESC"}
```

Group-wide open receivables in NOK, per company (never the no-suffix `balance` here):

```tool-call
warehouse_query
{"sql": "SELECT c.entity_name, c.base_currency, ROUND(SUM(i.balance_in_currency_market_nok)) AS open_nok FROM `aquatiq-datavarehus.visma_gold_finance.fact_customer_invoice` i JOIN `aquatiq-datavarehus.visma_gold_common.dim_company` c USING (company_key) WHERE i.status = 'Open' GROUP BY c.entity_name, c.base_currency ORDER BY open_nok DESC"}
```

The rates themselves (daily to-NOK table), for checking a conversion by hand:

```tool-call
warehouse_query
{"sql": "SELECT calendar_date, from_currency_id, market_exchange_rate_to_nok, is_rate_filled_forward FROM `aquatiq-datavarehus.visma_gold_common.dim_currency_market_rate_to_nok_daily` WHERE from_currency_id = 'USD' AND calendar_date BETWEEN '2025-09-01' AND '2025-09-07' ORDER BY calendar_date"}
```
