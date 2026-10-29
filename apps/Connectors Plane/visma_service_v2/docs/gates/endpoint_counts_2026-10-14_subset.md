# Endpoint counts: Visma against staging (2026-10-14)

Rule: FAIL when more records are missing from staging than Visma reports as modified
since the last write. Where an endpoint has no modified filter that works, or a tenant
has no write on record, the tolerance is 0. See scripts/verify_endpoint_counts.py.

- Last write: the heartbeat table where a row exists, else the last staging write.
- Scope: 1 endpoints counted for 1 tenants (1 cells); 1 NOT COUNTABLE.
- Cells: PASS 1 (1 exact, 0 within the tolerance), WARN 0, FAIL 0, ERROR 0
- Visma calls: {'token': 1, 'GET': 1}.

## Per endpoint

Missing is summed over the tenants where records are missing; Allowed is what excuses
it (the modified-since count, 0 where no filter works).

| Endpoint | PASS | WARN | FAIL | ERROR | Visma | Staging | Missing | Allowed | Modified filter | Method |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| general_ledger_transactions | 1 | 0 | 0 | 0 | 589,689 | 589,689 | 0 | 0 | not needed | finance list, one row (ledger=1, fromPeriod=202001, toPeriod=203012) |

## Cells that are not PASS

None.

## NOT COUNTABLE

| Endpoint | Reason |
|---|---|
| journal_transactions | period-scanned and line-counted: periodId is required (HTTP 400 without it) and the total counts journal lines, while staging holds one row per batch. |
