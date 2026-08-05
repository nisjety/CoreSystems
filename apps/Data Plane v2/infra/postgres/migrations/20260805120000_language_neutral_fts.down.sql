-- Reverse of 20260805120000_language_neutral_fts.sql — restores the `english`
-- text-search configuration on every FTS surface.
--
-- WARNING: rolling this back reintroduces English stemming over Norwegian
-- content. It must be paired with reverting the query-side change in
-- retrieval-engine-rs (search/sparse.rs, graph.rs, wiki.rs,
-- contradictions.rs), or the `english` index will be queried with `simple`
-- and return ZERO rows.

ALTER TABLE knowledge_units DROP COLUMN IF EXISTS content_tsv;
ALTER TABLE knowledge_units
    ADD COLUMN content_tsv tsvector
    GENERATED ALWAYS AS (to_tsvector('english', text)) STORED;
CREATE INDEX IF NOT EXISTS idx_ku_content_tsv_gin
    ON knowledge_units USING GIN (content_tsv);

DROP INDEX IF EXISTS idx_ku_text_fts;
CREATE INDEX idx_ku_text_fts
    ON knowledge_units USING GIN (to_tsvector('english', text));

DROP INDEX IF EXISTS idx_documents_content_fts;
CREATE INDEX idx_documents_content_fts
    ON documents USING GIN (to_tsvector('english', content));

DROP INDEX IF EXISTS idx_ge_text;
CREATE INDEX idx_ge_text
    ON graph_entities USING GIN (to_tsvector('english', entity_text));

DROP INDEX IF EXISTS idx_gc_text;
CREATE INDEX idx_gc_text
    ON graph_claims USING GIN (to_tsvector('english', claim_text));
