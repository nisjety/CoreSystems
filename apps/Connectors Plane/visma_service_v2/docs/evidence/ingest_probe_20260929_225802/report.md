# Ingestion probe ingest_probe_20260929_225802

## PASS - sales_order_customers
- note: completeness proven by API totalCount=6359
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=6359 bronze=6359 pages=64 last_page=59/100 rate_remaining>=None slowest_page=2012ms

## PASS - sales_order_inventory
- note: completeness proven by API totalCount=5806
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=5806 bronze=5806 pages=59 last_page=6/100 rate_remaining>=None slowest_page=783ms

## FAIL - sales_orders_v3
- FAIL: endpoint has a modified_field but no checkpoint watermark was stored
- note: completeness proven by API totalCount=22022
- pass 1: succeeded in=22022 bronze=22022 pages=221 last_page=22/100 rate_remaining>=None slowest_page=2382ms
