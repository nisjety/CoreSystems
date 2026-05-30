-- Phase F: Context injection — persistent agent memory per org/session

CREATE TABLE IF NOT EXISTS agent_memory (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    session_id  TEXT,                   -- NULL = org-level; set = session-scoped
    key         TEXT NOT NULL,          -- e.g. "debugging.md", "patterns.md"
    content     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_agent_memory_org ON agent_memory(org_id);
CREATE INDEX IF NOT EXISTS idx_agent_memory_org_session ON agent_memory(org_id, session_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_memory_unique_key ON agent_memory(org_id, COALESCE(session_id, ''), key);
