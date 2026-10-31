# Ingestion probe ingest_probe_20260929_134705

## PASS - earning_types
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=4 bronze=4 pages=1 last_page=4/100 rate_remaining>=13891 slowest_page=420ms
- pass 2: succeeded in=4 bronze=4 pages=1 last_page=4/100 rate_remaining>=13890 slowest_page=326ms

## PASS - landed_cost_codes
- note: completeness proven by API totalCount=13
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=13 bronze=13 pages=1 last_page=13/100 rate_remaining>=13889 slowest_page=557ms
- pass 2: succeeded in=13 bronze=13 pages=1 last_page=13/100 rate_remaining>=13888 slowest_page=446ms

## PASS - sales_order_types
- note: completeness proven by API totalCount=30
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=30 bronze=30 pages=1 last_page=30/100 rate_remaining>=13887 slowest_page=473ms
- pass 2: succeeded in=30 bronze=30 pages=1 last_page=30/100 rate_remaining>=13886 slowest_page=509ms

## PASS - attributes
- note: completeness proven by API totalCount=48
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=48 bronze=48 pages=1 last_page=48/100 rate_remaining>=13885 slowest_page=562ms
- pass 2: succeeded in=48 bronze=48 pages=1 last_page=48/100 rate_remaining>=13884 slowest_page=747ms

## PASS - payment_methods
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=8 bronze=8 pages=1 last_page=8/100 rate_remaining>=13883 slowest_page=532ms
- pass 2: succeeded in=8 bronze=8 pages=1 last_page=8/100 rate_remaining>=13882 slowest_page=799ms

## PASS - employees
- note: completeness proven by API totalCount=111
- pass 1: succeeded in=111 bronze=111 pages=2 last_page=11/100 rate_remaining>=13880 slowest_page=567ms
- pass 2: succeeded in=1 bronze=111 pages=1 last_page=1/100 rate_remaining>=13879 slowest_page=529ms

## PASS - stocktakes_v2
- note: completeness proven by API totalCount=22
- pass 1: succeeded in=22 bronze=22 pages=1 last_page=22/100 rate_remaining>=13878 slowest_page=8750ms
- pass 2: succeeded in=1 bronze=22 pages=1 last_page=1/100 rate_remaining>=13877 slowest_page=1295ms
