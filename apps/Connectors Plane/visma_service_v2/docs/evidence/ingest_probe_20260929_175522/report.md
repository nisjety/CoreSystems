# Ingestion probe ingest_probe_20260929_175522

## PASS - countries
- pass 1: succeeded in=260 bronze=260 pages=1 last_page=260/500 rate_remaining>=None slowest_page=532ms
- pass 2: succeeded in=1 bronze=260 pages=1 last_page=1/500 rate_remaining>=None slowest_page=547ms

## PASS - states
- pass 1: succeeded in=806 bronze=806 pages=2 last_page=306/500 rate_remaining>=None slowest_page=490ms
- pass 2: succeeded in=806 bronze=806 pages=2 last_page=306/500 rate_remaining>=None slowest_page=548ms

## PASS - addresses
- pass 1: succeeded in=37699 bronze=37699 pages=76 last_page=199/500 rate_remaining>=None slowest_page=651ms
- pass 2: succeeded in=603 bronze=37699 pages=2 last_page=103/500 rate_remaining>=None slowest_page=699ms

## PASS - vat_zones
- note: completeness proven by API totalCount=15
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=15 bronze=15 pages=1 last_page=15/100 rate_remaining>=14573 slowest_page=543ms
- pass 2: succeeded in=15 bronze=15 pages=1 last_page=15/100 rate_remaining>=14572 slowest_page=464ms

## PASS - vat_categories
- pass 1: succeeded in=48 bronze=48 pages=1 last_page=48/100 rate_remaining>=14571 slowest_page=1157ms
- pass 2: succeeded in=47 bronze=48 pages=1 last_page=47/100 rate_remaining>=14570 slowest_page=1031ms

## FAIL - supplier_payments
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 25 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: duplicate primary key wit')
- pass 1: failed in=9132 bronze=0 pages=92 last_page=32/100 rate_remaining>=14478 slowest_page=8278ms
