-- 0116: url_patterns on agent_skills (Quarry v2 E4.3, skills that apply on matching browser pages).
--
-- A skill may carry URL patterns (host/path globs such as "docs.example.com/api/*"). The matcher
-- that reads them is a later, switched change (Q4-E4.3-043c2); this file is the storage half only:
-- the column, its shape check and the version body.
--
--   url_patterns  JSONB array of lowercase pattern strings, '[]' for every existing row and for
--                 every skill that does not use them. The writer (session-core UpsertAgentSkill)
--                 validates the list (at most 20 patterns, each 1-200 chars, no scheme, no
--                 whitespace, no '?' or '#'); the CHECK below only pins that the column is an array.
--
-- Digests: agent_skill_version_body is 0109's object plus ONE conditional key. A skill with no
-- url_patterns gets exactly the same JSON as before, so the digest and the version of every
-- existing row stay byte-identical (no version is cut by this migration). Only a skill that uses
-- patterns carries them in its version, so a pinned version (EP-11) answers "which URLs was this
-- skill for" from the version body and not from the base row, which a pin never reads. jsonb
-- stores keys in its own canonical order, so appending with || leaves the text of the other keys as it was.
-- The session-core test digest_field_list_is_pinned pins the unconditional list; the conditional
-- key is pinned beside it.
--
-- The revision snapshot (0064, 0109, 0110) is NOT touched: a url_patterns edit changes the version
-- body, which cuts a version for an accepted skill, and the revision log keeps its current shape.
--
-- Content: URL patterns are org configuration, like the skill text, and are erased with the skill,
-- with the organization and by owner (gdpr.rs already deletes every agent_skills row). Zero Data
-- Retention: ZDR credentials cannot write agent_skills at either writer, so no ZDR content reaches
-- this column.

ALTER TABLE agent_skills
    ADD COLUMN IF NOT EXISTS url_patterns JSONB NOT NULL DEFAULT '[]'::jsonb;

-- NOT VALID first, VALIDATE second, as 0109 does: a validated CHECK scans the table under ACCESS
-- EXCLUSIVE. The guard keeps a re-run harmless. It must exist before the function below calls
-- jsonb_array_length on the column.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_url_patterns_array_check'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_url_patterns_array_check
            CHECK (jsonb_typeof(url_patterns) = 'array') NOT VALID;
    END IF;
END
$$;
ALTER TABLE agent_skills VALIDATE CONSTRAINT agent_skills_url_patterns_array_check;

-- 0109's version body, plus url_patterns only when the skill has any.
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
    ) || CASE
        WHEN jsonb_array_length(s.url_patterns) > 0
            THEN jsonb_build_object('url_patterns', s.url_patterns)
        ELSE '{}'::jsonb
    END
$$ LANGUAGE sql STABLE;
