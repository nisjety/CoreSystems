-- Reverses 0110_agent_skill_drafts.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction.
--
-- DESTROYS every unpublished draft (the draft, draft_updated_at and draft_by
-- columns). Live skill text, versions and revisions are untouched: a draft was
-- never live, so no turn and no pinned run read it. Export the drafts first if
-- they matter:
--
--   SELECT id, org_id, draft_by, draft_updated_at, draft FROM agent_skills
--   WHERE draft IS NOT NULL;
--
-- The revision snapshot goes back to 0109's field list; revisions written
-- meanwhile keep their has_draft key, which the restore ignores.
--
-- ORDER, all three steps:
--   1. set SKILL_DRAFT_PUBLISH unset (or not `true`) for capability-core, and
--      roll capability-core back to a build that does not read the columns;
--   2. run this file;
--   3. delete the 0110 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (IF EXISTS).

BEGIN;
-- 0109's revision snapshot, without has_draft. Replaced before the column goes,
-- so no trigger call can reach a function that names a dropped column.
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
        'proposal_reason', s.proposal_reason,
        'kind', s.kind,
        'settings', s.settings
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
            'review_state', s.review_state,
            'kind', s.kind
        );
    END IF;
    RETURN snapshot;
END
$$ LANGUAGE plpgsql IMMUTABLE;
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_draft_object_check;
ALTER TABLE agent_skills
    DROP COLUMN IF EXISTS draft_by,
    DROP COLUMN IF EXISTS draft_updated_at,
    DROP COLUMN IF EXISTS draft;
COMMIT;
