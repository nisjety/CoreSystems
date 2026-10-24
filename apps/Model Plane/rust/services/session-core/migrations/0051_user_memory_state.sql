-- Per-user memory state and its content-free change log (build plan WS07 S1).
--
-- `user_memory_controls` (0039) holds one row per (org, user) and was only ever the
-- row lock that serialises a delete against an extraction. It now also holds the
-- user's own choice, which `memory_control::resolve` reads under that same lock so
-- every memory writer and reader sees one answer:
--
--   on       the default: memory is used and learned from as before
--   paused   rows stay visible and deletable but are not recalled, and nothing new is
--            learned; what was said while paused is never mined afterwards
--   off      as paused, and the user's rows are also hidden from the list
--
--   memory_state       the choice; every existing row reads 'on', so nothing changes
--                      for anyone until they set it
--   state_changed_at   when the choice last changed; NULL for a user who never set it
--   state_changed_by   the principal that changed it (the user themselves)
--
-- `memory_control_events` is the audit record of a change. It is content-free by
-- construction: states, an actor and counts, never a memory or a message.
-- `detail` carries counts only. Rows are erased with the organization
-- (gdpr::purge_org_data), never kept past it.
--
-- Additive: new nullable/defaulted columns, one new table, no backfill. The reversal
-- is in revert/0051_user_memory_state.down.sql.
ALTER TABLE user_memory_controls
    ADD COLUMN IF NOT EXISTS memory_state TEXT NOT NULL DEFAULT 'on',
    ADD COLUMN IF NOT EXISTS state_changed_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS state_changed_by TEXT;

-- NOT VALID first, VALIDATE second, as 0050 does: a validated CHECK scans the table
-- under ACCESS EXCLUSIVE. The guard keeps a re-run harmless.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'user_memory_controls_state_check'
    ) THEN
        ALTER TABLE user_memory_controls
            ADD CONSTRAINT user_memory_controls_state_check
            CHECK (memory_state IN ('on', 'paused', 'off')) NOT VALID;
    END IF;
END
$$;
ALTER TABLE user_memory_controls VALIDATE CONSTRAINT user_memory_controls_state_check;

CREATE TABLE IF NOT EXISTS memory_control_events (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    event TEXT NOT NULL
        CHECK (event IN ('state_changed', 'reset')),
    from_state TEXT NOT NULL DEFAULT ''
        CHECK (from_state IN ('', 'on', 'paused', 'off')),
    to_state TEXT NOT NULL DEFAULT ''
        CHECK (to_state IN ('', 'on', 'paused', 'off')),
    actor TEXT NOT NULL DEFAULT '',
    detail JSONB NOT NULL DEFAULT '{}'::jsonb,
    at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS memory_control_events_user_at_idx
    ON memory_control_events (org_id, user_id, at);
