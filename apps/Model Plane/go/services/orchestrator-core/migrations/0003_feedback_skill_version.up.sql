-- Rate and quarantine skills by VERSION (EP-11 S3 part b).
--
-- model-gateway records a pinned skill as `id@N`. Until now the version was
-- stripped on ingest, so a rating of v3 and a rating of v4 pooled into one bucket
-- and a bad v3 could quarantine a fixed v4. This migration lets a row remember
-- which version it scored.
--
-- skill_version = 0 means "no version known": every row recorded before this
-- migration, every unpinned rating, and every row written while
-- FEEDBACK_SKILL_VERSIONS is off. Existing rows therefore all default to 0 and
-- the new key is a strict superset of the old one, so no row can collide during
-- the swap.
--
-- The key is repointed from the 0002 shape
-- (org_id, run_id, user_id, skill_id, source, signal_kind) so that one human can
-- rate two versions of a skill on the same run without the second upsert
-- erasing the first.

ALTER TABLE feedback_ratings
    ADD COLUMN IF NOT EXISTS skill_version integer NOT NULL DEFAULT 0
        CHECK (skill_version >= 0);

-- Both swaps below are guarded because the container entrypoint replays every
-- *.up.sql on each start: they run once, then become no-ops.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conrelid = 'feedback_ratings'::regclass
          AND contype = 'p'
          AND pg_get_constraintdef(oid) LIKE '%skill_version%'
    ) THEN
        ALTER TABLE feedback_ratings
            DROP CONSTRAINT IF EXISTS feedback_ratings_pkey;
        ALTER TABLE feedback_ratings
            ADD CONSTRAINT feedback_ratings_pkey
            PRIMARY KEY (org_id, run_id, user_id, skill_id, skill_version, source, signal_kind);
    END IF;
END
$$;

-- Promotion / demotion sweeps aggregate per skill and now per version, so carry
-- the version in the index. Recreated under the same name because
-- CREATE INDEX IF NOT EXISTS would otherwise keep the old column list.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_indexes
        WHERE tablename = 'feedback_ratings'
          AND indexname = 'feedback_ratings_org_skill_idx'
          AND indexdef LIKE '%skill_version%'
    ) THEN
        DROP INDEX IF EXISTS feedback_ratings_org_skill_idx;
        CREATE INDEX feedback_ratings_org_skill_idx
            ON feedback_ratings (org_id, skill_id, skill_version, from_scope, to_scope)
            WHERE skill_id <> '';
    END IF;
END
$$;
