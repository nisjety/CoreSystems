-- Reverses 0081_agent_memory_audience_labels.sql.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads
-- only the top level of migrations/. Run it by hand, against the database only,
-- inside one transaction.
--
-- Drops the label checks, the Space index and the two label columns this file added. The
-- columns it restates (scope_id, audience_hash, audience_revision, source_label) belong to
-- 0103 and are kept. Dropping origin_kind loses which rows are withheld: roll session-core
-- back first (a build that reads origin_kind fails on a missing column), then run this file.
--
-- Safe to run twice.

BEGIN;
ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_label_chk;
ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_origin_kind_chk;
DROP INDEX IF EXISTS agent_memory_space_idx;
ALTER TABLE agent_memory DROP COLUMN IF EXISTS space_ref;
ALTER TABLE agent_memory DROP COLUMN IF EXISTS origin_kind;
COMMIT;
