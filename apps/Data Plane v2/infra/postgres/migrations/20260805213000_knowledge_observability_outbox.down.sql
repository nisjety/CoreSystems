DROP INDEX IF EXISTS idx_documents_outbox_observability_pending;
ALTER TABLE documents_outbox
    DROP COLUMN IF EXISTS observability_published_at,
    DROP COLUMN IF EXISTS observability_published;
