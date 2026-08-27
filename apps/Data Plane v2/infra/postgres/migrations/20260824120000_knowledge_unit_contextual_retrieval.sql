-- Contextual Retrieval (Anthropic, 2024) — both halves.
--
-- The technique: ask an LLM for one or two sentences situating each chunk inside
-- its document, then let BOTH retrieval arms see those sentences. Contextual
-- EMBEDDINGS alone cut retrieval failures ~35%; adding contextual BM25 takes it
-- to ~49%, because the two arms fail on different queries — the dense arm misses
-- rare literal tokens, the lexical arm misses paraphrase.
--
-- ── What is stored ───────────────────────────────────────────────────────────
--
-- `chunk_context` holds the generated sentences ALONE, not the chunk with the
-- context prepended. Two reasons, and the second is the load-bearing one:
--
--   1. Storing the composed form would duplicate every contextualized chunk's
--      text in the same row. The composed form is a pure function of
--      (chunk_context, text) — see `compose_contextualized` — so there is
--      nothing to gain by materialising it.
--   2. It keeps the chunk's own words in exactly ONE searchable field. BM25 sums
--      across fields, so indexing a composed "context + chunk" string alongside
--      the chunk itself would score the chunk's own terms twice and quietly
--      re-weight every lexical result. Quickwit's `body`/`context_body` split
--      and this column's A/B tsvector weighting are the same structure, so the
--      primary sparse backend and its Postgres fallback agree.
--
-- WRITTEN BY: embedding-engine-rs (`batch::persist_chunk_context`), NOT
-- index-engine-rs. The chunker's module header claimed for a long time that
-- Contextual Retrieval was "wired behind flags" there; it never was, and the
-- chunker is the wrong home — `builder::process_document` runs its whole build
-- inside one transaction holding FOR UPDATE row locks, so per-chunk LLM calls
-- would hold those locks for minutes. The embedding engine already holds an
-- org-bound inference credential, runs async off the transaction critical path,
-- and is the layer that decides what text gets embedded.
--
-- Nullable, no backfill, and expected to stay NULL for most rows: the feature is
-- OFF by default (CONTEXTUAL_RETRIEVAL_ENABLED) because it spends one inference
-- call per chunk on every first-time embed. Restricted (ZDR) documents are never
-- contextualized at all, so their rows stay NULL by design, not by omission.
ALTER TABLE knowledge_units ADD COLUMN IF NOT EXISTS chunk_context TEXT;

-- Which prompt produced the context. Lets an audit tell a `ctx-v1` row from a
-- later revision's, and lets a future refresh pass find rows built by a
-- superseded prompt instead of re-generating every row blindly.
ALTER TABLE knowledge_units ADD COLUMN IF NOT EXISTS context_prompt_version TEXT;

COMMENT ON COLUMN knowledge_units.chunk_context IS
    'Contextual Retrieval: LLM-generated sentences situating this chunk in its document. NOT the text shown to callers - that is always `text`. Prepended to `text` at embed time and indexed at weight B in content_tsv.';

-- ── Contextual BM25: fold the context into the sparse arm ────────────────────
--
-- `content_tsv` is the STORED generated column behind the lexical arm. It is
-- dropped and recreated rather than altered because Postgres has no
-- ALTER ... GENERATED ALWAYS AS; this follows the identical drop/recreate in
-- 20260805120000_language_neutral_fts.sql, including recreating the GIN index
-- that the column drop takes with it.
--
-- WEIGHTS ARE THE POINT. The chunk's own words are weight A, the generated
-- context is weight B. `ts_rank_cd`'s default weight vector is
-- {D,C,B,A} = {0.1, 0.2, 0.4, 1.0}, so the context contributes at 0.4 of the
-- chunk's own terms — present enough to make an otherwise-anonymous chunk
-- findable by the entity or date it only implies, but never enough for a
-- model-generated sentence to outrank a real literal match. Concatenating
-- unweighted would have let generated text compete on equal footing with the
-- document's own, which is the failure mode that makes people distrust this
-- technique.
--
-- `coalesce(chunk_context, '')` is required, not defensive: `||` with a NULL
-- tsvector yields NULL, so without it every non-contextualized row — i.e. all
-- of them today — would have a NULL content_tsv and silently vanish from the
-- lexical arm entirely.
--
-- `to_tsvector('simple', ...)`: `simple` is deliberate and must not drift to a
-- stemmer here; see 20260805120000_language_neutral_fts.sql for why (mixed
-- English/Norwegian inside single chunks, and parity with Quickwit's `default`
-- tokenizer).
--
-- Today this rewrite is a semantic no-op: `chunk_context` is NULL on every
-- existing row, so the new expression produces byte-identical vectors to the
-- old one. Doing it now, while that is true, is the cheap moment — after
-- contextualization has run, the same migration would change live results.
ALTER TABLE knowledge_units DROP COLUMN IF EXISTS content_tsv;
ALTER TABLE knowledge_units
    ADD COLUMN content_tsv tsvector
    GENERATED ALWAYS AS (
        setweight(to_tsvector('simple', text), 'A')
        || setweight(to_tsvector('simple', coalesce(chunk_context, '')), 'B')
    ) STORED;
CREATE INDEX IF NOT EXISTS idx_ku_content_tsv_gin
    ON knowledge_units USING GIN (content_tsv);

COMMENT ON COLUMN knowledge_units.content_tsv IS
    'Generated tsvector for BM25/FTS: `text` at weight A, `chunk_context` at weight B (Contextual Retrieval). Indexed by idx_ku_content_tsv_gin.';
