DROP INDEX IF EXISTS idx_ku_content_tsv_gin;
ALTER TABLE knowledge_units DROP COLUMN IF EXISTS content_tsv;
