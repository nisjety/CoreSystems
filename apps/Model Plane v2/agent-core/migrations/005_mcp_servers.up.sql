-- Phase E: MCP server integration — external tool providers per org
-- Supports stdio (subprocess + JSON-RPC) and HTTP transports

CREATE TABLE IF NOT EXISTS mcp_servers (
    id            TEXT PRIMARY KEY,
    org_id        TEXT NOT NULL,
    name          TEXT NOT NULL,
    transport     TEXT NOT NULL CHECK (transport IN ('stdio', 'http')),
    command       TEXT,                -- stdio: executable path
    args_json     JSONB,               -- stdio: command arguments array
    env_json      JSONB,               -- stdio: environment variables
    url           TEXT,                -- http: server URL
    headers_json  JSONB,               -- http: request headers
    enabled       BOOLEAN NOT NULL DEFAULT true,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (org_id, name)
);

CREATE INDEX IF NOT EXISTS idx_mcp_servers_org ON mcp_servers(org_id);
CREATE INDEX IF NOT EXISTS idx_mcp_servers_org_enabled ON mcp_servers(org_id) WHERE enabled = true;
