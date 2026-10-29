# Ingestion probe ingest_probe_20260929_224426

## PASS - countries
- note: completeness proven by API totalCount=260
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=260 bronze=260 pages=3 last_page=60/100 rate_remaining>=14498 slowest_page=517ms

## PASS - currency_rate_types
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=2 bronze=2 pages=1 last_page=2/100 rate_remaining>=14497 slowest_page=451ms

## PASS - customer_balances
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=3087 bronze=3087 pages=31 last_page=87/100 rate_remaining>=14466 slowest_page=2810ms

## PASS - customer_balances_v2
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=3036 bronze=3036 pages=31 last_page=36/100 rate_remaining>=14435 slowest_page=2353ms

## PASS - customer_credit_write_offs
- note: completeness proven by API totalCount=20
- pass 1: succeeded in=20 bronze=20 pages=1 last_page=20/100 rate_remaining>=14434 slowest_page=648ms

## PASS - discounts
- note: completeness proven by API totalCount=129
- pass 1: succeeded in=129 bronze=129 pages=2 last_page=29/100 rate_remaining>=14432 slowest_page=442ms

## FAIL - expense_receipts
- FAIL: pass 1 did not succeed (exit=1, status='running', error='')
- pass 1: running in=0 bronze=0 pages=0 last_page=0/0 rate_remaining>=None slowest_page=0ms

## PASS - inventory_item_classes
- note: completeness proven by API totalCount=22
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=22 bronze=22 pages=1 last_page=22/100 rate_remaining>=14402 slowest_page=1226ms

## PASS - inventory_item_post_classes
- note: completeness proven by API totalCount=22
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=22 bronze=22 pages=1 last_page=22/100 rate_remaining>=14401 slowest_page=511ms

## PASS - inventory_receipt_transfers
- note: completeness proven by API totalCount=1
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14400 slowest_page=638ms

## PASS - languages
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=6 bronze=6 pages=1 last_page=6/100 rate_remaining>=14399 slowest_page=387ms

## FAIL - payments
- FAIL: rows lost between fetch and bronze: fetched 21392, bronze holds 21391
- FAIL: records_written 21391 != records_in 21392
- note: completeness proven by API totalCount=21392
- pass 1: succeeded in=21392 bronze=21391 pages=214 last_page=92/100 rate_remaining>=14134 slowest_page=63137ms

## PASS - purchase_receipt_transfers
- note: completeness proven by API totalCount=5
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=5 bronze=5 pages=1 last_page=5/100 rate_remaining>=14133 slowest_page=627ms

## PASS - supplier_balances_v2
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=602 bronze=602 pages=7 last_page=2/100 rate_remaining>=14126 slowest_page=1924ms

## PASS - supplier_inbox_rules
- note: completeness proven by API totalCount=30
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=30 bronze=30 pages=1 last_page=30/100 rate_remaining>=14125 slowest_page=881ms
