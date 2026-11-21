-- Reverses 0135_validate_agent_memory_scope_constraints.sql: puts the four agent_memory scope constraints back to
-- NOT VALID, exactly as 0103_agent_memory_scope_key.sql declares them. Nothing else changes: no row is read out,
-- deleted or rewritten.
--
-- Not applied by sqlx: this file lives in a subdirectory, and sqlx's migrate! reads only the top level of
-- migrations/. Run it by hand, against the database only, inside one transaction.
--
-- Safe to run twice: each constraint is dropped if it exists, then added NOT VALID.

BEGIN;

ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_scope_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_scope_chk CHECK (
    scope IN ('run', 'thread', 'session', 'workspace', 'user', 'org', 'global', 'space', 'agent')
) NOT VALID;

ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_scope_id_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_scope_id_chk CHECK (
    (scope IN ('space', 'agent')) = (scope_id IS NOT NULL AND scope_id <> '')
) NOT VALID;

ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_space_audience_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_space_audience_chk CHECK (
    scope <> 'space'
    OR (audience_hash IS NOT NULL AND audience_hash <> '' AND audience_revision IS NOT NULL)
) NOT VALID;

ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_source_label_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_source_label_chk CHECK (
    source_label IS NULL OR source_label IN ('room_statement', 'member_save', 'agent_memory')
) NOT VALID;

COMMIT;
