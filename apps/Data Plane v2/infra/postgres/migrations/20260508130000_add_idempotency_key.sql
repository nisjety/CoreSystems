-- Idempotency key for document creation.
-- Allows clients to safely retry CreateDocument / BulkIngest without producing duplicates.
-- Scoped per (org_id, idempotency_key); NULL keys are allowed and skip the constraint.

ALTER TABLE documents ADD COLUMN IF NOT EXISTS idempotency_key TEXT;

-- Partial unique index: enforced only when idempotency_key is non-NULL.
CREATE UNIQUE INDEX IF NOT EXISTS uq_documents_idempotency
    ON documents (org_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL AND deleted_at IS NULL;
