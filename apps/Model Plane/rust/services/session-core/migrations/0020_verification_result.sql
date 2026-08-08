-- Verified Outcome Foundation (verevon-roadmap.md §3b): the independent
-- judgment on whether a completed/failed continuation's claimed outcome
-- actually happened, distinct from `outcome` itself (a workflow-state
-- concept: did the dispatcher consider this delivery done). All three
-- columns are nullable together — an older worker that never sends a
-- verification result leaves them NULL, never a fabricated default.

ALTER TABLE approval_continuation_outcomes
    ADD COLUMN IF NOT EXISTS verification_status TEXT
        CHECK (verification_status IS NULL OR verification_status IN (
            'unknown', 'verified_success', 'verified_failure', 'partially_verified'
        ));

ALTER TABLE approval_continuation_outcomes
    ADD COLUMN IF NOT EXISTS verification_method TEXT
        CHECK (verification_method IS NULL OR length(verification_method) BETWEEN 1 AND 64);

ALTER TABLE approval_continuation_outcomes
    ADD COLUMN IF NOT EXISTS verification_reason TEXT
        CHECK (verification_reason IS NULL OR length(verification_reason) BETWEEN 0 AND 512);

ALTER TABLE approval_continuation_outcomes
    ADD CONSTRAINT approval_continuation_outcomes_verification_together_chk
    CHECK (
        (verification_status IS NULL AND verification_method IS NULL AND verification_reason IS NULL)
        OR
        (verification_status IS NOT NULL AND verification_method IS NOT NULL)
    );
