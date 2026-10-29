---
id: start-here
title: Start here
description: Which tool answers which question, the safe order of operations, and the rules every answer must follow. Read this first, once per conversation.
tags: [meta, getting-started]
tools: [visma_init, visma_list_skills, visma_get_skill, visma_list_endpoints, visma_list_tenants]
primary: [visma_init, visma_list_skills, visma_get_skill]
difficulty: beginner
steps: 3
---
# Start here

This server gives you the Aquatiq group's Visma data in two forms, and (only when enabled) audited edits.
`visma_init` says whose authority the live tools use (`acts_as`). Normally that is the **integration's service
credentials, not the user's Visma roles**: whoever can call it can read every configured company. If the user has
signed in with `vsv2-mcp login`, it is **the user**: Visma applies their own roles, only the companies they connected
are visible (`visma_list_tenants`), there is no default company so every live call must name one, and DataMart is not
available. A company that is missing in that case is a company the user has not connected: tell them to run
`vsv2-mcp login`, do not look for another way. Either way, always say which company a figure belongs to.

## Pick the source

| You need | Use | Notes |
|---|---|---|
| History, trends, totals, comparisons across months, years or companies | `warehouse_query` (BigQuery gold layer) | Complete history, all companies, real SQL. A snapshot (2026-10-01/02) until the scheduler has run, so know how fresh it is (skill `data-freshness`). |
| The state of one record *right now* (an order, invoice, customer) | `visma_erp_query` or `visma_salesorder_get` | Live from Visma, one record or one page at a time (skill `live-erp-queries`). |
| DataMart-only fields or a bulk reporting extract | `visma_datamart_query` | Behind the live ERP, different query dialect (skill `datamart-graphql`). |
| "Is the data fresh? Did a sync fail?" | `warehouse_overview`, then `visma_sync_status` | Skill `data-freshness` explains what each one actually measures. |
| Which companies exist | `visma_list_tenants` | `tenant_id` (a UUID) is the stable key (skill `tenants-and-entities`). |
| Which tables / endpoints exist | `warehouse_list_tables`, `warehouse_describe_table`, `visma_list_endpoints` | Never guess a name. |
| Which Visma operation answers a question, and which filter names it accepts | `visma_search_api` (skill `api-reference`) | Offline search of Visma's own API reference; no call to Visma. |
| Change a sales order | skill `edit-sales-order`, then `visma_salesorder_patch_plan` | The user approves each change. |
| Create a sales order | skill `create-sales-order`, then `visma_salesorder_create_plan` | Created on hold; the user approves first. |
| Put a sales order on hold or release it | skill `hold-or-release-sales-order`, then `visma_salesorder_status_plan` | Open to Hold and back only; the user approves first. |

## Safe order of operations

1. `visma_init` once per conversation: what is enabled, whose authority applies, why anything is missing, and the
   **known data issues**: check them against the question, and if one affects a figure you are about to report,
   read its skill and say so.
2. Before the first use of a tool, read its skill: `visma_list_skills` with `tool`, then `visma_get_skill`. Always before a write.
3. Discover, do not guess: companies with `visma_list_tenants`, tables with `warehouse_list_tables` and
   `warehouse_describe_table`, endpoints with `visma_list_endpoints`.
4. Start small and filtered: a company and a period in every SQL query, a small `page_size` / `max_rows` first.
5. Report with source, company, period, currency and completeness (skill `honest-results`).

## Rules that apply to every answer

- **Company key**: `tenant_id`. Entity names and codes differ between tables (skill `tenants-and-entities`).
- **Amounts**: say the sign convention and the currency. Tables disagree on both (skills `general-ledger-reporting`, `multi-currency`).
- **Completeness**: one page is not the whole collection; an empty or failed read is *unknown*, not zero.
- **Writes**: only when the user explicitly asks for that specific change, and only through the plan, approve, execute flow.

## Worked examples

What this server can do right now:

```tool-call
visma_init
```

The skills for the SQL tool (the first one listed is the one to read first):

```tool-call
visma_list_skills
{"tool": "warehouse_query"}
```

Read one skill in full:

```tool-call
visma_get_skill
{"id": "warehouse-sql-basics"}
```

Find a skill by what you want to do:

```tool-call
visma_get_skill
{"query": "overdue invoices aging"}
```

## All skills

- `start-here`: this page.
- `honest-results`, `error-triage`: how to read results and what to do when a call fails.
- `tenants-and-entities`: companies, business units, historical tenants, base currencies.
- `warehouse-navigation`, `warehouse-sql-basics`, `multi-currency`: the warehouse and how to query it correctly.
- `general-ledger-reporting`, `account-balance-explanation`, `period-end-review`: accounting analysis.
- `receivables-payables`, `sales-reporting`, `project-profitability`: commercial analysis.
- `data-freshness`: is the data current?
- `live-erp-queries`, `sales-orders-live`, `datamart-graphql`: live Visma and DataMart.
- `edit-sales-order`, `write-safety`: changing data safely.
