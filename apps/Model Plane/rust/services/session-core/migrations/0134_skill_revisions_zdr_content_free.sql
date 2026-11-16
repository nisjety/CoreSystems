-- 0134: a revision made under Zero Data Retention keeps no skill text.
--
-- Owner decision D20 lets a zero-retention principal remove a skill (DELETE skills/{id}), discard its
-- draft (DELETE skills/{id}/draft) and uninstall a plugin, whose skills go with it. Each of those writes
-- agent_skills, and 0064's trigger snapshotted the row into skill_revisions with its text, so a removal
-- would have stored the text it removed. Under ZDR the trigger still records the change (ids, the actor,
-- the action, the time), but its before and after hold metadata only, and the reason is blank.
--
-- The writer names the posture in the transaction, as it names the actor: mp.zdr_posture is 'true' under
-- ZDR. capability-core sets it on every skill write from the signed principal (skillActorSQL). Any other
-- value, unset included, keeps the 0064 snapshot, so session-core's writers and the erasure in gdpr.rs are
-- unchanged. session-core's undo and decide refuse a ZDR credential (auth.rs authorize_operation).
--
-- A content-free snapshot carries "truncated": true beside "content_free": true. The undo in
-- undo_learning_entry (grpc.rs) and the learning ledger's undoable flag already refuse a truncated
-- snapshot, so a content-free revision cannot be restored and reads as not undoable, with no reader
-- change. content_free marks the row for any reader that checks the column.
--
-- Additive: the column defaults to false, so every existing row stays content-bearing. The non-ZDR
-- snapshot, the 64 KB marker and the 50-revision window are unchanged. Down:
-- revert/0134_skill_revisions_zdr_content_free.down.sql (it refuses while content-free rows exist).

ALTER TABLE skill_revisions
    ADD COLUMN IF NOT EXISTS content_free BOOLEAN NOT NULL DEFAULT false;

-- What a content-free revision keeps: ids, the scope, the owner, the enabled flag, the origin and the
-- review state (CHECK-constrained words), and whether a draft is open. No name, description, content,
-- keywords, patterns, settings, proposal text or reason.
CREATE OR REPLACE FUNCTION agent_skill_revision_metadata(s agent_skills) RETURNS JSONB AS $$
BEGIN
    RETURN jsonb_build_object(
        'truncated', true,
        'content_free', true,
        'id', s.id,
        'org_id', s.org_id,
        'owner_user_id', s.owner_user_id,
        'scope', s.scope,
        'enabled', s.enabled,
        'origin', s.origin,
        'review_state', s.review_state,
        'has_draft', s.draft IS NOT NULL
    );
END
$$ LANGUAGE plpgsql IMMUTABLE;

CREATE OR REPLACE FUNCTION agent_skills_revision_fn() RETURNS TRIGGER AS $$
DECLARE
    before_snapshot JSONB;
    after_snapshot  JSONB;
    row_ref         agent_skills;
    kind            TEXT;
    override        TEXT;
    derived         TEXT;
    next_revision   BIGINT;
    zdr             BOOLEAN;
    kept_before     JSONB;
    kept_after      JSONB;
    kept_reason     TEXT;
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

    -- What the row keeps. The comparisons above used the full snapshots, in memory only. Under ZDR the
    -- row keeps each side's metadata and no reason text; otherwise it keeps the snapshots as 0064 wrote them.
    zdr := COALESCE(current_setting('mp.zdr_posture', true), '') = 'true';
    IF zdr THEN
        IF TG_OP <> 'INSERT' THEN
            kept_before := agent_skill_revision_metadata(OLD);
        END IF;
        IF TG_OP <> 'DELETE' THEN
            kept_after := agent_skill_revision_metadata(NEW);
        END IF;
        kept_reason := '';
    ELSE
        kept_before := before_snapshot;
        kept_after := after_snapshot;
        kept_reason := left(COALESCE(current_setting('mp.reason', true), ''), 300);
    END IF;

    SELECT COALESCE(MAX(revision), 0) + 1 INTO next_revision
    FROM skill_revisions WHERE skill_id = row_ref.id;

    INSERT INTO skill_revisions
        (id, org_id, skill_id, owner_user_id, revision, action, actor_kind, actor_id,
         run_id, thread_id, reason, before, after, content_free)
    VALUES
        ('skr_' || replace(gen_random_uuid()::text, '-', ''), row_ref.org_id, row_ref.id,
         row_ref.owner_user_id, next_revision, derived, kind,
         left(COALESCE(current_setting('mp.actor_id', true), ''), 128),
         left(COALESCE(current_setting('mp.run_id', true), ''), 128),
         left(COALESCE(current_setting('mp.thread_id', true), ''), 128),
         kept_reason, kept_before, kept_after, zdr);

    DELETE FROM skill_revisions
    WHERE skill_id = row_ref.id AND revision <= next_revision - 50;

    RETURN NULL;
END
$$ LANGUAGE plpgsql;
