# Ingestion probe ingest_probe_20260929_230918

## PASS - branches
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14657 slowest_page=659ms

## PASS - cash_sales
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14656 slowest_page=673ms

## PASS - customer_contracts
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=14654 slowest_page=829ms

## PASS - customer_credit_notes
- note: completeness proven by API totalCount=720
- pass 1: succeeded in=720 bronze=720 pages=8 last_page=20/100 rate_remaining>=14565 slowest_page=18099ms

## PASS - customer_credit_write_offs
- note: completeness proven by API totalCount=20
- pass 1: succeeded in=20 bronze=20 pages=1 last_page=20/100 rate_remaining>=14559 slowest_page=2777ms

## PASS - customer_debit_notes
- note: completeness proven by API totalCount=17
- pass 1: succeeded in=17 bronze=17 pages=1 last_page=17/100 rate_remaining>=14553 slowest_page=2498ms

## FAIL - customer_documents
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 1 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: duplicate primary key with')
- pass 1: failed in=23132 bronze=0 pages=232 last_page=32/100 rate_remaining>=14019 slowest_page=13060ms

## FAIL - customer_invoices
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='upstream returned non-success status 502: <!DOCTYPE html>')
- pass 1: failed in=0 bronze=0 pages=1 last_page=0/0 rate_remaining>=None slowest_page=0ms

## PASS - customer_overdue_charges
- note: pass 1 fetched 0 rows - nothing was proven (widen the window or pick another endpoint)
- pass 1: succeeded in=0 bronze=0 pages=1 last_page=0/100 rate_remaining>=12978 slowest_page=918ms

## PASS - customers
- note: completeness proven by API totalCount=6586
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=6586 bronze=6586 pages=66 last_page=86/100 rate_remaining>=12929 slowest_page=70247ms

## PASS - fixed_assets
- note: completeness proven by API totalCount=69
- pass 1: succeeded in=69 bronze=69 pages=1 last_page=69/100 rate_remaining>=14828 slowest_page=2114ms

## FAIL - inventory_items
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='state store error: error returned from database: unsupported Unicode escape sequence')
- pass 1: failed in=7252 bronze=0 pages=73 last_page=52/100 rate_remaining>=14658 slowest_page=23053ms

## PASS - kit_assemblies
- note: completeness proven by API totalCount=249
- pass 1: succeeded in=249 bronze=249 pages=3 last_page=49/100 rate_remaining>=14644 slowest_page=11874ms

## PASS - organizations
- pass 1: succeeded in=1 bronze=1 pages=1 last_page=1/100 rate_remaining>=14642 slowest_page=12896ms

## PASS - price_classes
- note: completeness proven by API totalCount=6586
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=6586 bronze=6586 pages=66 last_page=86/100 rate_remaining>=14297 slowest_page=19032ms

## FAIL - project_account_groups
- FAIL: endpoint has a modified_field but no checkpoint watermark was stored
- pass 1: succeeded in=250 bronze=250 pages=1 last_page=250/0 rate_remaining>=14294 slowest_page=1673ms

## PASS - projects
- note: completeness proven by API totalCount=1237
- pass 1: succeeded in=1237 bronze=1237 pages=13 last_page=37/100 rate_remaining>=14279 slowest_page=1768ms

## PASS - purchase_receipt_transfers
- note: completeness proven by API totalCount=5
- note: no modified_field: every sync is a full re-fetch (no incremental watermark)
- pass 1: succeeded in=5 bronze=5 pages=1 last_page=5/100 rate_remaining>=14278 slowest_page=1224ms

## PASS - purchase_receipts
- note: completeness proven by API totalCount=3025
- pass 1: succeeded in=3025 bronze=3025 pages=31 last_page=25/100 rate_remaining>=14244 slowest_page=1481ms

## PASS - supplier_documents
- note: completeness proven by API totalCount=11512
- pass 1: succeeded in=11512 bronze=11512 pages=116 last_page=12/100 rate_remaining>=14119 slowest_page=1351ms

## FAIL - supplier_invoices
- FAIL: pass 1 did not succeed (exit=1, status='failed', error='sync validation failed: 3 rejected row(s), 0 exact duplicate row(s) dropped; first rejection: duplicate primary key with')
- pass 1: failed in=11512 bronze=0 pages=116 last_page=12/100 rate_remaining>=13977 slowest_page=5800ms

## PASS - suppliers
- note: completeness proven by API totalCount=1273
- pass 1: succeeded in=1273 bronze=1273 pages=13 last_page=73/100 rate_remaining>=13961 slowest_page=7319ms
