---
id: sales-reporting
title: Sales reporting (orders, customers, products, margin, backlog)
description: How to total sales from BI_SalesOrder without counting quotes, returns, cancelled orders or intercompany sales by accident, how credit memos and zero-cost lines behave, the order-type behavior mapping, and validated queries for monthly sales, top customers, products and order backlog.
tags: [sales, sales-order, reporting, revenue, margin, intercompany, warehouse]
tools: [warehouse_query, visma_erp_query]
prerequisites: [warehouse-sql-basics, multi-currency, honest-results]
difficulty: intermediate
steps: 5
---
# Sales reporting

`visma_datamart.BI_SalesOrder` has one row per **order line**: `date` (order date), `year`, `quarter`, `month`,
`entity_name`, `tenant_id`, `order_type`, `status`, `line_type`, `customer_id`, `customer`, `ic`, `product_number`,
`product_name`, `quantity`, `amount_price_nok`, `amount_cost_price_nok`, `sales_person`, `project_id`.
`visma_gold_sales` has the finer tables (`dim_sales_order`, `fact_sales_order_line`).

## The traps (all checked on real data)

1. **Order types are company-defined and many of them are not sales.** One company has 30 types (SO, IO, PA, FU,
   SF, IN, CS, CM, SK, QT, Q1, Q2, PR, RC, RM, RR, TR, ...). Each has a **`behavior`**: `SalesOrder`, `Invoice`,
   `CreditMemo`, `Quote` or `RMAOrder` (a return). A plain `SUM(amount_price_nok)` includes quotes (about 9% of
   the total in 2025) and returns.
2. **Get the behavior from the synced order-type table**, not from a hard-coded list of codes:
   `visma_bronze.visma_sales_order_types` (`tenantId`, `orderType`, `behavior`; use `DISTINCT` and
   `NOT IFNULL(isDeleted, FALSE)`). Every order type in `BI_SalesOrder` is covered. The descriptions are in
   Norwegian; fetch them live with the `sales_order_types` endpoint when you need to explain a code.
3. **Credit memos are positive numbers.** `CreditMemo` lines carry positive amounts and quantities, so net sales
   must subtract them: `IF(behavior = 'CreditMemo', -amount, amount)`. Show them as their own line as well.
4. **Exclude `Quote` and `RMAOrder`** from sales, and exclude `status = 'Cancelled'`. Types such as `TR`
   (transfer, "Overføring") carry no revenue.
5. **Intercompany sales are in the totals.** The column `ic` is subaccount segment 1, named "IC" or "Intercompany"
   in all 12 companies: `00` on external customers (91% of the 2025 lines), another code (`23`, `28`, `01`, ...)
   when the customer is a group company, and NULL on a few lines (all of them group companies by name). 13.0% of
   the 2025 value (427.0M NOK over all behaviors) has an `ic` other than `00`, and the single largest "customer"
   of 2025 is a group company. The meaning of each code is not in the warehouse (`ic_name` is always NULL), so say
   "intercompany (`ic` other than 00)", not a company name. External sales: `ic = '00'`.
6. **Order-based, not invoice-based.** `date` is the order date and the amounts are order values. Booked,
   invoiced revenue is a different number (`BI_FinanceTable` revenue, skill `general-ledger-reporting`;
   invoices in `fact_customer_invoice`). Say which one you report.
7. **Status** values: `Completed` (almost everything), `Invoiced`, `Open`, `BackOrder`, `Hold`, `Shipping`,
   `Cancelled`. Open, back-ordered, held and shipping orders are the **backlog**, not yet completed sales.
8. **The backlog of merged companies is frozen.** Aquatiq Hygiene Systems AS and Aquatiq Food Factory AS stopped
   booking on 2025-12-31 (skill `tenants-and-entities`) and still show tens of millions in `Hold` and `BackOrder`.
   Nobody ships those orders from that company. Leave the historical tenants out of a current backlog.
9. **Margin needs cost, and 39% of the value has none.** Over all lines, 19.4% of the lines and 39% of the value
   (514M of 1.32bn NOK) have no cost, because Visma returns no cost on the line types `NonInventoryGoods` (98% of
   its lines) and `MiscCharge` (99.5%); `GoodsForInventory` has it (1.2% without). In 2025, 4,702 of 26,983
   `SalesOrder` lines (93.9M of 339.1M NOK) have `amount_cost_price_nok = 0`: fixed-price project lines, services,
   imported items. Their "margin" would be 100%. Compute margin **on costed lines only** and show how much of the
   sales that covers. `Invoice`-behavior lines almost never have cost (14 of 2,631). Since 2026-10-13 the table does
   it for you: `has_cost` marks the lines with a cost, `margin_amount_nok` is empty without one and
   `amount_price_with_cost_nok` is the price of the costed lines, so the margin percentage is
   `SUM(margin_amount_nok) / SUM(amount_price_with_cost_nok)`: 53.1% over all companies, where a margin over every
   line reads 71.4%. `is_charge_line` marks the 68 charge products of the finance workbook (freight, travel, lodging,
   packaging and fee lines: 9,336 lines, 11.6M NOK); they explain only 2% of the value without cost, the rest is
   services and project deliveries that Visma costs elsewhere.
10. **Columns that are empty or partial**: `product_category` is NULL on every row (do not use it); in 2025
    `sales_person` is NULL on about 38% of lines, `project_id` on 64%, `price_class` on 28%. `department_code` and
    `department_name` follow each company's own department segment (the segment Visma names "Avdeling"): subaccount
    segment 3 in Aquatiq AS, AB, Aps, Food Automation, Group and Sensilist, segment 2 in Aqua Pharma AS, Aqua Pharma
    Group AS, Hygiene Systems, Consult, Food Factory and Sense (where segment 3 is a project, a location or not in
    use). Before 2026-10-13 the column read segment 3 everywhere and was NULL-named.
11. **Companies**: `entity_name` and `tenant_id` identify the company; amounts are in NOK at market rate
    (skill `multi-currency`). Merged companies' history stays under their old `tenant_id`.
12. **An order is tenant + order type + order id.** 845 order numbers are used by more than one type (a quote
    `QT` and the order made from it share the number), so `sales_order_key` is unique per (tenant, type, id):
    count orders with `COUNT(DISTINCT sales_order_key)`, never by order id alone.

Order numbers shared by two types in Aquatiq AS (each is two orders with two keys; `orders_with_this_number` and
`distinct_keys` agree):

```tool-call
warehouse_query
{"sql": "SELECT order_id, STRING_AGG(order_type ORDER BY order_type) AS order_types, COUNT(*) AS orders_with_this_number, COUNT(DISTINCT sales_order_key) AS distinct_keys FROM `aquatiq-datavarehus.visma_gold_sales.dim_sales_order` WHERE tenant_id = '33f319b9-4d57-11ee-960c-025417856183' GROUP BY order_id HAVING COUNT(DISTINCT order_type) > 1 ORDER BY order_id DESC LIMIT 8", "max_rows": 8}
```

The mapping used below (repeat it in every query that needs behavior):

```sql
WITH t AS (SELECT DISTINCT tenantId AS tenant_id, orderType AS order_type, behavior
           FROM `aquatiq-datavarehus.visma_bronze.visma_sales_order_types` WHERE NOT IFNULL(isDeleted, FALSE))
```

## Queries

### 1. What is in the data (look before you filter)

Order types with behavior and value:

```tool-call
warehouse_query
{"sql": "WITH t AS (SELECT DISTINCT tenantId AS tenant_id, orderType AS order_type, behavior FROM `aquatiq-datavarehus.visma_bronze.visma_sales_order_types` WHERE NOT IFNULL(isDeleted, FALSE)) SELECT t.behavior, s.order_type, COUNT(DISTINCT s.sales_order_key) AS orders, COUNT(*) AS lines, ROUND(SUM(s.amount_price_nok)) AS amount_nok FROM `aquatiq-datavarehus.visma_datamart.BI_SalesOrder` s JOIN t ON t.tenant_id = s.tenant_id AND t.order_type = s.order_type WHERE s.year = 2025 GROUP BY t.behavior, s.order_type ORDER BY amount_nok DESC", "max_rows": 50}
```

### 2. Net sales per month

In NOK: sales orders and invoice-type documents, less credit memos; no quotes, returns or cancelled orders. The
second column is the external part (`ic = '00'`).

```tool-call
warehouse_query
{"sql": "WITH t AS (SELECT DISTINCT tenantId AS tenant_id, orderType AS order_type, behavior FROM `aquatiq-datavarehus.visma_bronze.visma_sales_order_types` WHERE NOT IFNULL(isDeleted, FALSE)) SELECT s.year, s.month, COUNT(DISTINCT s.sales_order_key) AS orders, ROUND(SUM(IF(t.behavior = 'CreditMemo', -s.amount_price_nok, s.amount_price_nok))) AS net_sales_nok, ROUND(SUM(IF(IFNULL(s.ic, '') = '00', IF(t.behavior = 'CreditMemo', -s.amount_price_nok, s.amount_price_nok), 0))) AS external_net_sales_nok, ROUND(SUM(IF(t.behavior = 'CreditMemo', s.amount_price_nok, 0))) AS credit_memos_nok FROM `aquatiq-datavarehus.visma_datamart.BI_SalesOrder` s JOIN t ON t.tenant_id = s.tenant_id AND t.order_type = s.order_type WHERE s.year = 2025 AND t.behavior IN ('SalesOrder', 'Invoice', 'CreditMemo') AND s.status <> 'Cancelled' GROUP BY s.year, s.month ORDER BY s.month"}
```

### 3. Top customers

Group by company and customer id, never by name. `intercompany` is true when the customer is a group company;
filter on it (or on `ic = '00'`) for external reporting.

```tool-call
warehouse_query
{"sql": "WITH t AS (SELECT DISTINCT tenantId AS tenant_id, orderType AS order_type, behavior FROM `aquatiq-datavarehus.visma_bronze.visma_sales_order_types` WHERE NOT IFNULL(isDeleted, FALSE)) SELECT s.entity_name, s.customer_id, s.customer, LOGICAL_OR(IFNULL(s.ic, '') <> '00') AS intercompany, COUNT(DISTINCT s.sales_order_key) AS orders, ROUND(SUM(IF(t.behavior = 'CreditMemo', -s.amount_price_nok, s.amount_price_nok))) AS net_sales_nok FROM `aquatiq-datavarehus.visma_datamart.BI_SalesOrder` s JOIN t ON t.tenant_id = s.tenant_id AND t.order_type = s.order_type WHERE s.date BETWEEN '2025-01-01' AND '2025-12-31' AND t.behavior IN ('SalesOrder', 'Invoice', 'CreditMemo') AND s.status <> 'Cancelled' GROUP BY s.entity_name, s.customer_id, s.customer ORDER BY net_sales_nok DESC LIMIT 15", "max_rows": 15}
```

### 4. Top products and margin

Margin is value minus cost **on costed lines only**; `sales_with_cost_nok` shows how much of the product's sales
that covers. A low coverage means the margin figure says little about the product.

```tool-call
warehouse_query
{"sql": "WITH t AS (SELECT DISTINCT tenantId AS tenant_id, orderType AS order_type, behavior FROM `aquatiq-datavarehus.visma_bronze.visma_sales_order_types` WHERE NOT IFNULL(isDeleted, FALSE)) SELECT s.product_number, s.product_name, ROUND(SUM(s.quantity)) AS quantity, ROUND(SUM(s.amount_price_nok)) AS sales_nok, ROUND(SUM(s.amount_price_with_cost_nok)) AS sales_with_cost_nok, ROUND(SUM(s.margin_amount_nok)) AS margin_on_costed_lines_nok, ROUND(100 * SAFE_DIVIDE(SUM(s.margin_amount_nok), SUM(s.amount_price_with_cost_nok)), 1) AS margin_pct_on_costed_lines FROM `aquatiq-datavarehus.visma_datamart.BI_SalesOrder` s JOIN t ON t.tenant_id = s.tenant_id AND t.order_type = s.order_type WHERE s.year = 2025 AND t.behavior = 'SalesOrder' AND s.status <> 'Cancelled' GROUP BY s.product_number, s.product_name ORDER BY sales_nok DESC LIMIT 15", "max_rows": 15}
```

### 5. Backlog

Orders not yet completed (open, back order, hold, shipping) in the **current** companies; the `NOT IN` list is the
four historical tenants (skill `tenants-and-entities`):

```tool-call
warehouse_query
{"sql": "WITH t AS (SELECT DISTINCT tenantId AS tenant_id, orderType AS order_type, behavior FROM `aquatiq-datavarehus.visma_bronze.visma_sales_order_types` WHERE NOT IFNULL(isDeleted, FALSE)) SELECT s.entity_name, s.status, COUNT(DISTINCT s.sales_order_key) AS orders, MIN(s.date) AS oldest_order_date, ROUND(SUM(s.amount_price_nok)) AS value_nok FROM `aquatiq-datavarehus.visma_datamart.BI_SalesOrder` s JOIN t ON t.tenant_id = s.tenant_id AND t.order_type = s.order_type WHERE t.behavior = 'SalesOrder' AND s.status IN ('Open', 'BackOrder', 'Hold', 'Shipping') AND s.tenant_id NOT IN ('f630d6b0-5566-11ee-960c-025417856183', '61320c0a-102d-11ee-a0e0-0693d8a7c3dd', 'bb6c024e-5567-11ee-960c-025417856183', '9b1fa69b-5568-11ee-960c-025417856183') GROUP BY s.entity_name, s.status ORDER BY value_nok DESC", "max_rows": 40}
```

An order that has been `Open` or `BackOrder` for a year is stale rather than backlog; look at `oldest_order_date`.

### What an order type means, live from Visma

30 types for the default company, each with `behavior` and a Norwegian description:

```tool-call
visma_erp_query
{"endpoint": "sales_order_types", "page_size": 100}
```

## Reporting

Say the definition you used (which behaviors, whether credit memos were netted, whether cancelled orders, quotes
and intercompany sales were excluded), the basis (order date, order value, NOK at market rate), the period and the
companies. For margin, say what share of the sales had a cost. If the user wants invoiced revenue, point to the
ledger (`BI_FinanceTable`) and say the two will differ.
