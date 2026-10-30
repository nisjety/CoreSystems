# Endpoint counts: Visma against staging (2026-10-13)

Rule: FAIL when more records are missing from staging than Visma reports as modified
since the last write. Where an endpoint has no modified filter that works, or a tenant
has no write on record, the tolerance is 0. See scripts/verify_endpoint_counts.py.

- Last write: the heartbeat table where a row exists, else the last staging write.
- Scope: 64 endpoints counted for 1 tenants (64 cells); 32 NOT COUNTABLE.
- Cells: PASS 64 (64 exact, 0 within the tolerance), WARN 0, FAIL 0, ERROR 0
- Visma calls: {'token': 2, 'GET': 88}.

## Per endpoint

Missing is summed over the tenants where records are missing; Allowed is what excuses
it (the modified-since count, 0 where no filter works).

| Endpoint | PASS | WARN | FAIL | ERROR | Visma | Staging | Missing | Allowed | Modified filter | Method |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| accounts | 1 | 0 | 0 | 0 | 225 | 225 | 0 | 0 | not needed | finance list, one row |
| attributes | 1 | 0 | 0 | 0 | 41 | 41 | 0 | 0 | not needed | finance list, one row |
| carriers | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| cash_sales | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| contacts | 1 | 0 | 0 | 0 | 9 | 9 | 0 | 0 | not needed | finance list, one row |
| countries | 1 | 0 | 0 | 0 | 260 | 260 | 0 | 0 | not needed | finance list, one row |
| credit_terms | 1 | 0 | 0 | 0 | 11 | 11 | 0 | 0 | not needed | finance list, one row |
| customer_contracts | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| customer_credit_notes | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | not needed | finance list, one row |
| customer_credit_write_offs | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | not needed | finance list, one row |
| customer_debit_notes | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| customer_documents | 1 | 0 | 0 | 0 | 55 | 55 | 0 | 0 | not needed | finance list, one row |
| customer_invoices | 1 | 0 | 0 | 0 | 53 | 53 | 0 | 0 | not needed | finance list, one row |
| customer_overdue_charges | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| customer_payment_methods | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| customer_payments | 1 | 0 | 0 | 0 | 44 | 44 | 0 | 0 | not needed | finance list, one row |
| customer_sales_prices | 1 | 0 | 0 | 0 | 17 | 17 | 0 | 0 | not needed | finance list, one row |
| customers | 1 | 0 | 0 | 0 | 8 | 8 | 0 | 0 | not needed | finance list, one row |
| deferral_codes | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | not needed | finance list, one row |
| discount_codes | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| discounts | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| employees | 1 | 0 | 0 | 0 | 3 | 3 | 0 | 0 | not needed | finance list, one row |
| expense_claims | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| fixed_asset_classes | 1 | 0 | 0 | 0 | 6 | 6 | 0 | 0 | not needed | finance list, one row |
| fixed_asset_property_tax_groups | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| fixed_asset_transactions | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| fixed_assets | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| general_ledger_transactions | 1 | 0 | 0 | 0 | 2,707 | 2,707 | 0 | 0 | not needed | finance list, one row (ledger=1, fromPeriod=202001, toPeriod=203012) |
| inventory_adjustments | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| inventory_issues | 1 | 0 | 0 | 0 | 6 | 6 | 0 | 0 | not needed | finance list, one row |
| inventory_item_classes | 1 | 0 | 0 | 0 | 5 | 5 | 0 | 0 | not needed | finance list, one row |
| inventory_item_post_classes | 1 | 0 | 0 | 0 | 7 | 7 | 0 | 0 | not needed | finance list, one row |
| inventory_items | 1 | 0 | 0 | 0 | 144 | 144 | 0 | 0 | not needed | finance list, one row |
| inventory_receipt_transfers | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| inventory_receipts | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| inventory_transfers | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| kit_assemblies | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| kit_specifications | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| landed_cost_codes | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| locations | 1 | 0 | 0 | 0 | 13 | 13 | 0 | 0 | not needed | finance list, one row |
| lot_serial_classes | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | not needed | finance list, one row |
| packaging_types | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| project_budgets | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| projects | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| purchase_orders | 1 | 0 | 0 | 0 | 7 | 7 | 0 | 0 | not needed | finance list, one row |
| purchase_receipt_transfers | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| purchase_receipts | 1 | 0 | 0 | 0 | 6 | 6 | 0 | 0 | not needed | finance list, one row |
| sales_categories | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| sales_order_customers | 1 | 0 | 0 | 0 | 7 | 7 | 0 | 0 | not needed | Sales Order API, one row |
| sales_order_inventory | 1 | 0 | 0 | 0 | 138 | 138 | 0 | 0 | not needed | Sales Order API, one row |
| sales_order_types | 1 | 0 | 0 | 0 | 14 | 14 | 0 | 0 | not needed | finance list, one row |
| sales_orders_v3 | 1 | 0 | 0 | 0 | 13 | 13 | 0 | 0 | not needed | Sales Order API, one row |
| sales_persons | 1 | 0 | 0 | 0 | 3 | 3 | 0 | 0 | not needed | finance list, one row |
| shipments | 1 | 0 | 0 | 0 | 5 | 5 | 0 | 0 | not needed | finance list, one row |
| stocktakes_v2 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| subaccounts | 1 | 0 | 0 | 0 | 4 | 4 | 0 | 0 | not needed | finance list, one row |
| supplier_documents | 1 | 0 | 0 | 0 | 147 | 147 | 0 | 0 | not needed | finance list, one row |
| supplier_inbox_rules | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | not needed | finance list, one row |
| supplier_invoices | 1 | 0 | 0 | 0 | 147 | 147 | 0 | 0 | not needed | finance list, one row |
| supplier_locations | 1 | 0 | 0 | 0 | 25 | 25 | 0 | 0 | not needed | finance list, one row |
| supplier_payments | 1 | 0 | 0 | 0 | 144 | 144 | 0 | 0 | not needed | finance list, one row |
| suppliers | 1 | 0 | 0 | 0 | 25 | 25 | 0 | 0 | not needed | finance list, one row |
| vat_zones | 1 | 0 | 0 | 0 | 13 | 13 | 0 | 0 | not needed | finance list, one row |
| warehouses | 1 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | not needed | finance list, one row |

## Cells that are not PASS

None.

## NOT COUNTABLE

| Endpoint | Reason |
|---|---|
| branches | no total: the one-row answer has a row but no totalCount |
| budgets | needs branch, ledger and financialYear (a partitioned fan-out): HTTP 400 without them. A count is one request per combination, not one total. |
| cash_accounts | no total: the endpoint ignores pageSize and answers its whole list (8 rows for a one-row request) without a totalCount |
| cash_transactions | no total: the one-row answer has a row but no totalCount |
| currencies | no total: the endpoint ignores pageSize and answers its whole list (9 rows for a one-row request) without a totalCount |
| currency_rate_types | no total: the one-row answer has a row but no totalCount |
| currency_rates | no total: the endpoint ignores pageSize and answers its whole list (80 rows for a one-row request) without a totalCount |
| customer_balances | no total: the one-row answer has a row but no totalCount |
| customer_balances_v2 | no total: the one-row answer has a row but no totalCount |
| customer_classes | no total: the endpoint ignores pageSize and answers its whole list (3 rows for a one-row request) without a totalCount |
| customer_locations | child of the Sales Order API (/Customers/{customerId}/locations): an unpaged list per customer with no total, and it needs the parent key. |
| departments | no total: the one-row answer has a row but no totalCount |
| dimensions | no total: the endpoint ignores pageSize and answers its whole list (23 rows for a one-row request) without a totalCount |
| earning_types | no total: the endpoint ignores pageSize and answers its whole list (4 rows for a one-row request) without a totalCount |
| expense_receipts | no total: the answer is the whole unpaged list, no totalCount, about 100 s (pilot: 14,548 rows, HTTP 200). |
| financial_periods | no total: the endpoint ignores pageSize and answers its whole list (48 rows for a one-row request) without a totalCount |
| general_ledger_balances_v2 | period-scanned: periodId is required (HTTP 400 without it) and the total is per period, so a count is one request per period, not one total. |
| journal_transactions | period-scanned and line-counted: periodId is required (HTTP 400 without it) and the total counts journal lines, while staging holds one row per batch. |
| languages | no total: the endpoint ignores pageSize and answers its whole list (6 rows for a one-row request) without a totalCount |
| ledgers | no total: the endpoint ignores pageSize and answers its whole list (4 rows for a one-row request) without a totalCount |
| number_sequences | no total: the endpoint ignores pageSize and answers its whole list (75 rows for a one-row request) without a totalCount |
| organizations | no total: the one-row answer has a row but no totalCount |
| payment_methods | no total: the endpoint ignores pageSize and answers its whole list (8 rows for a one-row request) without a totalCount |
| project_account_groups | no total: the endpoint ignores pageSize and answers its whole list (9 rows for a one-row request) without a totalCount |
| project_tasks | needs projectId (one request per project): HTTP 400 without it, so no single total. |
| project_transactions | period-scanned: fromPeriod and toPeriod are required (HTTP 400 without them) and the fetch walks one month at a time (staging keeps a row per document and _scan_period), so a count is one request per month, not one total. |
| supplier_balances_v2 | no total: the one-row answer has a row but no totalCount |
| supplier_classes | no total: the endpoint ignores pageSize and answers its whole list (7 rows for a one-row request) without a totalCount |
| time_cards | needs employeeCd (one request per employee): HTTP 400 without it, so no single total. |
| unit_of_measures | no total: the endpoint ignores pageSize and answers its whole list (24 rows for a one-row request) without a totalCount |
| vat_categories | no total: the endpoint ignores pageSize and answers its whole list (19 rows for a one-row request) without a totalCount |
| vats | no total: the endpoint ignores pageSize and answers its whole list (25 rows for a one-row request) without a totalCount |
