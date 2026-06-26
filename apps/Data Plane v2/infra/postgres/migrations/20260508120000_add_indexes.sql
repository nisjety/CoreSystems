-- Sample migration: add indexes that init.sql may have missed.
-- Idempotent: uses IF NOT EXISTS.

CREATE INDEX IF NOT EXISTS idx_documents_org_status
    ON documents (org_id, status)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_knowledge_units_doc_status
    ON knowledge_units (document_id, embedding_status);

CREATE INDEX IF NOT EXISTS idx_retrieval_runs_org_created
    ON retrieval_runs (org_id, created_at DESC);
