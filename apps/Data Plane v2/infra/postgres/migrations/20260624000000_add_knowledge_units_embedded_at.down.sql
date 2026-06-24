-- Revert Phase 3 embedded_at. The migrator wraps this file in a transaction;
-- do not declare BEGIN/COMMIT here.
DROP INDEX IF EXISTS idx_ku_embedded_at;
ALTER TABLE knowledge_units DROP COLUMN IF EXISTS embedded_at;
