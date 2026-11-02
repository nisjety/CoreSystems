-- 041_space_dm_kinds: direct-message Space kinds (WS12-S1).
--
-- A DM is a Control-registered Space of kind 'dm' (exactly 2 people) or
-- 'group_dm' (3 to 9). Its roster is fixed at registration. participant_hash
-- is a content-free commitment to the sorted participant set
-- ("sha256:" + hex(SHA-256("participant-set/v1\0" || len-prefixed ids))); the
-- partial unique index makes the same set resolve to one live DM per org.
-- dm_create_entitled is the deny-by-default org entitlement for opening one.
--
-- Idempotent: every statement tolerates a re-run.

BEGIN;

ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_kind_chk;
ALTER TABLE registered_spaces
    ADD CONSTRAINT registered_spaces_kind_chk
    CHECK (space_kind IN ('personal', 'room', 'project', 'case', 'dm', 'group_dm'));

ALTER TABLE registered_spaces
    ADD COLUMN IF NOT EXISTS participant_hash TEXT;

ALTER TABLE registered_spaces DROP CONSTRAINT IF EXISTS registered_spaces_dm_hash_chk;
ALTER TABLE registered_spaces
    ADD CONSTRAINT registered_spaces_dm_hash_chk
    CHECK ((space_kind IN ('dm', 'group_dm')) = (participant_hash IS NOT NULL));

CREATE UNIQUE INDEX IF NOT EXISTS uq_registered_spaces_dm_participants
    ON registered_spaces (org_id, participant_hash)
    WHERE participant_hash IS NOT NULL AND registration_state IN ('active', 'suspended');

ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS dm_create_entitled BOOLEAN NOT NULL DEFAULT FALSE;

COMMIT;
