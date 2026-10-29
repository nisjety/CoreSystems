# Ingestion probe ingest_probe_20260930_172344

## PASS - general_ledger_balances_v2
- note: fetched one financial period at a time; each period's walk is checked against its own API totalCount by the walk guard
- pass 1: succeeded in=28363 bronze=28363 pages=348 last_page=91/100 rate_remaining>=12927 slowest_page=7531ms
- pass 2: succeeded in=28363 bronze=28363 pages=348 last_page=91/100 rate_remaining>=12507 slowest_page=4122ms

## PASS - project_transactions
- note: fetched one financial period at a time; each period's walk is checked against its own API totalCount by the walk guard
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=4967 bronze=4967 pages=91 last_page=1/100 rate_remaining>=12414 slowest_page=2983ms
- pass 2: succeeded in=4967 bronze=4967 pages=91 last_page=1/100 rate_remaining>=12323 slowest_page=4227ms
