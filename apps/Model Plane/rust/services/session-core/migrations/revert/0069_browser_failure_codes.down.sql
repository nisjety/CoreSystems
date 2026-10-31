-- Reverses 0069_browser_failure_codes.sql: restores the 0033 failure_code CHECK.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only,
-- inside one transaction.
--
-- Valid ONLY while no row uses one of the codes 0069 added: re-adding the CHECK
-- validates every existing row, so it fails while any outbox row still says
-- 'browser_permission_denied', 'browser_resource_exhausted' or 'browser_timed_out'.
-- Check first, read-only:
--
--   SELECT failure_code, count(*) FROM managed_run_terminalization_outbox
--    WHERE failure_code IN ('browser_permission_denied',
--                           'browser_resource_exhausted',
--                           'browser_timed_out')
--    GROUP BY failure_code;
--
-- ORDER, all three steps:
--   1. roll session-core back to a build whose ALLOWED_FAILURE_CODES lacks the three
--      codes (Gateway then falls back to 'browser_failed' on its own);
--   2. confirm the query above returns no rows;
--   3. run this file.
--
-- Safe to run twice (the DROP is IF EXISTS).

BEGIN;

ALTER TABLE managed_run_terminalization_outbox
    DROP CONSTRAINT IF EXISTS managed_run_terminalization_outbox_failure_code_check;

ALTER TABLE managed_run_terminalization_outbox
    ADD CONSTRAINT managed_run_terminalization_outbox_failure_code_check
    CHECK (failure_code IS NULL OR failure_code IN (
        'browser_failed',
        'dispatch_rejected',
        'dispatch_unreachable',
        'execution_failed',
        'inference_failed',
        'outcome_unknown',
        'provider_timeout',
        'provider_unavailable'
    ));

COMMIT;
