-- §16.3.3 BM25 tsvector: precompute the to_tsvector('english', text) so the
-- sparse-retrieval query plans against a stored GIN index instead of
-- re-tokenizing every row at query time. 30-50% latency win on sparse path.
--
-- Generated column keeps the index in sync without app code changes.

ALTER TABLE knowledge_units
    ADD COLUMN IF NOT EXISTS content_tsv tsvector
    GENERATED ALWAYS AS (to_tsvector('english', text)) STORED;

-- Replace the inline-expression index with one over the generated column.
-- The old idx_ku_text_fts (init.sql:71) stays — Postgres will just pick
-- whichever index has lower cost; we drop it next migration once we've
-- verified production traffic uses content_tsv.
CREATE INDEX IF NOT EXISTS idx_ku_content_tsv_gin
    ON knowledge_units USING GIN (content_tsv);

COMMENT ON COLUMN knowledge_units.content_tsv IS
    'Generated tsvector of `text` for BM25 / FTS. Indexed by idx_ku_content_tsv_gin.';
