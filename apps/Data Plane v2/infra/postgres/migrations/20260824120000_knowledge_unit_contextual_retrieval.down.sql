-- Restore `content_tsv` to the text-only expression BEFORE dropping the column
-- it references: a generated column cannot outlive its inputs, so dropping
-- chunk_context first would fail (or cascade the tsvector away).
ALTER TABLE knowledge_units DROP COLUMN IF EXISTS content_tsv;
ALTER TABLE knowledge_units
    ADD COLUMN content_tsv tsvector
    GENERATED ALWAYS AS (to_tsvector('simple', text)) STORED;
CREATE INDEX IF NOT EXISTS idx_ku_content_tsv_gin
    ON knowledge_units USING GIN (content_tsv);

COMMENT ON COLUMN knowledge_units.content_tsv IS
    'Generated tsvector of `text` for BM25 / FTS. Indexed by idx_ku_content_tsv_gin.';

ALTER TABLE knowledge_units DROP COLUMN IF EXISTS context_prompt_version;
ALTER TABLE knowledge_units DROP COLUMN IF EXISTS chunk_context;
