# Ingestion probe ingest_probe_20260929_133008

## PASS - warehouses
- note: completeness proven by API totalCount=15
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=15 bronze=15 pages=1 last_page=15/100 rate_remaining>=14219 slowest_page=1011ms
- pass 2: succeeded in=15 bronze=15 pages=1 last_page=15/100 rate_remaining>=14218 slowest_page=1159ms
