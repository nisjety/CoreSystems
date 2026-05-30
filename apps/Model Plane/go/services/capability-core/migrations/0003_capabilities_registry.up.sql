-- 0003_capabilities_registry.up.sql
--
-- Full durable capability registry.  Replaces the static in-memory seed
-- catalog and adds tables for skill packages, MCP servers, plugins,
-- routing policies, safety policies, and a unified audit log.
--
-- Convention: TEXT primary keys (caller-supplied ULID/UUID).
--             JSONB columns default to '{}' or '[]'.
--             soft-delete via deleted_at; hard delete is not performed.
--             Idempotent: CREATE TABLE IF NOT EXISTS / CREATE INDEX IF NOT EXISTS.

-- ---------------------------------------------------------------------------
-- capabilities  (master registry)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS capabilities (
    id                  TEXT PRIMARY KEY,
    org_id              TEXT NOT NULL DEFAULT 'global',
    kind                TEXT NOT NULL,                      -- tool | skill | plugin | mcp_server | model | ...
    name                TEXT NOT NULL,
    version             TEXT NOT NULL DEFAULT '1.0.0',
    description         TEXT NOT NULL DEFAULT '',
    risk_level          TEXT NOT NULL DEFAULT 'low',        -- low | medium | high
    scope               TEXT NOT NULL DEFAULT 'workspace',  -- run | thread | workspace | user | org | global
    lazy_load           BOOLEAN NOT NULL DEFAULT FALSE,
    enabled             BOOLEAN NOT NULL DEFAULT TRUE,
    idempotency_key     TEXT NOT NULL DEFAULT '',
    schema_input        JSONB NOT NULL DEFAULT '{}'::jsonb,
    schema_output       JSONB NOT NULL DEFAULT '{}'::jsonb,
    config_json         JSONB NOT NULL DEFAULT '{}'::jsonb,
    tags                TEXT[] NOT NULL DEFAULT '{}',
    enabled_for_scopes  TEXT[] NOT NULL DEFAULT '{}',
    -- registry scoring (populated by health worker)
    success_rate        DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    schema_fail_rate    DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    p95_latency_ms      DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    mean_cost_usd       DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    approval_rate       DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    incident_count      INT NOT NULL DEFAULT 0,
    operator_rating     DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    -- rollout / lifecycle
    rollout_state       TEXT NOT NULL DEFAULT 'stable',     -- canary | stable | quarantine | deprecated
    pinned_at           TIMESTAMPTZ,
    quarantined_at      TIMESTAMPTZ,
    deprecated_at       TIMESTAMPTZ,
    -- audit
    created_by          TEXT NOT NULL DEFAULT '',
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at          TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS capabilities_org_kind_name_uq
    ON capabilities (org_id, kind, name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS capabilities_kind_enabled_idx
    ON capabilities (kind, enabled)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS capabilities_org_idx
    ON capabilities (org_id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS capabilities_scope_idx
    ON capabilities (scope)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS capabilities_rollout_idx
    ON capabilities (rollout_state)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- capability_versions  (immutable history of every registered version)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS capability_versions (
    id              TEXT PRIMARY KEY,
    capability_id   TEXT NOT NULL REFERENCES capabilities(id),
    version         TEXT NOT NULL,
    schema_input    JSONB NOT NULL DEFAULT '{}'::jsonb,
    schema_output   JSONB NOT NULL DEFAULT '{}'::jsonb,
    config_json     JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS capability_versions_cap_version_uq
    ON capability_versions (capability_id, version);

-- ---------------------------------------------------------------------------
-- capability_scopes  (explicit scope grants per capability)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS capability_scopes (
    id              TEXT PRIMARY KEY,
    capability_id   TEXT NOT NULL REFERENCES capabilities(id),
    scope_kind      TEXT NOT NULL,   -- run | thread | workspace | user | org | global
    scope_value     TEXT NOT NULL DEFAULT '*',  -- '*' = all, otherwise specific ID
    granted_by      TEXT NOT NULL DEFAULT '',
    granted_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at      TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS capability_scopes_cap_idx
    ON capability_scopes (capability_id)
    WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS capability_scopes_scope_idx
    ON capability_scopes (scope_kind, scope_value)
    WHERE revoked_at IS NULL;

-- ---------------------------------------------------------------------------
-- capability_health  (runtime health and scoring snapshots)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS capability_health (
    id              TEXT PRIMARY KEY,
    capability_id   TEXT NOT NULL REFERENCES capabilities(id),
    success_rate    DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    schema_fail_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    p95_latency_ms  DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    mean_cost_usd   DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    approval_rate   DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    incident_count  INT NOT NULL DEFAULT 0,
    operator_rating DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    sample_window   TEXT NOT NULL DEFAULT '24h',
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS capability_health_cap_idx
    ON capability_health (capability_id, recorded_at DESC);

-- ---------------------------------------------------------------------------
-- skill_packages  (versioned, pinnable, testable skill bundles)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS skill_packages (
    id              TEXT PRIMARY KEY,
    capability_id   TEXT REFERENCES capabilities(id),
    org_id          TEXT NOT NULL DEFAULT 'global',
    name            TEXT NOT NULL,
    version         TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    trigger_keywords TEXT[] NOT NULL DEFAULT '{}',
    trigger_file_patterns TEXT[] NOT NULL DEFAULT '{}',
    tool_restrictions TEXT[] NOT NULL DEFAULT '{}',
    content         TEXT NOT NULL DEFAULT '',
    schema_json     JSONB NOT NULL DEFAULT '{}'::jsonb,
    examples        JSONB NOT NULL DEFAULT '[]'::jsonb,
    eval_score      DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    pinned_version  TEXT,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_by      TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS skill_packages_org_name_version_uq
    ON skill_packages (org_id, name, version)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS skill_packages_org_idx
    ON skill_packages (org_id, enabled)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- skill_resources  (files/assets within a skill package)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS skill_resources (
    id              TEXT PRIMARY KEY,
    skill_id        TEXT NOT NULL REFERENCES skill_packages(id),
    kind            TEXT NOT NULL DEFAULT 'file',  -- file | prompt | dataset | eval
    name            TEXT NOT NULL,
    content         TEXT NOT NULL DEFAULT '',
    mime_type       TEXT NOT NULL DEFAULT 'text/plain',
    size_bytes      BIGINT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS skill_resources_skill_idx ON skill_resources (skill_id);

-- ---------------------------------------------------------------------------
-- mcp_servers  (registered MCP server configurations)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS mcp_servers (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL DEFAULT 'global',
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    endpoint_url    TEXT NOT NULL,
    transport       TEXT NOT NULL DEFAULT 'http',  -- http | sse | stdio
    auth_kind       TEXT NOT NULL DEFAULT 'none',  -- none | api_key | oauth
    config_json     JSONB NOT NULL DEFAULT '{}'::jsonb,
    scope           TEXT NOT NULL DEFAULT 'workspace',
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    pinned_version  TEXT,
    rollout_state   TEXT NOT NULL DEFAULT 'stable',
    risk_level      TEXT NOT NULL DEFAULT 'medium',
    created_by      TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS mcp_servers_org_name_uq
    ON mcp_servers (org_id, name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS mcp_servers_org_idx
    ON mcp_servers (org_id, enabled)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- mcp_oauth_tokens  (OAuth credentials per org+server)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS mcp_oauth_tokens (
    id              TEXT PRIMARY KEY,
    server_id       TEXT NOT NULL REFERENCES mcp_servers(id),
    org_id          TEXT NOT NULL,
    access_token    TEXT NOT NULL DEFAULT '',
    refresh_token   TEXT NOT NULL DEFAULT '',
    token_type      TEXT NOT NULL DEFAULT 'Bearer',
    scope           TEXT NOT NULL DEFAULT '',
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS mcp_oauth_tokens_server_org_uq
    ON mcp_oauth_tokens (server_id, org_id);

-- ---------------------------------------------------------------------------
-- plugin_packages  (plugin manifests; code executed in sandbox)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS plugin_packages (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL DEFAULT 'global',
    name            TEXT NOT NULL,
    version         TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    manifest_json   JSONB NOT NULL DEFAULT '{}'::jsonb,
    risk_level      TEXT NOT NULL DEFAULT 'high',
    enabled         BOOLEAN NOT NULL DEFAULT FALSE,  -- disabled until pinned+tested
    pinned          BOOLEAN NOT NULL DEFAULT FALSE,
    rollout_state   TEXT NOT NULL DEFAULT 'canary',
    created_by      TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS plugin_packages_org_name_version_uq
    ON plugin_packages (org_id, name, version)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- routing_policies  (LLM routing policy configs)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS routing_policies (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL DEFAULT 'global',
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    strategy        TEXT NOT NULL DEFAULT 'round_robin',
    config_json     JSONB NOT NULL DEFAULT '{}'::jsonb,
    model_ids       TEXT[] NOT NULL DEFAULT '{}',
    priority        INT NOT NULL DEFAULT 0,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_by      TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS routing_policies_org_name_uq
    ON routing_policies (org_id, name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS routing_policies_org_idx
    ON routing_policies (org_id, enabled, priority DESC)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- safety_policies  (content safety and PII filter configs)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS safety_policies (
    id              TEXT PRIMARY KEY,
    org_id          TEXT NOT NULL DEFAULT 'global',
    name            TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    kind            TEXT NOT NULL DEFAULT 'pii_filter', -- pii_filter | content_safety | injection_defense
    config_json     JSONB NOT NULL DEFAULT '{}'::jsonb,
    applies_to      TEXT[] NOT NULL DEFAULT '{}',       -- ['input','output'] or specific kinds
    priority        INT NOT NULL DEFAULT 0,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_by      TEXT NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at      TIMESTAMPTZ
);

CREATE UNIQUE INDEX IF NOT EXISTS safety_policies_org_name_uq
    ON safety_policies (org_id, name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS safety_policies_org_idx
    ON safety_policies (org_id, enabled, priority DESC)
    WHERE deleted_at IS NULL;

-- ---------------------------------------------------------------------------
-- registry_audit_log  (append-only change log for all registry mutations)
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS registry_audit_log (
    id              TEXT PRIMARY KEY,
    entity_kind     TEXT NOT NULL,   -- capability | skill_package | mcp_server | plugin | ...
    entity_id       TEXT NOT NULL,
    action          TEXT NOT NULL,   -- created | updated | deleted | pinned | quarantined | promoted
    actor           TEXT NOT NULL DEFAULT '',
    org_id          TEXT NOT NULL DEFAULT '',
    diff_json       JSONB NOT NULL DEFAULT '{}'::jsonb,
    ts              TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS registry_audit_log_entity_idx
    ON registry_audit_log (entity_kind, entity_id, ts DESC);

CREATE INDEX IF NOT EXISTS registry_audit_log_actor_idx
    ON registry_audit_log (actor, ts DESC);

CREATE INDEX IF NOT EXISTS registry_audit_log_org_idx
    ON registry_audit_log (org_id, ts DESC);
