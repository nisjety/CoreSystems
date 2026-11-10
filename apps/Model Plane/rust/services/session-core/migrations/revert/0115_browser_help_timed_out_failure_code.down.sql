-- Reverses 0115_browser_help_timed_out_failure_code.sql: restores the failure_code CHECK that 0088 left (every code
-- 0088 accepts, and not 'browser_help_timed_out').
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction. Then delete the version row so
-- the next boot re-applies 0115 cleanly (docs/runbooks/migration-policy.md, "Rolling back past a contract migration").
--
-- Valid ONLY while no outbox row says 'browser_help_timed_out' (re-adding the CHECK validates every existing row).
-- Check first, read-only:
--
--   SELECT count(*) FROM managed_run_terminalization_outbox
--    WHERE failure_code = 'browser_help_timed_out';
--
-- ORDER, all three steps:
--   1. roll session-core back to a build whose terminalization no longer writes 'browser_help_timed_out', or keep
--      SESSION_CORE_RECOVERY_APPROVAL_EXPIRY_ENABLED off and make sure no such row was written while it was on;
--   2. confirm the query above returns 0;
--   3. run this file, then DELETE FROM _sqlx_migrations WHERE version = 115.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

ALTER TABLE managed_run_terminalization_outbox
    DROP CONSTRAINT IF EXISTS managed_run_terminalization_outbox_failure_code_check;

ALTER TABLE managed_run_terminalization_outbox
    ADD CONSTRAINT managed_run_terminalization_outbox_failure_code_check
    CHECK (failure_code IS NULL OR failure_code IN (
        'browser_failed',
        'browser_permission_denied',
        'browser_resource_exhausted',
        'browser_timed_out',
        'dispatch_rejected',
        'dispatch_unreachable',
        'execution_failed',
        'inference_failed',
        'outcome_unknown',
        'provider_timeout',
        'provider_unavailable',
        'question_expired'
    ));

COMMIT;
