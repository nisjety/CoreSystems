-- Reverses 0107_retention_sweep_runs.sql (renumbered from 0095): drops the retention sweep run table.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER: set SESSION_RETENTION_SWEEP=off (or roll session-core back to a build without
-- src/retention_sweeper.rs) first, or the sweeper fails on the missing table. The rows hold counts only;
-- their audit events already left through session_audit_outbox, so nothing is copied first.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DROP TABLE IF EXISTS retention_sweep_runs;

COMMIT;
