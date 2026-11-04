-- Reverses 0103_agent_memory_scope_key.sql: drops the scope key columns, their constraints and index.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- ORDER, all steps:
--   1. set SESSION_SPACE_MEMORY_ENABLED off (or roll session-core back to a build without space_memory.rs),
--      or the next Space write fails on the missing columns;
--   2. delete the Space and agent rows first: without scope_id they cannot be told apart by Space, and
--      their keys alone do not say which room's audience they were learned under.
--        DELETE FROM agent_memory WHERE scope IN ('space', 'agent');
--
-- Safe to run twice (every DROP is IF EXISTS).

BEGIN;

DELETE FROM agent_memory WHERE scope IN ('space', 'agent');

DROP INDEX IF EXISTS agent_memory_scope_id_idx;

ALTER TABLE agent_memory
    DROP CONSTRAINT IF EXISTS agent_memory_source_label_chk,
    DROP CONSTRAINT IF EXISTS agent_memory_space_audience_chk,
    DROP CONSTRAINT IF EXISTS agent_memory_scope_id_chk,
    DROP CONSTRAINT IF EXISTS agent_memory_scope_chk;

ALTER TABLE agent_memory
    DROP COLUMN IF EXISTS source_label,
    DROP COLUMN IF EXISTS audience_revision,
    DROP COLUMN IF EXISTS audience_hash,
    DROP COLUMN IF EXISTS scope_id;

COMMIT;
