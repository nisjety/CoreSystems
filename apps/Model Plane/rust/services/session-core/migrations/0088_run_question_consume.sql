-- Same-run resume delivery for run questions (build plan WS14 S5).
--
-- A `resume` question pauses its run as awaiting_input, and the owner's answer continues that
-- same run through /continue. This migration holds the schema facts that the resume path
-- needs; it adds no table and rewrites no row.
--
--   consumed_at          the single-use mark of a `resume` answer. ConsumeRunQuestion (session-core)
--                        spends an answer once: a compare-and-set on `consumed_at IS NULL`, and a
--                        second spend by another claim is refused. Only an answered `resume`
--                        question can carry the mark (the CHECK).
--   consumed_claim_hash  the BLAKE3 hash of the claim token that spent the answer, so a retry with
--                        the same claim replays the answer and another claim is refused. It is set
--                        together with consumed_at and never without it (the CHECK).
--   failure_code         'question_expired' joins the managed-run terminal failure codes. The
--                        recovery worker writes it when the run's `resume` question lapsed
--                        unanswered. A caller may not submit it, the same rule as 'outcome_unknown'.
--
-- The CHECK list is the 0069 list plus 'question_expired'. It must match terminalization's
-- recovery-written codes in session-core. Additive: every value accepted before is still accepted,
-- and an existing row keeps its NULL consumed_at and consumed_claim_hash. The reversal is in
-- revert/0088_run_question_consume.down.sql.

ALTER TABLE run_questions
    ADD COLUMN IF NOT EXISTS consumed_at TIMESTAMPTZ;

ALTER TABLE run_questions
    ADD COLUMN IF NOT EXISTS consumed_claim_hash TEXT
        CHECK (consumed_claim_hash IS NULL OR length(consumed_claim_hash) = 64);

ALTER TABLE run_questions
    DROP CONSTRAINT IF EXISTS run_questions_consumed_shape;

ALTER TABLE run_questions
    ADD CONSTRAINT run_questions_consumed_shape CHECK (
        (consumed_at IS NULL OR (state = 'answered' AND delivery = 'resume'))
        AND ((consumed_at IS NULL) = (consumed_claim_hash IS NULL))
    );

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
