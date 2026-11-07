-- 0109: slash templates and presets as authored kinds of agent_skills (Wave 3 EP-11 S6).
--
-- Reserved as 0098 in round B; numbered 0109 so it applies after 0108, whose version body
-- function this file replaces (on a fresh database 0098 would run first and 0108 would put
-- the constants back).
--
-- EP-11 D1: one table for every authored behaviour, so one revision trigger, one version
-- trigger, one lint and one erasure statement cover all three kinds:
--   kind      'skill' (matched and injected into turns, every existing row),
--             'template' (plain text the composer inserts before the person sends) or
--             'preset' (visible composer/console defaults). Only 'skill' is ever matched or
--             injected: ListAgentSkills with no kinds lists skills only.
--   settings  a preset's closed settings object ('{}' for the other kinds), linted by the
--             authored-behaviour lint, never free text.
--   scope_id  the Space a scope = 'space' row belongs to (EP-11 S9); '' for every row today.
--
-- The unique key moves from (org_id, scope, owner_user_id, name) to
-- (org_id, kind, scope, scope_id, owner_user_id, name), so a template may share a skill's
-- name. It is weaker than the old key for every existing row (all are kind 'skill' with an
-- empty scope_id), so creating it cannot fail on existing data. CREATE UNIQUE INDEX without
-- CONCURRENTLY blocks writes to agent_skills briefly; the table holds per-org authored text
-- and is small.
--
-- contract: drop agent_skills_scope_owner_name_uq; agent_skills_kind_scope_owner_name_uq replaces it in this same migration and is implied by it for every existing row (an older session-core's ON CONFLICT (org_id, scope, owner_user_id, name) cannot run over this schema, so roll its image back only together with revert/0109_authored_behaviour_kind.down.sql)
--
-- Digests: agent_skill_version_body now reads the real kind and settings. 0108 used the
-- constants 'skill' and '{}', which are these columns' defaults, so the body of every
-- existing row is the same JSON and no digest or version moves (session-core test
-- existing_digests_unchanged_by_kind_migration).
--
-- Content: a template's text and a preset's settings are org configuration, like a skill,
-- and are erased with the skill, with the organization and by owner (gdpr.rs already deletes
-- every agent_skills row). Zero Data Retention: ZDR credentials cannot write agent_skills at
-- either writer.

ALTER TABLE agent_skills
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'skill',
    ADD COLUMN IF NOT EXISTS settings JSONB NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS scope_id TEXT NOT NULL DEFAULT '';

-- NOT VALID first, VALIDATE second, as 0058 does: a validated CHECK scans the table under
-- ACCESS EXCLUSIVE. The guard keeps a re-run harmless.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_kind_check'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_kind_check
            CHECK (kind IN ('skill', 'template', 'preset')) NOT VALID;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_settings_object_check'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_settings_object_check
            CHECK (jsonb_typeof(settings) = 'object') NOT VALID;
    END IF;
END
$$;
ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_kind_check;
ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_settings_object_check;

-- New key first, then the old one goes, so no moment exists without a unique key.
CREATE UNIQUE INDEX IF NOT EXISTS agent_skills_kind_scope_owner_name_uq
    ON agent_skills (org_id, kind, scope, scope_id, owner_user_id, name);

DROP INDEX IF EXISTS agent_skills_scope_owner_name_uq;

-- The versioned fields of an authored behaviour. Same field list and order as 0108 (pinned by
-- session-core tests digest_field_list_is_pinned and kind_migration_keeps_the_digest_field_list);
-- only the kind and settings values now come from the row.
CREATE OR REPLACE FUNCTION agent_skill_version_body(s agent_skills) RETURNS JSONB AS $$
    SELECT jsonb_build_object(
        'name', s.name,
        'description', s.description,
        'content', s.content,
        'trigger_keywords', s.trigger_keywords,
        'trigger_file_patterns', s.trigger_file_patterns,
        'tool_restrictions', s.tool_restrictions,
        'kind', s.kind,
        'settings', s.settings,
        'bundle_sha256', s.bundle_sha256
    )
$$ LANGUAGE sql STABLE;

-- The revision snapshot (0064) gains kind and settings, so an undo that re-creates a deleted
-- template or preset (session-core RESTORE_SKILL_SQL) brings it back as its own kind and never as a
-- skill that would be injected. Both are constant for a skill row, so no skill write records a
-- revision it did not record before; a preset's settings edit now reads as `edited`. Snapshots
-- written before this file lack both keys and are skills.
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
