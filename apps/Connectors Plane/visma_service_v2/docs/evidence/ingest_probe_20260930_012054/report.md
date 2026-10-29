# Ingestion probe ingest_probe_20260930_012054

## PASS - supplier_locations
- note: completeness proven by API totalCount=6891
- pass 1: succeeded in=6891 bronze=6891 pages=69 last_page=91/100 rate_remaining>=13702 slowest_page=2774ms

## PASS - unit_of_measures
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=43 bronze=43 pages=1 last_page=43/100 rate_remaining>=13701 slowest_page=656ms
