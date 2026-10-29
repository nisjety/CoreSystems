# Ingestion probe ingest_probe_20260930_203041

## PASS - project_transactions
- note: fetched one financial period at a time; each period's walk is checked against its own API totalCount by the walk guard
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=4967 bronze=4967 pages=91 last_page=1/100 rate_remaining>=14468 slowest_page=10954ms
- pass 2: succeeded in=4967 bronze=4967 pages=91 last_page=1/100 rate_remaining>=14377 slowest_page=3659ms
