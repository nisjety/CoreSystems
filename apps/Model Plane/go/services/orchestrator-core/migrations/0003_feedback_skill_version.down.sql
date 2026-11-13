-- Reverse 0003. The old key has no skill_version, so two rows that differ only by
-- version would collide. Keep the most recently updated one (ties broken toward
-- the higher version, so the choice is deterministic) and drop the rest: the
-- latest judgement is the one a re-rating would have left under the old key.
DELETE FROM feedback_ratings AS a
USING feedback_ratings AS b
WHERE a.org_id      = b.org_id
  AND a.run_id      = b.run_id
  AND a.user_id     = b.user_id
  AND a.skill_id    = b.skill_id
  AND a.source      = b.source
  AND a.signal_kind = b.signal_kind
  AND a.skill_version <> b.skill_version
  AND (a.updated_at < b.updated_at
       OR (a.updated_at = b.updated_at AND a.skill_version < b.skill_version));

DROP INDEX IF EXISTS feedback_ratings_org_skill_idx;
CREATE INDEX IF NOT EXISTS feedback_ratings_org_skill_idx
    ON feedback_ratings (org_id, skill_id, from_scope, to_scope)
    WHERE skill_id <> '';

ALTER TABLE feedback_ratings DROP CONSTRAINT IF EXISTS feedback_ratings_pkey;
ALTER TABLE feedback_ratings
    ADD CONSTRAINT feedback_ratings_pkey
    PRIMARY KEY (org_id, run_id, user_id, skill_id, source, signal_kind);

ALTER TABLE feedback_ratings DROP COLUMN IF EXISTS skill_version;
