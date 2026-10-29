---
id: tenants-and-entities
title: Companies, tenants, entities and business units
description: How a company is identified in Visma, in the live tools and in the warehouse, why entity codes differ between tables, and how merged companies and business units appear.
tags: [meta, tenants, entities, finance, warehouse]
tools: [visma_list_tenants, warehouse_query, visma_sync_status, visma_erp_query, visma_datamart_query]
primary: [visma_list_tenants]
prerequisites: [start-here]
difficulty: intermediate
steps: 3
---
# Companies, tenants, entities and business units

## The one stable key: `tenant_id`

Every Visma company is a **tenant**, identified by a UUID. Use it (or the exact company name) for the `tenant`
argument of the live tools, and as the join/filter key in SQL. `visma_list_tenants` returns, per tenant:
id, name, `status` (`active` or `historical`), `valid_to`, and the DataMart company id.

```tool-call
visma_list_tenants
```

- **Default tenant**: calls without a `tenant` argument use the one marked `is_default` (Aquatiq AS).
- **Historical tenants** were merged into another entity: `valid_to` is their last day with data
  (`2025-12-31` for Aquatiq Food Factory AS, Hygiene Systems AS, Sense AS and Consult AS, which became business
  units of Aquatiq AS). Their newest ledger postings are from 2026-02-09 and their open items, order backlog and
  period close flags are frozen there: leave them out of current receivables, payables and backlog (skills
  `receivables-payables`, `sales-reporting`). That is a merger, not a broken sync.
- **DataMart company id** (`datamart_company_id`) is what DataMart rows carry as `companyId`. It can be `null`
  in the tenant list for a tenant whose id is configured elsewhere (Aquatiq AS): ask the operator rather than
  guessing.
- **Base currency differs**: NOK for most companies, SEK for Aquatiq AB, DKK for Aquatiq Aps and Aquatiq Food
  Automation ApS. Never add a base-currency amount across companies (skill `multi-currency`).

## In the warehouse the vocabulary is not consistent

Three different things are called "entity" or "company". They do **not** share codes:

| Where | Column(s) | Example values |
|---|---|---|
| `visma_gold_common.dim_company` (and `dim_entity`) | `entity_code`, `entity_name`, `tenant_id` | `AQUATIQ_AS`, `CONSULT`, `AHS`, `AFF`, `APHARMA` |
| GL tables: `fact_general_ledger`, `fact_general_ledger_balance`, `BI_FinanceTable` | `entity_code`, `business_unit_name`, `group_name`, `tenant_id` | `AQUATIQ_AS` with several business units, `CONSULT_AS`, `AQUATIQ_HYGIENE_SYSTEMS_AS`, `CHEMISTRY` |
| Facts without a company column (invoices, payments, ...) | `company_key` only | join `dim_company` on `company_key` |

Consequences:

1. **Join on `tenant_id` or `company_key`, never on `entity_code`**: the same company has a different code in
   `dim_company` than in the GL tables (`CONSULT` vs `CONSULT_AS`).
2. **A tenant can have several business units.** From 2026 Aquatiq AS (tenant `33f319b9...`) is split into
   business units (AHS, Chemistry, Commercial, Consult, FT&A, Management & Shared Services, Properties, Aquatiq AS).
   Before 2026 `CHEMISTRY` was its own `entity_code` under the same tenant. For company totals group by
   `tenant_id`; for management reporting by business unit use `entity_code` with `business_unit_name` from the
   *same table*.
3. **Names for display** come from `dim_company.entity_name` (via `company_key`), not from the GL `entity_code`.

All companies with their base currency:

```tool-call
warehouse_query
{"sql": "SELECT tenant_id, entity_name, base_currency FROM `aquatiq-datavarehus.visma_gold_common.dim_company` ORDER BY entity_name"}
```

How one tenant is split into entity codes and business units over time:

```tool-call
warehouse_query
{"sql": "SELECT entity_code, business_unit_name, MIN(financial_period) AS first_period, MAX(financial_period) AS last_period FROM `aquatiq-datavarehus.visma_gold_finance.fact_general_ledger_balance` WHERE tenant_id = '33f319b9-4d57-11ee-960c-025417856183' AND balance_type = 'Actual' GROUP BY entity_code, business_unit_name ORDER BY entity_code, business_unit_name"}
```

## Live tools

`visma_erp_query`, `visma_salesorder_get`, `visma_datamart_query` and the write tools all take `tenant` (id or
exact name). An unknown tenant is refused before anything is sent. A tenant that is `historical` can still be read
for its history.

## Checklist for any cross-company number

- Which tenants are included, and are the historical ones meant to be?
- Is every amount in the same currency (NOK columns for group totals)?
- Does the period straddle the 2026 merger, where entity and business-unit labels change?
