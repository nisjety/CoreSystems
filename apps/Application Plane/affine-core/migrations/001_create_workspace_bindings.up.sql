CREATE TABLE IF NOT EXISTS affine_core_workspace_bindings (
    org_id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL UNIQUE,
    created_by_user_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_affine_core_workspace_bindings_workspace_id
    ON affine_core_workspace_bindings (workspace_id);
