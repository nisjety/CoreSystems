CREATE TABLE IF NOT EXISTS billing_organization_tombstones (
    org_id TEXT PRIMARY KEY,
    reason TEXT NOT NULL DEFAULT 'organization_deleted',
    deleted_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS billing_organization_tombstones_deleted_at
    ON billing_organization_tombstones (deleted_at);
