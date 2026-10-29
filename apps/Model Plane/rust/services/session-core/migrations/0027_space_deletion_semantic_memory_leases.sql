-- Lease semantic-memory deletion reconciliation so multiple Session Core
-- replicas cannot concurrently retry the same external side effect. A lease
-- is intentionally short: after worker loss the exact receipt becomes
-- eligible again, while a stale worker cannot settle a newer claim.

ALTER TABLE space_deletion_semantic_memory_receipts
    ADD COLUMN IF NOT EXISTS lease_token TEXT,
    ADD COLUMN IF NOT EXISTS lease_expires_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_space_deletion_semantic_memory_due
    ON space_deletion_semantic_memory_receipts (updated_at)
    WHERE status <> 'confirmed' AND lease_expires_at IS NULL;
