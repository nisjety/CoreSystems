# Ingestion probe ingest_probe_20260929_181052

## PASS - vat_zones
- note: completeness proven by API totalCount=15
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=15 bronze=15 pages=1 last_page=15/100 rate_remaining>=14754 slowest_page=559ms
- pass 2: succeeded in=15 bronze=15 pages=1 last_page=15/100 rate_remaining>=14753 slowest_page=369ms

## PASS - vat_categories
- pass 1: succeeded in=48 bronze=48 pages=1 last_page=48/100 rate_remaining>=14751 slowest_page=1560ms
- pass 2: succeeded in=47 bronze=48 pages=1 last_page=47/100 rate_remaining>=14750 slowest_page=2092ms

## PASS - suppliers
- note: completeness proven by API totalCount=1273
- pass 1: succeeded in=1273 bronze=1273 pages=13 last_page=73/100 rate_remaining>=14729 slowest_page=3221ms
- pass 2: succeeded in=4 bronze=1273 pages=1 last_page=4/100 rate_remaining>=14727 slowest_page=887ms

## PASS - supplier_payments
- note: completeness proven by API totalCount=9132
- pass 1: succeeded in=9132 bronze=9132 pages=92 last_page=32/100 rate_remaining>=14611 slowest_page=3558ms
- pass 2: succeeded in=1 bronze=9132 pages=1 last_page=1/100 rate_remaining>=14608 slowest_page=699ms
