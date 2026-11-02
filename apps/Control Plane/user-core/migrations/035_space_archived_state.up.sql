-- 035_space_archived_state: reversible room archive as a Control access state
-- (WS04-S7, decision D-WS04-3).
--
-- Archive is a Control registration_state, orthogonal to Application's
-- lifecycle: 'archived' keeps the room readable (roster, index, thread read,
-- retrieval read) while every write and agent-effect resolver stays 'active'
-- only, so an archived room denies each write by default. archived_at and
-- archived_by record who archived the room and when; both are cleared on
-- unarchive (the ledger rows 'archived' / 'unarchived' keep the history).
--
-- Idempotent: every statement tolerates a re-run.

BEGIN;

ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_state_chk;
ALTER TABLE registered_spaces
    ADD CONSTRAINT registered_spaces_state_chk
    CHECK (registration_state IN ('active', 'archived', 'suspended', 'deleting', 'deleted'));

ALTER TABLE registered_spaces
    ADD COLUMN IF NOT EXISTS archived_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS archived_by TEXT;

COMMIT;
