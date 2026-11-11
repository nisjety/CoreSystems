-- Reverses 0117_skill_revision_url_patterns.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Puts back 0110's revision snapshot, without url_patterns. Revisions already written keep their
-- url_patterns key; the restore reads it when present and keeps the row's patterns when it is absent, so
-- both shapes stay restorable. Run this BEFORE reverting 0116: this snapshot names the column 0116 drops.
--
-- ORDER:
--   1. run this file;
--   2. delete the 0117 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (CREATE OR REPLACE).

BEGIN;
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
        'settings', s.settings,
        'has_draft', s.draft IS NOT NULL
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
            'kind', s.kind,
            'has_draft', s.draft IS NOT NULL
        );
    END IF;
    RETURN snapshot;
END
$$ LANGUAGE plpgsql IMMUTABLE;
COMMIT;
