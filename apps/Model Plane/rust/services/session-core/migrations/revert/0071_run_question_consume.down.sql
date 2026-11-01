-- Reverses 0071_run_question_consume.sql: drops run_questions.consumed_claim_hash and
-- run_questions.consumed_at, and restores the failure_code CHECK that 0069 left (every code 0069
-- accepts, and not 'question_expired').
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top
-- level of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Valid ONLY while no outbox row says 'question_expired' (re-adding the CHECK validates every
-- existing row). Check first, read-only:
--
--   SELECT count(*) FROM managed_run_terminalization_outbox
--    WHERE failure_code = 'question_expired';
--
-- ORDER, all three steps:
--   1. roll session-core back to a build whose terminalization no longer writes 'question_expired';
--   2. confirm the query above returns 0;
--   3. run this file.
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

ALTER TABLE run_questions
    DROP CONSTRAINT IF EXISTS run_questions_consumed_shape;

ALTER TABLE run_questions
    DROP COLUMN IF EXISTS consumed_claim_hash;

ALTER TABLE run_questions
    DROP COLUMN IF EXISTS consumed_at;

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
        'provider_unavailable'
    ));

COMMIT;
