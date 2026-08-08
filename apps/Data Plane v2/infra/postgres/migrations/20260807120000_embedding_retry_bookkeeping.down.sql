DROP INDEX IF EXISTS idx_ku_embedding_retry;

ALTER TABLE knowledge_units
    DROP COLUMN IF EXISTS embedding_retry_count,
    DROP COLUMN IF EXISTS embedding_retry_at;
