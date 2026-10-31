# Ingestion probe ingest_probe_20260929_124756

## PASS - departments
- pass 1: succeeded in=54 bronze=54 pages=1 last_page=54/100 rate_remaining>=14379 slowest_page=617ms
- pass 2: succeeded in=1 bronze=54 pages=1 last_page=1/100 rate_remaining>=14378 slowest_page=366ms

## PASS - suppliers
- note: completeness proven by API totalCount=1273
- pass 1: succeeded in=1273 bronze=1273 pages=13 last_page=73/100 rate_remaining>=14365 slowest_page=1940ms
- pass 2: succeeded in=4 bronze=1273 pages=1 last_page=4/100 rate_remaining>=14364 slowest_page=1037ms

## PASS - customer_invoices
- note: completeness proven by API totalCount=1628
- pass 1: succeeded in=1628 bronze=1628 pages=17 last_page=28/100 rate_remaining>=14347 slowest_page=6465ms
- pass 2: succeeded in=66 bronze=1628 pages=1 last_page=66/100 rate_remaining>=14346 slowest_page=1554ms

## PASS - customers (crash-restart)
- note: killed run stays 'running' in sync_run forever (no stale-run reaper)
- pass 1: running in=0 bronze=0 pages=0 last_page=0/0 rate_remaining>=None slowest_page=0ms
- pass 2: succeeded in=6586 bronze=6586 pages=66 last_page=86/100 rate_remaining>=14274 slowest_page=6131ms
