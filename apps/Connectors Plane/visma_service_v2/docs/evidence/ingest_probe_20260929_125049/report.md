# Ingestion probe ingest_probe_20260929_125049

## FAIL - journal_transactions
- FAIL: API reports totalCount=3482 but only 24 rows were fetched
- note: completeness proven by API totalCount=3482
- note: 6 new key(s) appeared between passes (upstream changed)
- pass 1: succeeded in=24 bronze=24 pages=1 last_page=24/100 rate_remaining>=14273 slowest_page=1090ms
- pass 2: succeeded in=28 bronze=30 pages=1 last_page=28/100 rate_remaining>=14272 slowest_page=1954ms

## PASS - sales_orders_v3
- note: completeness proven by API totalCount=1037
- pass 1: succeeded in=1037 bronze=1037 pages=70 last_page=2/15 rate_remaining>=None slowest_page=1801ms
- pass 2: succeeded in=1 bronze=1037 pages=1 last_page=1/15 rate_remaining>=None slowest_page=638ms

## FAIL - expense_transactions
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='upstream returned non-success status 400: {"type":"business.validation_errors","title":"Bad Request","status":400,"detai')
- pass 1: failed in=0 bronze=0 pages=1 last_page=0/0 rate_remaining>=None slowest_page=0ms
