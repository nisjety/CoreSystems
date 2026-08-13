-- Exact external-memory reconciliation records for a Space deletion. The
-- canonical thread/memory rows may be gone before an external bridge confirms
-- its side-effect, so only opaque IDs and minimum routing facts are retained.

CREATE TABLE IF NOT EXISTS space_deletion_semantic_memory_receipts (
    deletion_request_id TEXT NOT NULL,
    org_id TEXT NOT NULL,
    owner_principal_id TEXT NOT NULL,
    space_id TEXT NOT NULL,
    memory_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (deletion_request_id, memory_id),
    CONSTRAINT space_deletion_semantic_memory_status_chk
        CHECK (status IN ('pending', 'confirmed', 'unconfirmed'))
);

CREATE INDEX IF NOT EXISTS idx_space_deletion_semantic_memory_unconfirmed
    ON space_deletion_semantic_memory_receipts (org_id, status, updated_at)
    WHERE status <> 'confirmed';
