-- Phase H: Skills system — reusable skill definitions per org

CREATE TABLE IF NOT EXISTS agent_skills (
    id                      TEXT PRIMARY KEY,
    org_id                  TEXT NOT NULL,
    name                    TEXT NOT NULL,
    description             TEXT NOT NULL,
    content                 TEXT NOT NULL,            -- SKILL.md equivalent
    trigger_keywords        JSONB DEFAULT '[]',       -- ["testing", "tdd"]
    trigger_file_patterns   JSONB DEFAULT '[]',       -- ["**/*.test.ts"]
    tool_restrictions       JSONB DEFAULT '[]',       -- tool allow-list when active
    enabled                 BOOLEAN NOT NULL DEFAULT true,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agent_skills_org ON agent_skills(org_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_skills_unique_name ON agent_skills(org_id, name);
