-- Reverses 0101_run_request_snapshots.sql: drops the request snapshot store.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: roll session-core back to a build without RunLedgerService.RecordRequestSnapshot and the snapshot
-- sweep first (or keep SESSION_CORE_REQUEST_SNAPSHOTS_ENABLED and SESSION_CORE_REQUEST_SNAPSHOTS_SWEEP_ENABLED
-- off), or they fail on the missing tables. The rows hold at most 30 days of debugging data; nothing else
-- reads them, so nothing is copied first. `runs.metadata.snapshots_dropped` is left in place: it is a counter
-- inside a JSONB column, and no reader depends on it once the store is gone.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP TABLE IF EXISTS run_request_snapshot_blobs;

DROP TABLE IF EXISTS run_request_snapshots;

COMMIT;
