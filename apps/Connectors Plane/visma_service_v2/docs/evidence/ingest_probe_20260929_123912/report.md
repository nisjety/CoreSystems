# Ingestion probe ingest_probe_20260929_123912

## PASS - branches
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14571 slowest_page=484ms
- pass 2: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14570 slowest_page=323ms

## FAIL - currencies
- FAIL: no checkpoint watermark stored after a successful pass 1
- pass 1: succeeded in=12 bronze=12 pages=1 last_page=12/100 rate_remaining>=14569 slowest_page=333ms
- pass 2: succeeded in=12 bronze=12 pages=1 last_page=12/100 rate_remaining>=14568 slowest_page=338ms

## FAIL - departments
- FAIL: no checkpoint watermark stored after a successful pass 1
- pass 1: succeeded in=54 bronze=54 pages=1 last_page=54/100 rate_remaining>=14567 slowest_page=539ms
- pass 2: succeeded in=54 bronze=54 pages=1 last_page=54/100 rate_remaining>=14566 slowest_page=317ms

## FAIL - accounts
- FAIL: last page holds 614 of 100 rows: pagination may have stopped early
- FAIL: no checkpoint watermark stored after a successful pass 1
- pass 1: succeeded in=614 bronze=614 pages=1 last_page=614/100 rate_remaining>=14565 slowest_page=584ms
- pass 2: succeeded in=614 bronze=614 pages=1 last_page=614/100 rate_remaining>=14564 slowest_page=550ms
