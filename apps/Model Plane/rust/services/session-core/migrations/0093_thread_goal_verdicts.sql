-- 0093: thread goal verdicts (Wave 3 EP-7 S3; the number integration round B reserved for it).
--
-- The completion judge answers, after a goal turn settles, whether the owner's success criteria
-- are met. `met` closes the goal, `blocked` stops it, and `not_met` and `unsure` leave it open
-- (the owner then closes or reopens it by hand). The verdict is written on the turn it judged:
-- one row per turn, written once, since RecordGoalVerdict is idempotent per turn. The goal's last
-- verdict is read from its latest judged turn, so no column is cached on thread_goals.
--
-- Zero Data Retention: `verdict_reasons` and `evidence_ids` are model text (the same class as
-- messages). They are kept only on a thread that holds no waived run; the RPC skips them there.
-- A goal never exists on a zero-retention or waived thread (0091), so neither do the reasons.
-- Erasure: rows go with their goal (0092's ON DELETE CASCADE), which goes with its thread and org.
--
-- Additive only: four nullable columns with no default, so no table rewrite. The judge's tokens
-- are already counted by `judge_tokens` (0092).

ALTER TABLE thread_goal_turns
    ADD COLUMN IF NOT EXISTS verdict TEXT NULL
        CHECK (verdict IN ('met', 'not_met', 'blocked', 'unsure')),
    ADD COLUMN IF NOT EXISTS verdict_reasons JSONB NULL
        CHECK (verdict_reasons IS NULL OR jsonb_typeof(verdict_reasons) = 'array'),
    ADD COLUMN IF NOT EXISTS evidence_ids JSONB NULL
        CHECK (evidence_ids IS NULL OR jsonb_typeof(evidence_ids) = 'array'),
    ADD COLUMN IF NOT EXISTS judge_model TEXT NULL;
