# Endpoint counts: Visma against staging (2026-10-12)

Rule: FAIL when more records are missing from staging than Visma reports as modified
since the last write. Where an endpoint has no modified filter that works, or a tenant
has no write on record, the tolerance is 0. See scripts/verify_endpoint_counts.py.

- Last write: last staging write (no heartbeat yet).
- Scope: 63 endpoints counted for 12 tenants (756 cells); 33 NOT COUNTABLE.
- Cells: PASS 752 (656 exact, 96 within the tolerance), WARN 1, FAIL 3, ERROR 0
- Visma calls: {'token': 24, 'GET': 913}.

## Per endpoint

Missing is summed over the tenants where records are missing; Allowed is what excuses
it (the modified-since count, 0 where no filter works).

| Endpoint | PASS | WARN | FAIL | ERROR | Visma | Staging | Missing | Allowed | Modified filter | Method |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| accounts | 12 | 0 | 0 | 0 | 5,825 | 5,821 | 4 | 4 | works | finance list, one row |
| attributes | 12 | 0 | 0 | 0 | 133 | 133 | 0 | 0 | not needed | finance list, one row |
| carriers | 12 | 0 | 0 | 0 | 109 | 109 | 0 | 0 | not needed | finance list, one row |
| cash_sales | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| contacts | 12 | 0 | 0 | 0 | 17,442 | 17,412 | 30 | 43 | works | finance list, one row |
| countries | 12 | 0 | 0 | 0 | 3,120 | 3,120 | 0 | 0 | not needed | finance list, one row |
| credit_terms | 12 | 0 | 0 | 0 | 170 | 170 | 0 | 0 | not needed | finance list, one row |
| customer_contracts | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| customer_credit_notes | 12 | 0 | 0 | 0 | 1,431 | 1,421 | 10 | 26 | works | finance list, one row |
| customer_credit_write_offs | 12 | 0 | 0 | 0 | 24 | 23 | 1 | 1 | works | finance list, one row |
| customer_debit_notes | 12 | 0 | 0 | 0 | 45 | 45 | 0 | 0 | not needed | finance list, one row |
| customer_documents | 12 | 0 | 0 | 0 | 33,324 | 33,009 | 315 | 707 | works | finance list, one row |
| customer_invoices | 12 | 0 | 0 | 0 | 31,824 | 31,520 | 304 | 678 | works | finance list, one row |
| customer_overdue_charges | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| customer_payments | 12 | 0 | 0 | 0 | 30,524 | 30,237 | 287 | 307 | works | finance list, one row |
| customer_sales_prices | 12 | 0 | 0 | 0 | 17,227 | 17,222 | 5 | 24 | works | finance list, one row |
| customers | 12 | 0 | 0 | 0 | 27,298 | 27,287 | 11 | 74 | works | finance list, one row |
| deferral_codes | 12 | 0 | 0 | 0 | 77 | 77 | 0 | 0 | not needed | finance list, one row |
| discount_codes | 12 | 0 | 0 | 0 | 32 | 32 | 0 | 0 | not needed | finance list, one row |
| discounts | 12 | 0 | 0 | 0 | 144 | 143 | 1 | 4 | works | finance list, one row |
| employees | 12 | 0 | 0 | 0 | 227 | 227 | 0 | 0 | not needed | finance list, one row |
| expense_claims | 12 | 0 | 0 | 0 | 4,946 | 4,832 | 114 | 114 | works | finance list, one row |
| fixed_asset_classes | 12 | 0 | 0 | 0 | 146 | 146 | 0 | 0 | not needed | finance list, one row |
| fixed_asset_property_tax_groups | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| fixed_asset_transactions | 12 | 0 | 0 | 0 | 3,341 | 3,339 | 2 | 2 | works | finance list, one row |
| fixed_assets | 12 | 0 | 0 | 0 | 164 | 163 | 1 | 1 | works | finance list, one row |
| general_ledger_transactions | 12 | 0 | 0 | 0 | 1,119,576 | 1,108,577 | 10,999 | 11,443 | works | finance list, one row (ledger=1, fromPeriod=202001, toPeriod=203012) |
| inventory_adjustments | 12 | 0 | 0 | 0 | 6,569 | 6,474 | 95 | 97 | works | finance list, one row |
| inventory_issues | 12 | 0 | 0 | 0 | 24,912 | 24,664 | 248 | 252 | works | finance list, one row |
| inventory_item_classes | 12 | 0 | 0 | 0 | 119 | 119 | 0 | 0 | not needed | finance list, one row |
| inventory_item_post_classes | 12 | 0 | 0 | 0 | 130 | 130 | 0 | 0 | not needed | finance list, one row |
| inventory_items | 12 | 0 | 0 | 0 | 12,952 | 12,934 | 18 | 132 | works | finance list, one row |
| inventory_receipt_transfers | 12 | 0 | 0 | 0 | 1 | 1 | 0 | 0 | not needed | finance list, one row |
| inventory_receipts | 12 | 0 | 0 | 0 | 4,479 | 4,440 | 39 | 39 | works | finance list, one row |
| inventory_transfers | 12 | 0 | 0 | 0 | 1,009 | 1,001 | 8 | 14 | works | finance list, one row |
| kit_assemblies | 12 | 0 | 0 | 0 | 994 | 986 | 8 | 8 | works | finance list, one row |
| kit_specifications | 12 | 0 | 0 | 0 | 1,286 | 1,283 | 3 | 4 | works | finance list, one row |
| landed_cost_codes | 12 | 0 | 0 | 0 | 19 | 19 | 0 | 0 | not needed | finance list, one row |
| locations | 12 | 0 | 0 | 0 | 35,752 | 35,726 | 26 | 83 | works | finance list, one row |
| lot_serial_classes | 12 | 0 | 0 | 0 | 14 | 14 | 0 | 0 | not needed | finance list, one row |
| packaging_types | 12 | 0 | 0 | 0 | 3 | 3 | 0 | 0 | not needed | finance list, one row |
| project_budgets | 12 | 0 | 0 | 0 | 19,380 | 19,241 | 139 | 336 | works | finance list, one row |
| projects | 12 | 0 | 0 | 0 | 2,636 | 2,602 | 34 | 74 | works | finance list, one row |
| purchase_orders | 11 | 1 | 0 | 0 | 4,296 | 4,269 | 28 | 83 | works | finance list, one row |
| purchase_receipt_transfers | 12 | 0 | 0 | 0 | 5 | 5 | 0 | 0 | not needed | finance list, one row |
| purchase_receipts | 12 | 0 | 0 | 0 | 5,313 | 5,266 | 47 | 69 | works | finance list, one row |
| sales_categories | 12 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | not needed | finance list, one row |
| sales_order_customers | 9 | 0 | 3 | 0 | 25,842 | 25,830 | 12 | 0 | ignored | Sales Order API, one row |
| sales_order_inventory | 12 | 0 | 0 | 0 | 10,148 | 10,139 | 9 | 517 | works | Sales Order API, one row |
| sales_order_types | 12 | 0 | 0 | 0 | 167 | 167 | 0 | 0 | not needed | finance list, one row |
| sales_orders_v3 | 12 | 0 | 0 | 0 | 27,637 | 27,377 | 260 | 393 | works | Sales Order API, one row |
| sales_persons | 12 | 0 | 0 | 0 | 133 | 133 | 0 | 0 | not needed | finance list, one row |
| shipments | 12 | 0 | 0 | 0 | 25,247 | 25,000 | 247 | 346 | works | finance list, one row |
| stocktakes_v2 | 12 | 0 | 0 | 0 | 45 | 45 | 0 | 0 | not needed | finance list, one row |
| subaccounts | 12 | 0 | 0 | 0 | 13,616 | 13,611 | 5 | 5 | works | finance list, one row |
| supplier_documents | 12 | 0 | 0 | 0 | 30,725 | 30,403 | 322 | 547 | works | finance list, one row |
| supplier_inbox_rules | 12 | 0 | 0 | 0 | 133 | 133 | 0 | 0 | not needed | finance list, one row |
| supplier_invoices | 12 | 0 | 0 | 0 | 30,725 | 30,403 | 322 | 547 | works | finance list, one row |
| supplier_locations | 12 | 0 | 0 | 0 | 8,747 | 8,741 | 6 | 16 | works | finance list, one row |
| supplier_payments | 12 | 0 | 0 | 0 | 24,180 | 24,007 | 173 | 248 | works | finance list, one row |
| suppliers | 12 | 0 | 0 | 0 | 3,129 | 3,123 | 6 | 10 | works | finance list, one row |
| vat_zones | 12 | 0 | 0 | 0 | 120 | 120 | 0 | 0 | not needed | finance list, one row |
| warehouses | 12 | 0 | 0 | 0 | 36 | 36 | 0 | 0 | not needed | finance list, one row |

## Cells that are not PASS

| Endpoint | Tenant | Visma | Staging | Missing | Changed since last write | Tolerance basis | Last write (source) | Verdict | Why |
|---|---|---:|---:|---:|---:|---|---|---|---|
| purchase_orders | Aquatiq Aps | 7 | 8 | -1 | - | not needed | 2026-10-01T23:28:22+00:00 (last staging write (no heartbeat yet)) | WARN | staging has 1 more than Visma (deleted in Visma since the last full pass) |
| sales_order_customers | Aquatiq AS (pilot) | 6,370 | 6,360 | 10 | - | no working modified filter, tolerance 0 | 2026-10-01T11:48:19+00:00 (last staging write (no heartbeat yet)) | FAIL | 10 records missing from staging and none can be pending (tolerance 0) |
| sales_order_customers | Aqua Pharma Group AS | 15 | 14 | 1 | - | no working modified filter, tolerance 0 | 2026-10-01T14:00:08+00:00 (last staging write (no heartbeat yet)) | FAIL | 1 records missing from staging and none can be pending (tolerance 0) |
| sales_order_customers | Sensilist AS | 357 | 356 | 1 | - | no working modified filter, tolerance 0 | 2026-10-01T13:52:35+00:00 (last staging write (no heartbeat yet)) | FAIL | 1 records missing from staging and none can be pending (tolerance 0) |

## NOT COUNTABLE

| Endpoint | Reason |
|---|---|
| branches | no total: the one-row answer has a row but no totalCount |
| budgets | needs branch, ledger and financialYear (a partitioned fan-out): HTTP 400 without them. A count is one request per combination, not one total. |
| cash_accounts | no total: the endpoint ignores pageSize and answers its whole list (29 rows for a one-row request) without a totalCount |
| cash_transactions | no total: the one-row answer has a row but no totalCount |
| currencies | no total: the endpoint ignores pageSize and answers its whole list (12 rows for a one-row request) without a totalCount |
| currency_rate_types | no total: the endpoint ignores pageSize and answers its whole list (2 rows for a one-row request) without a totalCount |
| currency_rates | no total: the endpoint ignores pageSize and answers its whole list (7,133 rows for a one-row request) without a totalCount |
| customer_balances | no total: the one-row answer has a row but no totalCount |
| customer_balances_v2 | no total: the one-row answer has a row but no totalCount |
| customer_classes | no total: the endpoint ignores pageSize and answers its whole list (3 rows for a one-row request) without a totalCount |
| customer_locations | child endpoint of the Sales Order API (/Customers/{customerId}/locations): an unpaged list per customer with no total, and it needs the parent key. |
| customer_payment_methods | no total: the one-row answer has a row but no totalCount |
| departments | no total: the endpoint ignores pageSize and answers its whole list (54 rows for a one-row request) without a totalCount |
| dimensions | no total: the endpoint ignores pageSize and answers its whole list (23 rows for a one-row request) without a totalCount |
| earning_types | no total: the endpoint ignores pageSize and answers its whole list (4 rows for a one-row request) without a totalCount |
| expense_receipts | no total: the answer is the whole unpaged list with no totalCount, and it takes about 100 s (pilot: 14,548 rows, HTTP 200). |
| financial_periods | no total: the endpoint ignores pageSize and answers its whole list (72 rows for a one-row request) without a totalCount |
| general_ledger_balances_v2 | period-scanned: periodId is required (HTTP 400 without it) and the total is per period, so a count is one request per period, not one total. |
| journal_transactions | period-scanned and line-counted: periodId is required (HTTP 400 without it) and the total counts journal lines, while staging holds one row per batch. |
| languages | no total: the endpoint ignores pageSize and answers its whole list (6 rows for a one-row request) without a totalCount |
| ledgers | no total: the endpoint ignores pageSize and answers its whole list (4 rows for a one-row request) without a totalCount |
| number_sequences | no total: the endpoint ignores pageSize and answers its whole list (82 rows for a one-row request) without a totalCount |
| organizations | no total: the one-row answer has a row but no totalCount |
| payment_methods | no total: the endpoint ignores pageSize and answers its whole list (8 rows for a one-row request) without a totalCount |
| project_account_groups | no total: the endpoint ignores pageSize and answers its whole list (250 rows for a one-row request) without a totalCount |
| project_tasks | needs projectId (one request per project): HTTP 400 without it. A parent key is required, so there is no single total. |
| project_transactions | period-scanned: fromPeriod and toPeriod are required (HTTP 400 without them), and staging keeps one row per document and month (_scan_period), so a window total is not a row count of staging. |
| supplier_balances_v2 | no total: the one-row answer has a row but no totalCount |
| supplier_classes | no total: the endpoint ignores pageSize and answers its whole list (16 rows for a one-row request) without a totalCount |
| time_cards | needs employeeCd (one request per employee): HTTP 400 without it. A parent key is required, so there is no single total. |
| unit_of_measures | no total: the endpoint ignores pageSize and answers its whole list (43 rows for a one-row request) without a totalCount |
| vat_categories | no total: the endpoint ignores pageSize and answers its whole list (48 rows for a one-row request) without a totalCount |
| vats | no total: the endpoint ignores pageSize and answers its whole list (67 rows for a one-row request) without a totalCount |

## Investigation of the FAIL and WARN cells

Written by hand after the run, from live GETs and SELECTs on 2026-10-12. The script does not
produce this section; running it again for the same date rewrites the file without it.

**FAIL: sales_order_customers, three tenants.** Visma ignores `modifiedSince` on this list, so the
tolerance is 0 and anything Visma changed since the last write counts as missing. Method: walked
Visma's whole customer list (Sales Order API), took the key `internalId`, compared it with the live
keys in staging, and read each missing customer from the finance API by key (`GET
/v1/customer/{number}`: `createdDateTime`, `lastModifiedDateTime`, `status`). The Sales Order API
has no by-key path that answers (`/Customers/{id}` and `/Customers/{internalId}` are HTTP 404 even
for staged customers), so it gave no evidence. All three walks had 0 duplicate keys.

| Tenant | Visma | Staging | Missing keys | Extra staged keys | Last write |
|---|---:|---:|---:|---:|---|
| Aquatiq AS (pilot) | 6,370 | 6,360 | 11 | 1 | 2026-10-01T11:48:19Z |
| Aqua Pharma Group AS | 15 | 14 | 1 | 0 | 2026-10-01T14:00:08Z |
| Sensilist AS | 357 | 356 | 1 | 0 | 2026-10-01T13:52:35Z |

- Pilot, 9 of the 11 missing keys (internalId 219601 to 219905, all above the highest staged id
  219541) are customers created after the last write: `createdDateTime` from 2026-10-01T23:24 to
  2026-10-12T12:14. None is in the staged finance `customers` table either.
- Pilot, 2 of the 11 (171062, 209321) are older customers. The staged finance `customers` rows,
  fetched 2026-10-01T11:57:54Z, hold both as `CreditHold`. They are Active now (modified
  2026-10-06 and 2026-10-05). The Sales Order list holds only status A and T customers (staging: A
  6,358, T 2; in the staged finance table Active 6,358 plus OneTime 2 is exactly those 6,360), so
  they entered the list after the fetch.
- Pilot, the 1 extra staged key (internalId 209106, customer 315069) was Active in the staged
  `customers` table and is Inactive now (modified 2026-10-05T16:00:52), so it left the list after
  the fetch. The net of 11 missing and 1 extra is the 10 the gate reports.
- Aqua Pharma Group AS: internalId 59654, created 2026-10-07T09:20, above the highest staged id
  (48785). Sensilist AS: internalId 219888, created 2026-10-12T08:52.

Classification: B for all three. Visma changed after the fetch (created, re-activated or
inactivated), so v2 is right. Not A: no record that was in the list and unchanged at the fetch is
missing. Not C: no duplicate key in any walk.

**WARN: purchase_orders, Aquatiq Aps.** Staging holds 8, Visma lists 7, and the extra staged key is
`000001`. `GET /v1/purchaseorder/000001` answers HTTP 404: deleted in Visma since the fetch (class
B). A complete scan would mark the row deleted; staging still holds it live, and the table's last
write is 2026-10-01T23:28:22Z.

**Negative controls** (live Visma answers, altered staging numbers; the gate exits FAIL as it
should): 650 rows taken off the pilot's customer_invoices count gives 942 missing against 647
changed; the real count with the last write set to 2026-10-12 00:00 UTC gives 292 missing against
115 changed (so the pilot's 292 pass today because 647 invoices changed in the 10 days since the
last write, which shows how much the age of staging widens the tolerance); one row off countries
(a filter Visma ignores) gives 1 missing against tolerance 0; 10,000 lines off the pilot's general
ledger gives 20,219 missing against 10,790 changed.
