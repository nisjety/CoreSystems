-- D19 (P4): bookkeeping for the failed-embedding reconciler.
--
-- Nothing re-drove `embedding_status = 'failed'` — data-quality only counted
-- those rows, data-orchestrator's stuck query filtered on `= 'pending'` so
-- they fell outside it entirely, and an idempotent re-POST of the document is
-- a no-op (`documentContentUnchanged`). Recovery meant a hand-written
-- `documents_outbox` re-enqueue.
--
-- `index-engine-rs::reconcile` now re-emits a signed
-- `dataplane.knowledge.units.created` for stranded units. These two columns
-- are what keep that bounded, so a permanently-poisoned unit cannot spin
-- forever:
--
--   embedding_retry_count  how many times the reconciler has re-driven this
--                          unit. Compared against a hard ceiling, and also
--                          the exponent of the backoff.
--   embedding_retry_at     when it last did so. The cooldown is measured from
--                          here, falling back to `updated_at` for a unit that
--                          failed before this migration existed.
--
-- Deliberately NOT reusing `metadata` JSONB: index-engine rewrites that column
-- wholesale when a chunk is rebuilt, which would silently reset the retry
-- budget and turn a bounded loop into an unbounded one.
ALTER TABLE knowledge_units
    ADD COLUMN IF NOT EXISTS embedding_retry_count INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS embedding_retry_at    TIMESTAMPTZ;

-- The reconciler's claim query is `WHERE embedding_status = 'failed' AND
-- embedding_retry_count < $ceiling ORDER BY COALESCE(embedding_retry_at,
-- updated_at)`. A partial index keeps that a cheap poll on a table that is
-- overwhelmingly `'done'`.
CREATE INDEX IF NOT EXISTS idx_ku_embedding_retry
    ON knowledge_units (embedding_retry_count, embedding_retry_at)
    WHERE embedding_status = 'failed';
