-- Reverse 0002. Implicit rows are DELETED rather than folded into explicit ones:
-- collapsing them would merge behavioural inference into stated judgement, which
-- is exactly the conflation the forward migration exists to prevent, and the old
-- primary key cannot hold both for one turn anyway.
DELETE FROM feedback_ratings WHERE source = 'implicit';

DROP INDEX IF EXISTS feedback_ratings_org_skill_source_idx;

ALTER TABLE feedback_ratings DROP CONSTRAINT IF EXISTS feedback_ratings_pkey;
ALTER TABLE feedback_ratings
    ADD CONSTRAINT feedback_ratings_pkey
    PRIMARY KEY (org_id, run_id, user_id, skill_id);

ALTER TABLE feedback_ratings DROP COLUMN IF EXISTS signal_strength;
ALTER TABLE feedback_ratings DROP COLUMN IF EXISTS signal_kind;
ALTER TABLE feedback_ratings DROP COLUMN IF EXISTS source;
