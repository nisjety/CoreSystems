-- Reverse 20261003130000_context_provenance.sql.
--
-- Order matters: the view depends on context_text_hash() and on the columns, so
-- it goes first; the function that writes context_status goes before the column
-- it writes; constraints and the index go before the columns they reference.
--
-- This is the migration whose rollback DISCARDS data: the status, provenance and
-- hash columns, and any org_processing_policy rows, are dropped with their
-- columns and table. `chunk_context` and `context_prompt_version` (added by
-- 20260824120000) are NOT touched, so the contexts themselves survive; only what
-- this migration recorded about them is lost. Re-applying the forward migration
-- re-derives `legacy` / `pending` from the surviving contexts.

-- DROP COLUMN needs an exclusive lock on knowledge_units. Do not queue behind a
-- long builder transaction (and block every other reader and writer while doing
-- so): give up after 10 s and let the operator retry. See the forward migration.
SET LOCAL lock_timeout = '10s';

DROP VIEW IF EXISTS v_context_coverage;
DROP FUNCTION IF EXISTS context_provenance_backfill(TEXT);

-- Dropping the table takes its policy, trigger and grants with it.
DROP TABLE IF EXISTS public.org_processing_policy;

DROP INDEX IF EXISTS idx_ku_context_work;

ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_status_chk;
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_error_class_chk;
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_failure_class_chk;
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_present_chk;
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_attempts_chk;
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_embedded_context_hash_chk;
ALTER TABLE knowledge_units DROP CONSTRAINT IF EXISTS knowledge_units_context_input_hash_chk;

ALTER TABLE knowledge_units
    DROP COLUMN IF EXISTS context_status,
    DROP COLUMN IF EXISTS context_model,
    DROP COLUMN IF EXISTS context_provider,
    DROP COLUMN IF EXISTS context_residency,
    DROP COLUMN IF EXISTS context_input_hash,
    DROP COLUMN IF EXISTS embedded_context_hash,
    DROP COLUMN IF EXISTS context_generated_at,
    DROP COLUMN IF EXISTS context_attempts,
    DROP COLUMN IF EXISTS context_next_attempt_at,
    DROP COLUMN IF EXISTS context_error_class,
    DROP COLUMN IF EXISTS context_lease_until;

DROP FUNCTION IF EXISTS context_text_hash(TEXT);

-- Restore the two comments the forward migration rewrote, byte for byte: the
-- original `chunk_context` text from 20260824120000, and no comment at all on
-- `context_prompt_version` (it had none).
COMMENT ON COLUMN knowledge_units.chunk_context IS
    'Contextual Retrieval: LLM-generated sentences situating this chunk in its document. NOT the text shown to callers - that is always `text`. Prepended to `text` at embed time and indexed at weight B in content_tsv.';
COMMENT ON COLUMN knowledge_units.context_prompt_version IS NULL;
