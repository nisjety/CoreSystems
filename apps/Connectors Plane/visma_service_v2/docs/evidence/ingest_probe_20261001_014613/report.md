# Ingestion probe ingest_probe_20261001_014613

## PASS - project_budgets
- note: completeness proven by API totalCount=5268
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=5268 bronze=5268 pages=53 last_page=68/100 rate_remaining>=14520 slowest_page=8100ms
- pass 2: succeeded in=5268 bronze=5268 pages=53 last_page=68/100 rate_remaining>=14467 slowest_page=5504ms

## PASS - expense_transactions
- note: completeness proven by API totalCount=30586
- pass 1: succeeded in=30586 bronze=30586 pages=153 last_page=186/200 rate_remaining>=None slowest_page=3032ms
- pass 2: succeeded in=6 bronze=30586 pages=1 last_page=6/200 rate_remaining>=None slowest_page=709ms

## PASS - project_tasks
- pass 1: succeeded in=3113 bronze=3113 pages=1239 last_page=2/100 rate_remaining>=13215 slowest_page=5330ms
- pass 2: succeeded in=2741 bronze=3113 pages=1239 last_page=2/100 rate_remaining>=12583 slowest_page=35527ms

## PASS - journal_transactions
- note: fetched one financial period at a time; 574967 lines in bronze, each period's walk checked against its own API totalCount (in lines) by the walk guard
- pass 1: succeeded in=67514 bronze=67514 pages=649 last_page=0/1000 rate_remaining>=13976 slowest_page=3208ms
- pass 2: succeeded in=67514 bronze=67514 pages=649 last_page=0/1000 rate_remaining>=13782 slowest_page=24589ms

## PASS - general_ledger_transactions
- note: completeness proven by API totalCount=574956
- pass 1: succeeded in=574956 bronze=574956 pages=575 last_page=956/1000 rate_remaining>=13425 slowest_page=48827ms
- pass 2: succeeded in=1403 bronze=574956 pages=2 last_page=403/1000 rate_remaining>=14737 slowest_page=600ms
