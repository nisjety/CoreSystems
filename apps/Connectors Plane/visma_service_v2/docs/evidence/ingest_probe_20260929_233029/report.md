# Ingestion probe ingest_probe_20260929_233029

## PASS - accounts
- note: completeness proven by API totalCount=614
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=614 bronze=614 pages=1 last_page=614/100 rate_remaining>=13836 slowest_page=929ms

## PASS - attributes
- note: completeness proven by API totalCount=48
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=48 bronze=48 pages=1 last_page=48/100 rate_remaining>=13835 slowest_page=1128ms

## PASS - carriers
- note: completeness proven by API totalCount=55
- pass 1: succeeded in=55 bronze=55 pages=1 last_page=55/100 rate_remaining>=13834 slowest_page=598ms

## PASS - cash_accounts
- pass 1: succeeded in=29 bronze=29 pages=1 last_page=29/100 rate_remaining>=13833 slowest_page=1773ms

## PASS - cash_transactions
- pass 1: succeeded in=490 bronze=490 pages=5 last_page=90/100 rate_remaining>=13827 slowest_page=5971ms

## PASS - contacts
- note: completeness proven by API totalCount=11727
- pass 1: succeeded in=11727 bronze=11727 pages=1 last_page=11727/100 rate_remaining>=13826 slowest_page=6110ms

## PASS - credit_terms
- note: completeness proven by API totalCount=22
- pass 1: succeeded in=22 bronze=22 pages=1 last_page=22/100 rate_remaining>=13825 slowest_page=516ms

## PASS - currencies
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=12 bronze=12 pages=1 last_page=12/100 rate_remaining>=13824 slowest_page=615ms

## PASS - currency_rates
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=7023 bronze=7023 pages=1 last_page=7023/0 rate_remaining>=13823 slowest_page=696ms

## PASS - customer_classes
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=3 bronze=3 pages=1 last_page=3/100 rate_remaining>=13821 slowest_page=548ms

## PASS - customer_documents
- note: completeness proven by API totalCount=23132
- pass 1: succeeded in=23132 bronze=23132 pages=232 last_page=32/100 rate_remaining>=13583 slowest_page=4810ms

## PASS - customer_locations
- note: completeness proven by API totalCount=14552
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=14552 bronze=14552 pages=146 last_page=52/100 rate_remaining>=13411 slowest_page=47869ms

## PASS - customer_payment_methods
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=2 bronze=2 pages=1 last_page=2/100 rate_remaining>=13410 slowest_page=600ms

## FAIL - customer_payments
- FAIL: rows lost between fetch and bronze: fetched 21392, bronze holds 21391
- FAIL: records_written 21391 != records_in 21392
- note: completeness proven by API totalCount=21392
- pass 1: succeeded in=21392 bronze=21391 pages=214 last_page=92/100 rate_remaining>=13176 slowest_page=38508ms

## PASS - customer_sales_prices
- note: completeness proven by API totalCount=17088
- pass 1: succeeded in=17088 bronze=17088 pages=171 last_page=88/100 rate_remaining>=13000 slowest_page=2490ms

## PASS - deferral_codes
- note: completeness proven by API totalCount=12
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=12 bronze=12 pages=1 last_page=12/100 rate_remaining>=12998 slowest_page=508ms

## PASS - departments
- pass 1: succeeded in=54 bronze=54 pages=1 last_page=54/100 rate_remaining>=12997 slowest_page=520ms

## PASS - dimensions
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=23 bronze=23 pages=1 last_page=23/100 rate_remaining>=12996 slowest_page=453ms

## PASS - discount_codes
- note: completeness proven by API totalCount=7
- pass 1: succeeded in=7 bronze=7 pages=1 last_page=7/100 rate_remaining>=12995 slowest_page=464ms

## PASS - earning_types
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=4 bronze=4 pages=1 last_page=4/100 rate_remaining>=12994 slowest_page=405ms

## PASS - employees
- note: completeness proven by API totalCount=111
- pass 1: succeeded in=111 bronze=111 pages=2 last_page=11/100 rate_remaining>=12992 slowest_page=992ms

## PASS - expense_claims
- note: completeness proven by API totalCount=1558
- pass 1: succeeded in=1558 bronze=1558 pages=16 last_page=58/100 rate_remaining>=12958 slowest_page=37865ms

## PASS - expense_transactions
- note: completeness proven by API totalCount=30580
- pass 1: succeeded in=30580 bronze=30580 pages=153 last_page=180/200 rate_remaining>=None slowest_page=22869ms

## PASS - financial_periods
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=72 bronze=72 pages=1 last_page=72/100 rate_remaining>=14898 slowest_page=818ms

## FAIL - fixed_asset_classes
- FAIL: endpoint has a modified_field but no checkpoint watermark was stored
- note: completeness proven by API totalCount=23
- pass 1: succeeded in=23 bronze=23 pages=1 last_page=23/100 rate_remaining>=14897 slowest_page=861ms

## PASS - fixed_asset_property_tax_groups
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14894 slowest_page=551ms

## PASS - fixed_asset_transactions
- note: completeness proven by API totalCount=1061
- pass 1: succeeded in=1061 bronze=1061 pages=11 last_page=61/100 rate_remaining>=14886 slowest_page=719ms

## FAIL - general_ledger_balances_v2
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='upstream returned non-success status 400: {"message":"At least one of the parameters should be provided: PeriodId, LastM')
- pass 1: failed in=0 bronze=0 pages=1 last_page=0/0 rate_remaining>=None slowest_page=0ms

## PASS - inventory_adjustments
- note: completeness proven by API totalCount=4689
- pass 1: succeeded in=4689 bronze=4689 pages=47 last_page=89/100 rate_remaining>=14349 slowest_page=44555ms

## PASS - inventory_issues
- note: completeness proven by API totalCount=21587
- pass 1: succeeded in=21587 bronze=21587 pages=216 last_page=87/100 rate_remaining>=13728 slowest_page=68352ms

## PASS - inventory_receipts
- note: completeness proven by API totalCount=2504
- pass 1: succeeded in=2504 bronze=2504 pages=26 last_page=4/100 rate_remaining>=13689 slowest_page=58049ms

## PASS - inventory_transfers
- note: completeness proven by API totalCount=736
- pass 1: succeeded in=736 bronze=736 pages=8 last_page=36/100 rate_remaining>=13681 slowest_page=616ms

## FAIL - journal_transactions
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='upstream returned non-success status 400: {"message":"\'LastModifiedDateTime\' is not allowed to be more than 6 month back')
- pass 1: failed in=0 bronze=0 pages=1 last_page=0/0 rate_remaining>=None slowest_page=0ms

## PASS - kit_specifications
- note: completeness proven by API totalCount=643
- pass 1: succeeded in=643 bronze=643 pages=7 last_page=43/100 rate_remaining>=13673 slowest_page=1597ms

## PASS - landed_cost_codes
- note: completeness proven by API totalCount=13
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=13 bronze=13 pages=1 last_page=13/100 rate_remaining>=13671 slowest_page=553ms

## PASS - ledgers
- pass 1: succeeded in=4 bronze=4 pages=1 last_page=4/100 rate_remaining>=13670 slowest_page=484ms

## PASS - locations
- note: completeness proven by API totalCount=14552
- pass 1: succeeded in=14552 bronze=14552 pages=146 last_page=52/100 rate_remaining>=13510 slowest_page=49873ms

## PASS - lot_serial_classes
- note: completeness proven by API totalCount=1
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14858 slowest_page=413ms

## PASS - number_sequences
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=82 bronze=82 pages=1 last_page=82/100 rate_remaining>=14834 slowest_page=2284ms

## PASS - packaging_types
- note: completeness proven by API totalCount=1
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14833 slowest_page=419ms

## PASS - payment_methods
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=8 bronze=8 pages=1 last_page=8/100 rate_remaining>=14857 slowest_page=441ms

## PASS - project_budgets
- note: completeness proven by API totalCount=5245
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=5245 bronze=5245 pages=53 last_page=45/100 rate_remaining>=14715 slowest_page=5585ms

## PASS - project_transactions
- note: completeness proven by API totalCount=481
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=481 bronze=481 pages=5 last_page=81/100 rate_remaining>=14700 slowest_page=1276ms

## PASS - purchase_orders
- note: completeness proven by API totalCount=2305
- pass 1: succeeded in=2305 bronze=2305 pages=24 last_page=5/100 rate_remaining>=14576 slowest_page=9119ms

## PASS - sales_categories
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14573 slowest_page=586ms

## PASS - sales_order_types
- note: completeness proven by API totalCount=30
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=30 bronze=30 pages=1 last_page=30/100 rate_remaining>=14570 slowest_page=764ms

## PASS - sales_persons
- note: completeness proven by API totalCount=114
- pass 1: succeeded in=114 bronze=114 pages=2 last_page=14/100 rate_remaining>=14567 slowest_page=4235ms

## PASS - shipments
- note: completeness proven by API totalCount=21556
- pass 1: succeeded in=21556 bronze=21556 pages=216 last_page=56/100 rate_remaining>=14201 slowest_page=23496ms

## PASS - stocktakes_v2
- note: completeness proven by API totalCount=22
- pass 1: succeeded in=22 bronze=22 pages=1 last_page=22/100 rate_remaining>=14189 slowest_page=5856ms

## PASS - subaccounts
- note: completeness proven by API totalCount=11802
- pass 1: succeeded in=11802 bronze=11802 pages=119 last_page=2/100 rate_remaining>=14032 slowest_page=2893ms

## PASS - supplier_classes
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=16 bronze=16 pages=1 last_page=16/100 rate_remaining>=14024 slowest_page=693ms

## FAIL - supplier_locations
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 1 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: missing validation field l')
- pass 1: failed in=6891 bronze=0 pages=69 last_page=91/100 rate_remaining>=13942 slowest_page=2289ms

## PASS - supplier_payments
- note: completeness proven by API totalCount=9132
- pass 1: succeeded in=9132 bronze=9132 pages=92 last_page=32/100 rate_remaining>=13848 slowest_page=3342ms

## FAIL - unit_of_measures
- FAIL: endpoint has a modified_field but no checkpoint watermark was stored
- pass 1: succeeded in=43 bronze=43 pages=1 last_page=43/100 rate_remaining>=13847 slowest_page=612ms

## PASS - vat_categories
- pass 1: succeeded in=48 bronze=48 pages=1 last_page=48/100 rate_remaining>=13846 slowest_page=1054ms

## PASS - vat_zones
- note: completeness proven by API totalCount=15
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=15 bronze=15 pages=1 last_page=15/100 rate_remaining>=13845 slowest_page=563ms

## PASS - vats
- pass 1: succeeded in=67 bronze=67 pages=1 last_page=67/100 rate_remaining>=13844 slowest_page=2991ms

## PASS - warehouses
- note: completeness proven by API totalCount=15
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=15 bronze=15 pages=1 last_page=15/100 rate_remaining>=13842 slowest_page=1303ms
