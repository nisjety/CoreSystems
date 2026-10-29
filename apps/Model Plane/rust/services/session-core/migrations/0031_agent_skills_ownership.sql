-- 0031_agent_skills_ownership.sql
-- SKILL-1 (apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md): extends
-- agent_skills with the same scope + owner + explicit-share shape
-- capability-core's mcp_servers already has, instead of the org-wide,
-- admin-only visibility agent_skills has today.
--
-- Existing rows default to scope='org' with no owner/grantees, which is
-- exactly today's de facto behavior (everyone in the org sees every skill) —
-- additive and non-breaking.

ALTER TABLE agent_skills
    ADD COLUMN IF NOT EXISTS scope TEXT NOT NULL DEFAULT 'org',
    ADD COLUMN IF NOT EXISTS owner_user_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS shared_with JSONB NOT NULL DEFAULT '[]'::jsonb;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'agent_skills_scope_chk'
    ) THEN
        ALTER TABLE agent_skills
            ADD CONSTRAINT agent_skills_scope_chk
            CHECK (scope IN ('org', 'user'));
    END IF;
END$$;
