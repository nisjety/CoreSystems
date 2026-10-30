# 2-pass local 5-day validation

- Artifact directory: `docs/evidence/validation_2pass_5day_20260505_164856`
- Tenant: `33f319b9-4d57-11ee-960c-025417856183`
- Window: `2026-04-29T00:00:00Z` .. `2026-05-03T23:59:59Z`
- Endpoint count: total=73, enabled=70, disabled=3
- Main table chosen for counts: `vsv2.local_bronze_record`
- Run1: pass=1, fail=0
- Run2: pass=1, fail=0

## Findings
- Duplicate-risk endpoints (delta2>0): None
- Deletion-risk endpoints (delta1<0 or delta2<0): None
- Zero-increment endpoints (delta1==0): accounts

## Top failing endpoints
- None
