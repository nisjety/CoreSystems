-- A skipped `resume` question is spendable (build plan WS14 S5, unit b1; D2-1162, D2-1168).
--
-- 0088 allowed the single-use mark (consumed_at, with its claim hash) only on an `answered` resume
-- question. A skip is an answer that continues the run too (the owner chose to carry on without
-- choosing), and its continuation can be retried, so a skipped question must be spendable once and
-- replayable by the same claim. ConsumeRunQuestion (session-core) now spends `answered` and `skipped`
-- rows alike, inside the answer window and on a live run.
--
--   run_questions_consumed_shape  the 0088 CHECK, widened by one state: consumed_at may be set only on
--                                 an `answered` or `skipped` row of `resume` delivery, and it is set
--                                 together with consumed_claim_hash and never without it.
--
-- Additive: every row the 0088 CHECK accepted is still accepted, no column is added or rewritten, and
-- no existing row changes. Nothing writes a skipped consumed row until the resume path is switched on
-- (AGENT_ASK_USER_RESUME_ENABLED), so behaviour is unchanged until then. The reversal is in
-- revert/0127_run_question_resume.down.sql.

ALTER TABLE run_questions
    DROP CONSTRAINT IF EXISTS run_questions_consumed_shape;

ALTER TABLE run_questions
    ADD CONSTRAINT run_questions_consumed_shape CHECK (
        (consumed_at IS NULL OR (state IN ('answered', 'skipped') AND delivery = 'resume'))
        AND ((consumed_at IS NULL) = (consumed_claim_hash IS NULL))
    );
