# Ingestion probe ingest_probe_20260929_133043

## FAIL - attributes
- FAIL: endpoint has a modified_field but no checkpoint watermark was stored
- note: completeness proven by API totalCount=48
- pass 1: succeeded in=48 bronze=48 pages=1 last_page=48/100 rate_remaining>=14217 slowest_page=850ms

## PASS - branches
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14216 slowest_page=537ms

## PASS - carriers
- note: completeness proven by API totalCount=55
- pass 1: succeeded in=55 bronze=55 pages=1 last_page=55/100 rate_remaining>=14215 slowest_page=482ms

## PASS - cash_accounts
- pass 1: succeeded in=29 bronze=29 pages=1 last_page=29/100 rate_remaining>=14214 slowest_page=2586ms

## PASS - cash_sales
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14213 slowest_page=577ms

## PASS - cash_transactions
- pass 1: succeeded in=490 bronze=490 pages=5 last_page=90/100 rate_remaining>=14208 slowest_page=1730ms

## PASS - credit_terms
- note: completeness proven by API totalCount=22
- pass 1: succeeded in=22 bronze=22 pages=1 last_page=22/100 rate_remaining>=14207 slowest_page=862ms

## PASS - customer_contracts
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14206 slowest_page=518ms

## PASS - customer_debit_notes
- note: completeness proven by API totalCount=17
- pass 1: succeeded in=17 bronze=17 pages=1 last_page=17/100 rate_remaining>=14205 slowest_page=1191ms

## PASS - customer_locations
- note: completeness proven by API totalCount=14552
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=14552 bronze=14552 pages=146 last_page=52/100 rate_remaining>=14058 slowest_page=63521ms

## PASS - customer_overdue_charges
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14057 slowest_page=440ms

## PASS - departments
- pass 1: succeeded in=54 bronze=54 pages=1 last_page=54/100 rate_remaining>=14056 slowest_page=437ms

## PASS - discount_codes
- note: completeness proven by API totalCount=7
- pass 1: succeeded in=7 bronze=7 pages=1 last_page=7/100 rate_remaining>=14055 slowest_page=377ms

## FAIL - earning_types
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 4 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: missing validation field l')
- pass 1: failed in=4 bronze=0 pages=1 last_page=4/100 rate_remaining>=14054 slowest_page=416ms

## FAIL - employees
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 6 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: missing validation field w')
- pass 1: failed in=111 bronze=0 pages=2 last_page=11/100 rate_remaining>=14052 slowest_page=635ms

## PASS - fixed_asset_property_tax_groups
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14051 slowest_page=361ms

## PASS - fixed_asset_transactions
- note: completeness proven by API totalCount=1061
- pass 1: succeeded in=1061 bronze=1061 pages=11 last_page=61/100 rate_remaining>=14040 slowest_page=459ms

## PASS - fixed_assets
- note: completeness proven by API totalCount=69
- pass 1: succeeded in=69 bronze=69 pages=1 last_page=69/100 rate_remaining>=14039 slowest_page=1325ms

## PASS - kit_specifications
- note: completeness proven by API totalCount=643
- pass 1: succeeded in=643 bronze=643 pages=7 last_page=43/100 rate_remaining>=14032 slowest_page=1689ms

## FAIL - landed_cost_codes
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 13 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: missing validation field ')
- pass 1: failed in=13 bronze=0 pages=1 last_page=13/100 rate_remaining>=14031 slowest_page=452ms

## PASS - ledgers
- pass 1: succeeded in=4 bronze=4 pages=1 last_page=4/100 rate_remaining>=14030 slowest_page=404ms

## PASS - lot_serial_classes
- note: completeness proven by API totalCount=1
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14029 slowest_page=334ms

## PASS - organizations
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14028 slowest_page=8723ms

## PASS - packaging_types
- note: completeness proven by API totalCount=1
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14027 slowest_page=339ms

## FAIL - payment_methods
- FAIL: endpoint has a modified_field but no checkpoint watermark was stored
- pass 1: succeeded in=8 bronze=8 pages=1 last_page=8/100 rate_remaining>=14026 slowest_page=371ms

## PASS - sales_categories
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14025 slowest_page=353ms

## FAIL - sales_order_types
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 30 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: missing validation field ')
- pass 1: failed in=30 bronze=0 pages=1 last_page=30/100 rate_remaining>=14024 slowest_page=625ms

## PASS - sales_persons
- note: completeness proven by API totalCount=114
- pass 1: succeeded in=114 bronze=114 pages=2 last_page=14/100 rate_remaining>=14022 slowest_page=1998ms

## FAIL - stocktakes_v2
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 22 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: missing primary key field')
- pass 1: failed in=22 bronze=0 pages=1 last_page=22/100 rate_remaining>=14021 slowest_page=6525ms

## PASS - subaccounts
- note: completeness proven by API totalCount=11802
- pass 1: succeeded in=11802 bronze=11802 pages=119 last_page=2/100 rate_remaining>=13902 slowest_page=728ms

## PASS - vat_categories
- pass 1: succeeded in=48 bronze=48 pages=1 last_page=48/100 rate_remaining>=13901 slowest_page=894ms

## PASS - vats
- pass 1: succeeded in=67 bronze=67 pages=1 last_page=67/100 rate_remaining>=13900 slowest_page=1751ms
