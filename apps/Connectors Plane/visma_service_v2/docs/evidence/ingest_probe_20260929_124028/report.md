# Ingestion probe ingest_probe_20260929_124028

## PASS - branches
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14563 slowest_page=471ms
- pass 2: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14562 slowest_page=514ms

## PASS - currencies
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=12 bronze=12 pages=1 last_page=12/100 rate_remaining>=14561 slowest_page=418ms
- pass 2: succeeded in=12 bronze=12 pages=1 last_page=12/100 rate_remaining>=14560 slowest_page=283ms

## FAIL - departments
- FAIL: endpoint has a modified_field but no checkpoint watermark was stored
- pass 1: succeeded in=54 bronze=54 pages=1 last_page=54/100 rate_remaining>=14559 slowest_page=428ms
- pass 2: succeeded in=54 bronze=54 pages=1 last_page=54/100 rate_remaining>=14558 slowest_page=550ms

## PASS - accounts
- note: completeness proven by API totalCount=614
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=614 bronze=614 pages=1 last_page=614/100 rate_remaining>=14557 slowest_page=644ms
- pass 2: succeeded in=614 bronze=614 pages=1 last_page=614/100 rate_remaining>=14556 slowest_page=799ms

## PASS - customers
- note: completeness proven by API totalCount=6586
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=6586 bronze=6586 pages=66 last_page=86/100 rate_remaining>=14490 slowest_page=14610ms
- pass 2: succeeded in=6586 bronze=6586 pages=66 last_page=86/100 rate_remaining>=14424 slowest_page=3685ms

## FAIL - suppliers
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 484 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: missing validation field')
- pass 1: failed in=1273 bronze=0 pages=13 last_page=73/100 rate_remaining>=14411 slowest_page=5387ms

## PASS - customer_invoices
- note: completeness proven by API totalCount=1628
- pass 1: succeeded in=1628 bronze=1628 pages=17 last_page=28/100 rate_remaining>=14394 slowest_page=3318ms
- pass 2: succeeded in=66 bronze=1628 pages=1 last_page=66/100 rate_remaining>=14393 slowest_page=1956ms
