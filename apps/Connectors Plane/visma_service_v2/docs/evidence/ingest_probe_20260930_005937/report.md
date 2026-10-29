# Ingestion probe ingest_probe_20260930_005937

## PASS - expense_receipts
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=13337 bronze=13337 pages=1 last_page=13337/0 rate_remaining>=14952 slowest_page=313922ms

## PASS - fixed_asset_classes
- note: completeness proven by API totalCount=23
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=23 bronze=23 pages=1 last_page=23/100 rate_remaining>=14985 slowest_page=520ms

## PASS - inventory_items
- note: completeness proven by API totalCount=7252
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=7252 bronze=7252 pages=73 last_page=52/100 rate_remaining>=14840 slowest_page=14799ms

## PASS - project_account_groups
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=250 bronze=250 pages=1 last_page=250/0 rate_remaining>=14832 slowest_page=1399ms

## PASS - supplier_invoices
- note: completeness proven by API totalCount=11512
- pass 1: succeeded in=11512 bronze=11512 pages=116 last_page=12/100 rate_remaining>=14393 slowest_page=10314ms
