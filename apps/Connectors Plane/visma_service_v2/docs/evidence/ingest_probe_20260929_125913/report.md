# Ingestion probe ingest_probe_20260929_125913

## PASS - accounts
- note: completeness proven by API totalCount=614
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=614 bronze=614 pages=1 last_page=614/100 rate_remaining>=14207 slowest_page=526ms

## PASS - attributes
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14206 slowest_page=393ms

## PASS - branches
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14205 slowest_page=424ms

## PASS - carriers
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14204 slowest_page=404ms

## PASS - cash_accounts
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14203 slowest_page=441ms

## PASS - cash_sales
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14202 slowest_page=470ms

## PASS - cash_transactions
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14201 slowest_page=579ms

## PASS - contacts
- note: completeness proven by API totalCount=11
- pass 1: succeeded in=11 bronze=11 pages=1 last_page=11/100 rate_remaining>=14200 slowest_page=587ms

## PASS - credit_terms
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- note: completeness proven by API totalCount=0
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14199 slowest_page=299ms

## PASS - currencies
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=12 bronze=12 pages=1 last_page=12/100 rate_remaining>=14198 slowest_page=394ms

## PASS - currency_rates
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=7023 bronze=7023 pages=1 last_page=7023/0 rate_remaining>=14197 slowest_page=582ms

## PASS - customer_classes
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=3 bronze=3 pages=1 last_page=3/100 rate_remaining>=14196 slowest_page=487ms

## PASS - customer_contracts
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14195 slowest_page=672ms

## PASS - customer_credit_notes
- note: completeness proven by API totalCount=8
- pass 1: succeeded in=8 bronze=8 pages=1 last_page=8/100 rate_remaining>=14194 slowest_page=766ms

## PASS - customer_debit_notes
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14193 slowest_page=391ms

## PASS - customer_documents
- note: completeness proven by API totalCount=236
- pass 1: succeeded in=236 bronze=236 pages=3 last_page=36/100 rate_remaining>=14190 slowest_page=1444ms

## PASS - customer_invoices
- note: completeness proven by API totalCount=228
- pass 1: succeeded in=228 bronze=228 pages=3 last_page=28/100 rate_remaining>=14187 slowest_page=2601ms

## FAIL - customer_locations
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 2 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: missing validation field l')
- pass 1: failed in=14552 bronze=0 pages=146 last_page=52/100 rate_remaining>=14117 slowest_page=53182ms

## PASS - customer_overdue_charges
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14686 slowest_page=454ms

## PASS - customer_payment_methods
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=2 bronze=2 pages=1 last_page=2/100 rate_remaining>=14685 slowest_page=739ms

## PASS - customer_payments
- note: completeness proven by API totalCount=78
- pass 1: succeeded in=78 bronze=78 pages=1 last_page=78/100 rate_remaining>=14684 slowest_page=3309ms

## PASS - customer_sales_prices
- note: completeness proven by API totalCount=1
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14682 slowest_page=345ms

## PASS - customers
- note: completeness proven by API totalCount=6586
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=6586 bronze=6586 pages=66 last_page=86/100 rate_remaining>=14568 slowest_page=4999ms

## PASS - deferral_codes
- note: completeness proven by API totalCount=12
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=12 bronze=12 pages=1 last_page=12/100 rate_remaining>=14562 slowest_page=374ms

## PASS - departments
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14560 slowest_page=320ms

## PASS - dimensions
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=23 bronze=23 pages=1 last_page=23/100 rate_remaining>=14558 slowest_page=303ms

## PASS - discount_codes
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14556 slowest_page=2547ms

## PASS - earning_types
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14554 slowest_page=337ms

## PASS - employees
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14552 slowest_page=724ms

## PASS - expense_claims
- note: completeness proven by API totalCount=45
- pass 1: succeeded in=45 bronze=45 pages=1 last_page=45/100 rate_remaining>=14550 slowest_page=9782ms

## PASS - expense_transactions
- note: completeness proven by API totalCount=321
- pass 1: succeeded in=321 bronze=321 pages=2 last_page=121/200 rate_remaining>=None slowest_page=485ms

## PASS - financial_periods
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=72 bronze=72 pages=1 last_page=72/100 rate_remaining>=14542 slowest_page=333ms

## PASS - fixed_asset_classes
- note: completeness proven by API totalCount=23
- pass 1: succeeded in=23 bronze=23 pages=1 last_page=23/100 rate_remaining>=14541 slowest_page=764ms

## PASS - fixed_asset_property_tax_groups
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14538 slowest_page=316ms

## PASS - fixed_asset_transactions
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14537 slowest_page=345ms

## PASS - fixed_assets
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14535 slowest_page=488ms

## PASS - general_ledger_balances_v2
- note: completeness proven by API totalCount=329
- pass 1: succeeded in=329 bronze=329 pages=4 last_page=29/100 rate_remaining>=14530 slowest_page=514ms

## PASS - inventory_adjustments
- note: completeness proven by API totalCount=18
- pass 1: succeeded in=18 bronze=18 pages=1 last_page=18/100 rate_remaining>=14528 slowest_page=4532ms

## PASS - inventory_issues
- note: completeness proven by API totalCount=81
- pass 1: succeeded in=81 bronze=81 pages=1 last_page=81/100 rate_remaining>=14524 slowest_page=18545ms

## PASS - inventory_items
- note: completeness proven by API totalCount=7252
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=7252 bronze=7252 pages=73 last_page=52/100 rate_remaining>=14445 slowest_page=8648ms

## PASS - inventory_receipts
- note: completeness proven by API totalCount=5
- pass 1: succeeded in=5 bronze=5 pages=1 last_page=5/100 rate_remaining>=14444 slowest_page=850ms

## PASS - inventory_transfers
- note: completeness proven by API totalCount=1
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14443 slowest_page=372ms

## PASS - journal_transactions
- note: completeness proven by lines: 3485 of API totalCount=3485
- pass 1: succeeded in=340 bronze=340 pages=35 last_page=11/100 rate_remaining>=14408 slowest_page=1764ms

## PASS - kit_assemblies
- note: completeness proven by API totalCount=1
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14407 slowest_page=356ms

## PASS - kit_specifications
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- note: completeness proven by API totalCount=0
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14406 slowest_page=309ms

## PASS - landed_cost_codes
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14405 slowest_page=490ms

## PASS - ledgers
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14404 slowest_page=276ms

## PASS - locations
- note: completeness proven by API totalCount=87
- pass 1: succeeded in=87 bronze=87 pages=1 last_page=87/100 rate_remaining>=14403 slowest_page=584ms

## PASS - lot_serial_classes
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14402 slowest_page=275ms

## PASS - number_sequences
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=82 bronze=82 pages=1 last_page=82/100 rate_remaining>=14401 slowest_page=1357ms

## PASS - organizations
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14400 slowest_page=5134ms

## PASS - packaging_types
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14399 slowest_page=317ms

## PASS - payment_methods
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14398 slowest_page=291ms

## PASS - price_classes
- note: completeness proven by API totalCount=6586
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=6586 bronze=6586 pages=66 last_page=86/100 rate_remaining>=14332 slowest_page=3151ms

## PASS - project_account_groups
- pass 1: succeeded in=250 bronze=250 pages=1 last_page=250/0 rate_remaining>=14331 slowest_page=399ms

## PASS - project_budgets
- note: completeness proven by API totalCount=5245
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=5245 bronze=5245 pages=53 last_page=45/100 rate_remaining>=14278 slowest_page=3892ms

## FAIL - project_tasks
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='upstream returned non-success status 400: {"message":"Either ProjectId or ProjectInternalId or PublicId must be specifie')
- pass 1: failed in=0 bronze=0 pages=1 last_page=0/0 rate_remaining>=None slowest_page=0ms

## PASS - project_transactions
- note: completeness proven by API totalCount=480
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=480 bronze=480 pages=5 last_page=80/100 rate_remaining>=14272 slowest_page=631ms

## PASS - projects
- note: completeness proven by API totalCount=12
- pass 1: succeeded in=12 bronze=12 pages=1 last_page=12/100 rate_remaining>=14271 slowest_page=406ms

## PASS - purchase_orders
- note: completeness proven by API totalCount=18
- pass 1: succeeded in=18 bronze=18 pages=1 last_page=18/100 rate_remaining>=14270 slowest_page=672ms

## PASS - purchase_receipts
- note: completeness proven by API totalCount=14
- pass 1: succeeded in=14 bronze=14 pages=1 last_page=14/100 rate_remaining>=14269 slowest_page=489ms

## PASS - sales_categories
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14268 slowest_page=301ms

## PASS - sales_order_types
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14267 slowest_page=606ms

## PASS - sales_orders_v3
- note: completeness proven by API totalCount=200
- pass 1: succeeded in=200 bronze=200 pages=14 last_page=5/15 rate_remaining>=None slowest_page=955ms

## PASS - sales_persons
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14266 slowest_page=424ms

## PASS - shipments
- note: completeness proven by API totalCount=201
- pass 1: succeeded in=201 bronze=201 pages=3 last_page=1/100 rate_remaining>=14263 slowest_page=666ms

## PASS - stocktakes_v2
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14262 slowest_page=315ms

## PASS - subaccounts
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14261 slowest_page=368ms

## PASS - supplier_classes
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=16 bronze=16 pages=1 last_page=16/100 rate_remaining>=14260 slowest_page=351ms

## PASS - supplier_documents
- note: completeness proven by API totalCount=163
- pass 1: succeeded in=163 bronze=163 pages=2 last_page=63/100 rate_remaining>=14258 slowest_page=770ms

## PASS - supplier_invoices
- note: completeness proven by API totalCount=163
- pass 1: succeeded in=163 bronze=163 pages=2 last_page=63/100 rate_remaining>=14256 slowest_page=1377ms

## PASS - supplier_locations
- note: completeness proven by API totalCount=5
- pass 1: succeeded in=5 bronze=5 pages=1 last_page=5/100 rate_remaining>=14255 slowest_page=427ms

## PASS - supplier_payments
- note: completeness proven by API totalCount=110
- pass 1: succeeded in=110 bronze=110 pages=2 last_page=10/100 rate_remaining>=14253 slowest_page=744ms

## PASS - suppliers
- note: completeness proven by API totalCount=6
- pass 1: succeeded in=6 bronze=6 pages=1 last_page=6/100 rate_remaining>=14252 slowest_page=953ms

## PASS - unit_of_measures
- pass 1: succeeded in=43 bronze=43 pages=1 last_page=43/100 rate_remaining>=14251 slowest_page=296ms

## PASS - vat_categories
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14250 slowest_page=269ms

## PASS - vat_zones
- note: completeness proven by API totalCount=15
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=15 bronze=15 pages=1 last_page=15/100 rate_remaining>=14249 slowest_page=285ms

## PASS - vats
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14248 slowest_page=626ms

## FAIL - warehouses
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 15 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: missing validation field ')
- pass 1: failed in=15 bronze=0 pages=1 last_page=15/100 rate_remaining>=14247 slowest_page=1379ms
