-- Phase 3 (freshness seam): record WHEN a knowledge unit became queryable
-- (vectors upserted into Qdrant). embedding-engine sets embedded_at = NOW()
-- in mark_units_done alongside embedding_status = 'done', so consumers can
-- distinguish "ingested/chunked" from "embedded/retrievable" and surface an
-- honest Indexing→Ready signal. The migrator wraps each file in a transaction;
-- do NOT declare BEGIN/COMMIT here.
ALTER TABLE knowledge_units ADD COLUMN IF NOT EXISTS embedded_at TIMESTAMPTZ;
CREATE INDEX IF NOT EXISTS idx_ku_embedded_at ON knowledge_units (embedded_at);
