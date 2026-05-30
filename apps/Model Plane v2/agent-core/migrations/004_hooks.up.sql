-- Phase D: Hook system — tool call interception middleware
-- Implements CC PreToolUse / PostToolUse / Stop hook patterns

CREATE TABLE IF NOT EXISTS hook_configs (
    id          TEXT PRIMARY KEY,
    org_id      TEXT NOT NULL,
    tool_name_pattern TEXT NOT NULL,       -- glob pattern: "*", "bash:*", "mcp:server:*"
    hook_type   TEXT NOT NULL CHECK (hook_type IN ('pre_tool_use', 'post_tool_use', 'stop')),
    action      TEXT NOT NULL CHECK (action IN ('approve', 'block', 'modify')),
    reason      TEXT NOT NULL DEFAULT '',
    modify_input  JSONB,
    modify_output JSONB,
    priority    INTEGER NOT NULL DEFAULT 0 CHECK (priority >= 0 AND priority <= 1000),
    enabled     BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_hook_configs_org ON hook_configs(org_id);
CREATE INDEX IF NOT EXISTS idx_hook_configs_org_type ON hook_configs(org_id, hook_type) WHERE enabled = true;
