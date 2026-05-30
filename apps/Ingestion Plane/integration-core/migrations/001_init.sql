CREATE TABLE IF NOT EXISTS integration_connection_mappings (
    id TEXT PRIMARY KEY,
    organization_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    user_email TEXT,
    provider_key TEXT NOT NULL,
    provider_label TEXT NOT NULL,
    nango_connection_id TEXT NOT NULL UNIQUE,
    nango_integration_id TEXT NOT NULL,
    status TEXT NOT NULL,
    last_sync_status TEXT,
    last_sync_summary JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS integration_connection_mappings_org_workspace_idx
    ON integration_connection_mappings (organization_id, workspace_id);

CREATE INDEX IF NOT EXISTS integration_connection_mappings_user_idx
    ON integration_connection_mappings (user_id);

CREATE INDEX IF NOT EXISTS integration_connection_mappings_provider_idx
    ON integration_connection_mappings (provider_key);

CREATE TABLE IF NOT EXISTS integration_webhook_events (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    event_type TEXT NOT NULL,
    operation TEXT,
    nango_connection_id TEXT,
    payload JSONB NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);