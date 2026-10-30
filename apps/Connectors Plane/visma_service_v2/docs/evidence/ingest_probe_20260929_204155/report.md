# Ingestion probe ingest_probe_20260929_204155

## PASS - accounts
- note: completeness proven by API totalCount=614
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=614 bronze=614 pages=1 last_page=614/100 rate_remaining>=14573 slowest_page=782ms

## PASS - branches
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14572 slowest_page=541ms

## PASS - contacts
- note: completeness proven by API totalCount=11727
- pass 1: succeeded in=11727 bronze=11727 pages=1 last_page=11727/100 rate_remaining>=14571 slowest_page=5981ms

## PASS - credit_terms
- note: completeness proven by API totalCount=22
- pass 1: succeeded in=22 bronze=22 pages=1 last_page=22/100 rate_remaining>=14570 slowest_page=496ms

## PASS - customer_sales_prices
- note: completeness proven by API totalCount=17088
- pass 1: succeeded in=17088 bronze=17088 pages=171 last_page=88/100 rate_remaining>=14399 slowest_page=8958ms

## PASS - customers
- note: completeness proven by API totalCount=6586
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=6586 bronze=6586 pages=66 last_page=86/100 rate_remaining>=14333 slowest_page=2051ms

## PASS - locations
- note: completeness proven by API totalCount=14552
- pass 1: succeeded in=14552 bronze=14552 pages=146 last_page=52/100 rate_remaining>=14185 slowest_page=63847ms

## FAIL - sales_orders_v3
- FAIL: pass 1 did not succeed (exit=-15, status='running', error='')
- pass 1: running in=0 bronze=0 pages=0 last_page=0/0 rate_remaining>=None slowest_page=0ms

## PASS - sales_persons
- note: completeness proven by API totalCount=114
- pass 1: succeeded in=114 bronze=114 pages=2 last_page=14/100 rate_remaining>=14870 slowest_page=2150ms

## PASS - subaccounts
- note: completeness proven by API totalCount=11802
- pass 1: succeeded in=11802 bronze=11802 pages=119 last_page=2/100 rate_remaining>=14730 slowest_page=593ms
