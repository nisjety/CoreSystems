-- The revision log of agent_skills (build plan WS07 S8).
--
-- Every change to a skill row, from any writer, leaves one row here with the actor
-- that made it. There are two writers today (session-core's RPCs and capability-core's
-- registry SQL, both on the shared session_core database) and a third is always
-- possible, so the log is written by a trigger on the table rather than by each writer.
-- A writer names itself by setting transaction-local settings in the transaction that
-- changes the row:
--
--   mp.actor_kind   'user', 'service' or 'system'
--   mp.actor_id     the verified principal id
--   mp.run_id       the run a learned change came from
--   mp.thread_id    the thread of that run
--   mp.reason       why, at most 300 characters
--   mp.skill_action optional: 'accepted', 'rejected', 'restored' or 'promoted' when the
--                   change is one of those decisions; otherwise the trigger derives the
--                   action from what changed
--
-- A write that names no actor is still logged, as 'unattributed', so a new writer
-- cannot make a change that leaves no trace.
--
-- before and after are snapshots of the row without its timestamps, null on the side that
-- does not exist (an insert has no before, a delete no after). A snapshot over 64 KB is
-- stored as a marker (`truncated`) without the body; an entry that holds one cannot be
-- undone, and says so. The newest 50 revisions of each skill are kept.
--
-- owner_user_id is the skill's owner at the time ('' for an org skill): the owner's
-- ledger and the erasure of an owner read it without a join to a row that may be gone.
--
-- Content-bearing (skill text is in the snapshots), derived only from retained runs and
-- the owner's own edits, and erased with the organization (gdpr.rs).
CREATE TABLE IF NOT EXISTS skill_revisions (
    id             TEXT PRIMARY KEY,
    org_id         TEXT NOT NULL,
    skill_id       TEXT NOT NULL,
    owner_user_id  TEXT NOT NULL DEFAULT '',
    revision       BIGINT NOT NULL,
    action         TEXT NOT NULL
        CHECK (action IN ('created', 'proposed', 'updated', 'accepted', 'rejected', 'enabled',
                          'disabled', 'edited', 'deleted', 'restored', 'promoted')),
    actor_kind     TEXT NOT NULL
        CHECK (actor_kind IN ('user', 'service', 'system', 'unattributed')),
    actor_id       TEXT NOT NULL DEFAULT '',
    run_id         TEXT NOT NULL DEFAULT '',
    thread_id      TEXT NOT NULL DEFAULT '',
    reason         TEXT NOT NULL DEFAULT '',
    before         JSONB,
    after          JSONB,
    undone_by      TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    CONSTRAINT skill_revisions_skill_revision_uq UNIQUE (skill_id, revision)
);

CREATE INDEX IF NOT EXISTS skill_revisions_org_created_idx
    ON skill_revisions (org_id, created_at);

-- The owner's ledger reads their own revisions newest first.
CREATE INDEX IF NOT EXISTS skill_revisions_owner_created_idx
    ON skill_revisions (org_id, owner_user_id, created_at DESC, id DESC);

CREATE OR REPLACE FUNCTION agent_skill_revision_snapshot(s agent_skills) RETURNS JSONB AS $$
DECLARE
    snapshot JSONB;
BEGIN
    snapshot := jsonb_build_object(
        'id', s.id,
        'org_id', s.org_id,
        'name', s.name,
        'description', s.description,
        'content', s.content,
        'trigger_keywords', s.trigger_keywords,
        'trigger_file_patterns', s.trigger_file_patterns,
        'tool_restrictions', s.tool_restrictions,
        'enabled', s.enabled,
        'origin', s.origin,
        'scope', s.scope,
        'owner_user_id', s.owner_user_id,
        'shared_with', s.shared_with,
        'review_state', s.review_state,
        'source_run_id', s.source_run_id,
        'source_thread_id', s.source_thread_id,
        'proposal_reason', s.proposal_reason
    );
    IF pg_column_size(snapshot) > 65536 THEN
        RETURN jsonb_build_object(
            'truncated', true,
            'id', s.id,
            'org_id', s.org_id,
            'name', s.name,
            'enabled', s.enabled,
            'scope', s.scope,
            'owner_user_id', s.owner_user_id,
            'review_state', s.review_state
        );
    END IF;
    RETURN snapshot;
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

CREATE OR REPLACE TRIGGER agent_skills_revision_trg
    AFTER INSERT OR UPDATE OR DELETE ON agent_skills
    FOR EACH ROW EXECUTE FUNCTION agent_skills_revision_fn();
