-- 0103_agent_memory_scope_key.sql
--
-- One scope key for agent_memory (build plan EP-8 S6, decision D3 option b). `scope` stays the kind
-- and becomes a closed set; `scope_id` names the instance for the two kinds that need one:
--
--   scope_id           the Space id for scope 'space'; '<space_id>:<agent_id>' for scope 'agent'
--                      (EP-8 S10); NULL for every older kind, which keep session_id/owner as before.
--   audience_hash      the recipient audience hash of the message (or thread) a Space row was learned
--   audience_revision  from, stamped from server state at write. Recall (EP-8 S7) compares it with the
--                      thread's current audience, so a member added later does not see what was said
--                      before they joined (D5).
--   source_label       how the row came to exist: 'room_statement' (extracted from a room turn),
--                      'member_save' (an explicit save in a room), 'agent_memory' (EP-8 S10).
--
-- Every constraint is added NOT VALID: it holds for every row written from now on and does not scan the
-- existing table. Run `SELECT DISTINCT scope FROM agent_memory` on each deployment first; when it lists
-- only the kinds below, `ALTER TABLE agent_memory VALIDATE CONSTRAINT agent_memory_scope_chk` (and the
-- other three) can run as a separate step. Nothing here rewrites a row.
--
-- Space rows use owner '' and a key prefixed with the scope_id, so the existing unique index
-- (org_id, scope, owner, key) WHERE session_id IS NULL keeps two Spaces apart.
--
-- Numbered 0103: round A reserved 0075, which 0075_space_mentions_and_wake_origin holds on this branch;
-- round B moves round A's five session-core migrations to 0100-0104 (EP-8 S6 is the fourth). The reversal
-- is in revert/0103_agent_memory_scope_key.down.sql.

ALTER TABLE agent_memory
    ADD COLUMN IF NOT EXISTS scope_id          TEXT,
    ADD COLUMN IF NOT EXISTS audience_hash     TEXT,
    ADD COLUMN IF NOT EXISTS audience_revision BIGINT,
    ADD COLUMN IF NOT EXISTS source_label      TEXT;

ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_scope_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_scope_chk CHECK (
    scope IN ('run', 'thread', 'session', 'workspace', 'user', 'org', 'global', 'space', 'agent')
) NOT VALID;

-- The two instance kinds carry an instance; no other kind does.
ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_scope_id_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_scope_id_chk CHECK (
    (scope IN ('space', 'agent')) = (scope_id IS NOT NULL AND scope_id <> '')
) NOT VALID;

-- A Space row without the audience it was said to cannot be recalled safely, so it cannot be written.
ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_space_audience_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_space_audience_chk CHECK (
    scope <> 'space'
    OR (audience_hash IS NOT NULL AND audience_hash <> '' AND audience_revision IS NOT NULL)
) NOT VALID;

ALTER TABLE agent_memory DROP CONSTRAINT IF EXISTS agent_memory_source_label_chk;
ALTER TABLE agent_memory ADD CONSTRAINT agent_memory_source_label_chk CHECK (
    source_label IS NULL OR source_label IN ('room_statement', 'member_save', 'agent_memory')
) NOT VALID;

CREATE INDEX IF NOT EXISTS agent_memory_scope_id_idx
    ON agent_memory (org_id, scope, scope_id, review_state, is_latest)
    WHERE scope_id IS NOT NULL;
