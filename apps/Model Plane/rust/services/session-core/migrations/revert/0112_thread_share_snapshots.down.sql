-- Reverses 0112_thread_share_snapshots.sql: drops the thread share snapshot table and its indexes.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set SESSION_THREAD_SHARES_ENABLED to anything but true (or roll session-core back to a build without
-- src/thread_shares.rs) first, or the snapshot RPCs fail on the missing table. The rows are copies of threads
-- that still exist, so nothing is copied first; their Control grants, if any were written, name snapshot ids
-- that no longer resolve and read as not found.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP INDEX IF EXISTS thread_share_snapshots_thread_idx;
DROP INDEX IF EXISTS thread_share_snapshots_owner_idx;
DROP TABLE IF EXISTS thread_share_snapshots;

COMMIT;
