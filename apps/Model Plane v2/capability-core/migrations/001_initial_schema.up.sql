-- capability-core schema

-- pg_trgm for fuzzy tool search
CREATE EXTENSION IF NOT EXISTS pg_trgm;

-- ── Tools ───────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS tools (
    id                   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name                 TEXT NOT NULL UNIQUE,
    version              TEXT NOT NULL DEFAULT '1.0.0',
    description          TEXT NOT NULL DEFAULT '',
    category             TEXT NOT NULL DEFAULT 'general',
    source               TEXT NOT NULL DEFAULT 'built_in',
    execution_target     TEXT NOT NULL DEFAULT 'llm_worker',

    input_schema         JSONB NOT NULL DEFAULT '{}',
    output_schema        JSONB NOT NULL DEFAULT '{}',

    should_defer         BOOLEAN NOT NULL DEFAULT false,
    always_load          BOOLEAN NOT NULL DEFAULT false,

    tags                 TEXT[] NOT NULL DEFAULT '{}',
    search_hint          TEXT NOT NULL DEFAULT '',

    avg_latency_ms       INT NOT NULL DEFAULT 100,
    max_result_size_chars INT NOT NULL DEFAULT 16000,
    read_only            BOOLEAN NOT NULL DEFAULT true,
    requires_approval    BOOLEAN NOT NULL DEFAULT false,
    timeout_ms           INT NOT NULL DEFAULT 30000,

    mcp_server_id        TEXT,
    resolved_tool_name   TEXT,
    plugin_id            TEXT,
    skill_id             TEXT,

    created_at           TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_tools_category ON tools (category);
CREATE INDEX IF NOT EXISTS idx_tools_source ON tools (source);
CREATE INDEX IF NOT EXISTS idx_tools_trgm ON tools USING gin ((name || ' ' || description || ' ' || search_hint) gin_trgm_ops);

-- ── MCP server configs ─────────────────────────────────────────

CREATE TABLE IF NOT EXISTS mcp_server_configs (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    server_id         TEXT NOT NULL UNIQUE,
    scope             TEXT NOT NULL DEFAULT 'session',
    transport         TEXT NOT NULL DEFAULT 'stdio',
    command           TEXT NOT NULL DEFAULT '',
    url               TEXT NOT NULL DEFAULT '',
    auth_json         JSONB NOT NULL DEFAULT '{}',
    auto_connect      BOOLEAN NOT NULL DEFAULT false,
    globally_enabled  BOOLEAN NOT NULL DEFAULT false,
    discovered_tools  TEXT[] NOT NULL DEFAULT '{}',
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── MCP session / org / user overrides ─────────────────────────

CREATE TABLE IF NOT EXISTS mcp_session_overrides (
    server_id    TEXT NOT NULL REFERENCES mcp_server_configs(server_id) ON DELETE CASCADE,
    session_id   TEXT,
    org_id       TEXT,
    user_id      TEXT,
    enabled      BOOLEAN NOT NULL DEFAULT true
);

-- Expression-based unique index (inline UNIQUE with COALESCE is not valid DDL)
CREATE UNIQUE INDEX IF NOT EXISTS uq_mcp_session_overrides
    ON mcp_session_overrides (
        server_id,
        COALESCE(session_id, ''),
        COALESCE(org_id, ''),
        COALESCE(user_id, '')
    );

-- ── Plugin catalog ─────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS plugin_catalog (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    plugin_id           TEXT NOT NULL UNIQUE,
    version             TEXT NOT NULL DEFAULT '1.0.0',
    author              TEXT NOT NULL DEFAULT '',
    description         TEXT NOT NULL DEFAULT '',
    tools               TEXT[] NOT NULL DEFAULT '{}',
    mcp_server_ids      TEXT[] NOT NULL DEFAULT '{}',
    enabled             BOOLEAN NOT NULL DEFAULT true,
    installed_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    installed_by_org_id TEXT NOT NULL DEFAULT ''
);

-- ── Routing policies ───────────────────────────────────────────

CREATE TABLE IF NOT EXISTS routing_policies (
    id                        UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    org_id                    TEXT NOT NULL UNIQUE,
    tier                      TEXT NOT NULL DEFAULT 'basic',
    latency_class             TEXT NOT NULL DEFAULT 'balanced',
    fallback_strategy         TEXT NOT NULL DEFAULT 'sequential',
    provider_priority         TEXT[] NOT NULL DEFAULT '{}',
    max_cost_per_request_nok  NUMERIC(10,4) NOT NULL DEFAULT 1.0,
    daily_budget_nok          NUMERIC(10,2) NOT NULL DEFAULT 25.0,
    monthly_budget_nok        NUMERIC(10,2) NOT NULL DEFAULT 500.0,
    feature_requirements      JSONB NOT NULL DEFAULT '{}',
    created_at                TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Model configs ──────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS model_configs (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    model_id                TEXT NOT NULL UNIQUE,
    provider                TEXT NOT NULL,
    api_endpoint            TEXT NOT NULL DEFAULT '',
    capabilities            JSONB NOT NULL DEFAULT '{}',
    cost_per_1k_input_nok   NUMERIC(10,6) NOT NULL DEFAULT 0,
    cost_per_1k_output_nok  NUMERIC(10,6) NOT NULL DEFAULT 0,
    context_window          INT NOT NULL DEFAULT 128000,
    enabled                 BOOLEAN NOT NULL DEFAULT true,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ── Memory adapter catalog ─────────────────────────────────────

CREATE TABLE IF NOT EXISTS memory_adapter_catalog (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    adapter_type    TEXT NOT NULL UNIQUE,
    description     TEXT NOT NULL DEFAULT '',
    config_schema   JSONB NOT NULL DEFAULT '{}',
    default_policy  JSONB NOT NULL DEFAULT '{}',
    enabled         BOOLEAN NOT NULL DEFAULT true
);

-- ── Audit log ──────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS capability_audit_log (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type  TEXT NOT NULL,
    entity_id   TEXT NOT NULL DEFAULT '',
    actor_id    TEXT NOT NULL DEFAULT '',
    org_id      TEXT NOT NULL DEFAULT '',
    payload     JSONB NOT NULL DEFAULT '{}',
    ts          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_audit_event ON capability_audit_log (event_type, ts DESC);
CREATE INDEX IF NOT EXISTS idx_audit_org ON capability_audit_log (org_id, ts DESC);
