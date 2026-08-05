-- P0-2 — Language-correct full-text search.
--
-- DEFECT: every FTS surface was pinned to the `english` text-search
-- configuration, including the STORED generated column
-- `knowledge_units.content_tsv`. Norwegian content was therefore stemmed and
-- stopword-filtered by English rules — silently, with no error — on the arm
-- that serves as the primary sparse backend's fallback.
--
-- WHY `simple` AND NOT `norwegian`: measured against the live corpus on
-- 2026-08-05, the 221 embedded chunks are ~60% English / ~38% Norwegian, and
-- the two languages MIX INSIDE A SINGLE CHUNK (e.g. an English heading
-- "Pathogen Management" over a Norwegian body "Tjenester Erfaringen viser at
-- manglende oppfyllelse..."). No single stemmer is correct for that corpus,
-- and a per-document language column cannot help when the mixing is
-- intra-chunk. `simple` performs no stemming and no stopword removal, so it
-- is wrong in neither direction, and it matches Quickwit's `tokenizer:
-- default` — which matters because Quickwit is the live primary sparse
-- backend and Postgres is its fallback, so the two arms now agree on
-- tokenisation instead of disagreeing.
--
-- Trade-off accepted: inflected forms no longer unify in either language
-- ("hund" will not match "hunder"). That is a smaller and more predictable
-- loss than incorrect cross-language stemming, and it is revisitable once
-- `eval_golden_judgments` exists to measure the difference (plan P0.5).
--
-- ROLLOUT NOTE: index-side and query-side configuration must change together.
-- A `simple` index queried with `plainto_tsquery('english', …)` returns ZERO
-- rows — a worse failure than bad stemming. The matching query-side change
-- ships in the same commit (retrieval-engine-rs: search/sparse.rs, graph.rs,
-- wiki.rs, contradictions.rs). At production scale these index rebuilds
-- should use CREATE INDEX CONCURRENTLY outside a transaction; at this corpus
-- size (19 MB, 223 rows) the plain in-transaction form is millisecond-scale
-- and keeps the migration atomic.

-- knowledge_units.content_tsv — the generated column behind the sparse arm.
-- Dropping the column also drops idx_ku_content_tsv_gin, so it is recreated.
ALTER TABLE knowledge_units DROP COLUMN IF EXISTS content_tsv;
ALTER TABLE knowledge_units
    ADD COLUMN content_tsv tsvector
    GENERATED ALWAYS AS (to_tsvector('simple', text)) STORED;
CREATE INDEX IF NOT EXISTS idx_ku_content_tsv_gin
    ON knowledge_units USING GIN (content_tsv);

-- Expression index on the same text. Retained (rather than dropped as
-- redundant) so this migration stays reversible and scoped to the language
-- change only.
DROP INDEX IF EXISTS idx_ku_text_fts;
CREATE INDEX idx_ku_text_fts
    ON knowledge_units USING GIN (to_tsvector('simple', text));

DROP INDEX IF EXISTS idx_documents_content_fts;
CREATE INDEX idx_documents_content_fts
    ON documents USING GIN (to_tsvector('simple', content));

DROP INDEX IF EXISTS idx_ge_text;
CREATE INDEX idx_ge_text
    ON graph_entities USING GIN (to_tsvector('simple', entity_text));

DROP INDEX IF EXISTS idx_gc_text;
CREATE INDEX idx_gc_text
    ON graph_claims USING GIN (to_tsvector('simple', claim_text));
