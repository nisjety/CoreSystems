-- Reverses 0116_url_patterns_on_agent_skills.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- DESTROYS every skill's url_patterns (the column). The version body goes back to 0109's, which has no
-- url_patterns key: the digest of a skill that has patterns is computed over a body without them from
-- then on, so its NEXT write of any versioned field cuts a new version (the digest moves once), and the
-- version rows already written keep their url_patterns key, which nothing older reads. Skills without
-- patterns are untouched: their body is identical in both functions. Export the patterns first if they
-- matter:
--
--   SELECT id, org_id, name, url_patterns FROM agent_skills WHERE jsonb_array_length(url_patterns) > 0;
--
-- ORDER:
--   1. roll session-core back to a build that does not read or write the column (its upsert, list and
--      restore statements name url_patterns);
--   2. run this file;
--   3. delete the 0116 row from _sqlx_migrations so the next boot re-applies it.
--
-- Safe to run twice (IF EXISTS).

BEGIN;
SELECT set_config('mp.actor_kind', 'system', true), set_config('mp.actor_id', 'revert:0116', true);
-- 0109's version body, without url_patterns. Replaced BEFORE the column goes: a function that
-- names the column would otherwise block the DROP.
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
ALTER TABLE agent_skills DROP CONSTRAINT IF EXISTS agent_skills_url_patterns_array_check;
ALTER TABLE agent_skills DROP COLUMN IF EXISTS url_patterns;
COMMIT;
