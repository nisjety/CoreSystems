-- Reverses 0134_skill_revisions_zdr_content_free.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Roll session-core back first: the build that writes the ZDR posture needs the column. The rollback
-- refuses while any content-free revision exists: 0064's trigger would keep such a row as a snapshot with
-- no text, and a restore could not tell it from a content-bearing one. Delete those rows first (they hold
-- ids, actions and times only), or keep the build that writes them.
--
-- Safe to run twice.

BEGIN;

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_name = 'skill_revisions' AND column_name = 'content_free'
    ) THEN
        IF EXISTS (SELECT 1 FROM skill_revisions WHERE content_free) THEN
            RAISE EXCEPTION 'skill_revisions holds content-free revisions; 0134 cannot be reverted while they exist';
        END IF;
        ALTER TABLE skill_revisions DROP COLUMN content_free;
    END IF;
END
$$;

-- 0064's trigger function, as it stood before this file.
CREATE OR REPLACE FUNCTION agent_skills_revision_fn() RETURNS TRIGGER AS $$
DECLARE
    before_snapshot JSONB;
    after_snapshot  JSONB;
    row_ref         agent_skills;
    kind            TEXT;
    override        TEXT;
    derived         TEXT;
    next_revision   BIGINT;
BEGIN
    IF TG_OP = 'INSERT' THEN
        row_ref := NEW;
        after_snapshot := agent_skill_revision_snapshot(NEW);
    ELSIF TG_OP = 'DELETE' THEN
        row_ref := OLD;
        before_snapshot := agent_skill_revision_snapshot(OLD);
    ELSE
        row_ref := NEW;
        before_snapshot := agent_skill_revision_snapshot(OLD);
        after_snapshot := agent_skill_revision_snapshot(NEW);
        -- A touch that changes nothing a person can see (updated_at alone) is not a
        -- change to record.
        IF before_snapshot = after_snapshot THEN
            RETURN NULL;
        END IF;
    END IF;

    kind := nullif(current_setting('mp.actor_kind', true), '');
    IF kind IS NULL OR kind NOT IN ('user', 'service', 'system') THEN
        kind := 'unattributed';
    END IF;

    override := nullif(current_setting('mp.skill_action', true), '');
    IF override IS NOT NULL AND override NOT IN ('accepted', 'rejected', 'restored', 'promoted') THEN
        override := NULL;
    END IF;

    IF TG_OP = 'DELETE' THEN
        derived := 'deleted';
    ELSIF override IS NOT NULL THEN
        derived := override;
    ELSIF TG_OP = 'INSERT' THEN
        derived := CASE WHEN NEW.review_state = 'proposed' THEN 'proposed' ELSE 'created' END;
    ELSIF OLD.review_state IS DISTINCT FROM NEW.review_state THEN
        derived := CASE NEW.review_state
            WHEN 'accepted' THEN 'accepted'
            WHEN 'rejected' THEN 'rejected'
            ELSE 'edited'
        END;
    ELSIF (before_snapshot - 'enabled') = (after_snapshot - 'enabled') THEN
        derived := CASE WHEN NEW.enabled THEN 'enabled' ELSE 'disabled' END;
    ELSIF NEW.origin = 'background_review' AND kind = 'service' THEN
        derived := 'updated';
    ELSE
        derived := 'edited';
    END IF;

    SELECT COALESCE(MAX(revision), 0) + 1 INTO next_revision
    FROM skill_revisions WHERE skill_id = row_ref.id;

    INSERT INTO skill_revisions
        (id, org_id, skill_id, owner_user_id, revision, action, actor_kind, actor_id,
         run_id, thread_id, reason, before, after)
    VALUES
        ('skr_' || replace(gen_random_uuid()::text, '-', ''), row_ref.org_id, row_ref.id,
         row_ref.owner_user_id, next_revision, derived, kind,
         left(COALESCE(current_setting('mp.actor_id', true), ''), 128),
         left(COALESCE(current_setting('mp.run_id', true), ''), 128),
         left(COALESCE(current_setting('mp.thread_id', true), ''), 128),
         left(COALESCE(current_setting('mp.reason', true), ''), 300),
         before_snapshot, after_snapshot);

    DELETE FROM skill_revisions
    WHERE skill_id = row_ref.id AND revision <= next_revision - 50;

    RETURN NULL;
END
$$ LANGUAGE plpgsql;

DROP FUNCTION IF EXISTS agent_skill_revision_metadata(agent_skills);

COMMIT;
