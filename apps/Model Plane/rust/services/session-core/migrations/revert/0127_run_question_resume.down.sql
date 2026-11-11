-- Reverses 0127_run_question_resume.sql: clears the single-use mark of every skipped resume question and
-- restores the 0088 constraint text, under which only an `answered` resume question can carry it.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level
-- of migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER, both steps:
--   1. roll session-core back to a build whose ConsumeRunQuestion spends only `answered` rows, so no
--      skipped answer is spent while the constraint changes;
--   2. run this file.
--
-- A skipped question that was already spent loses its mark, so its continuation can claim it again.
-- That is the state the 0088 build would have left it in (a skip was never spendable there).
--
-- Safe to run twice (the DROP is IF EXISTS and the UPDATE matches nothing the second time).

BEGIN;

UPDATE run_questions
   SET consumed_at = NULL, consumed_claim_hash = NULL
 WHERE state = 'skipped';

ALTER TABLE run_questions
    DROP CONSTRAINT IF EXISTS run_questions_consumed_shape;

ALTER TABLE run_questions
    ADD CONSTRAINT run_questions_consumed_shape CHECK (
        (consumed_at IS NULL OR (state = 'answered' AND delivery = 'resume'))
        AND ((consumed_at IS NULL) = (consumed_claim_hash IS NULL))
    );

COMMIT;
