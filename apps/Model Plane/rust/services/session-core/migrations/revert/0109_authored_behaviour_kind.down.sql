-- Reverses 0109_authored_behaviour_kind.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate!
-- reads only the top level of migrations/. Run it by hand, against the database
-- only, inside one transaction.
--
-- DESTROYS every template and preset (rows of kind 'template' or 'preset', with
-- their versions through 0108's delete path, and their revision log keeps a
-- `deleted` entry) and the kind, settings and scope_id columns. Skills are
-- untouched. Export the templates and presets first if they matter:
--
--   SELECT * FROM agent_skills WHERE kind <> 'skill';
--
-- Recreating the old unique key (org_id, scope, owner_user_id, name) cannot fail
-- once only skills remain, unless a row has a non-empty scope_id (EP-11 S9 Space
-- rows); revert 0099's Space scope first if it is applied. The digest body goes
-- back to 0108's constants, which equal every remaining skill's kind and
-- settings, so no digest moves. The revision snapshot goes back to 0064's field
-- list; revisions written meanwhile keep their kind and settings keys, which the
-- older restore ignores.
--
-- ORDER, all three steps:
--   1. set AUTHORED_KINDS_ENABLED unset (or not `true`) for capability-core, and
--      roll session-core and capability-core back to builds that do not read the
--      columns (session-core's ListAgentSkills selects kind and settings, and its
--      upsert names the new conflict key);
--   2. run this file;
--   3. delete the 0109 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (IF EXISTS / IF NOT EXISTS).

BEGIN;
SELECT set_config('mp.actor_kind', 'system', true), set_config('mp.actor_id', 'revert:0109', true);
DELETE FROM agent_skills WHERE kind <> 'skill';
CREATE OR REPLACE FUNCTION agent_skill_version_body(s agent_skills) RETURNS JSONB AS $$
    SELECT jsonb_build_object(
        'name', s.name,
        'description', s.description,
        'content', s.content,
        'trigger_keywords', s.trigger_keywords,
        'trigger_file_patterns', s.trigger_file_patterns,
        'tool_restrictions', s.tool_restrictions,
        'kind', 'skill'::text,
        'settings', '{}'::jsonb,
        'bundle_sha256', s.bundle_sha256
    )
$$ LANGUAGE sql STABLE;
-- 0064's revision snapshot, without kind and settings.
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
CREATE UNIQUE INDEX IF NOT EXISTS agent_skills_scope_owner_name_uq
    ON agent_skills (org_id, scope, owner_user_id, name);
DROP INDEX IF EXISTS agent_skills_kind_scope_owner_name_uq;
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_settings_object_check;
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_kind_check;
ALTER TABLE agent_skills
    DROP COLUMN IF EXISTS scope_id,
    DROP COLUMN IF EXISTS settings,
    DROP COLUMN IF EXISTS kind;
COMMIT;
