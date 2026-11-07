-- 0102_approval_descriptor_revision.sql
--
-- Approve with edits (EP-10 S4; reserved as 0074 in round A, renumbered to 0102 by round B C-B2).
-- A person may change a pending mail send's recipients, subject or plain-text body and grant it in the
-- same act (decision D1). The edited call replaces the input inside the existing encrypted descriptor;
-- these columns record that it happened:
--
--   revision                     how many times the call was edited before the grant (0 = as drafted,
--                                at most 3). An edit grants in its own transaction, so in practice 0 or 1.
--   original_action_fingerprint  the fingerprint of the call as first drafted, kept once (COALESCE) when
--                                the first edit lands. A 64-hex digest only: the original content is not
--                                kept anywhere, so the person's replaced draft is gone with the edit.
--   revised_at                   when the last edit landed.
--
-- ZDR. A zero-retention run has no descriptor, and an edit is refused for ZDR and waived callers before
-- any read (DEC-32: a waived run counts as ZDR for a new store dimension). The columns carry no content;
-- the edited input stays in `ciphertext` with the same lifetime and erasure as before (the row is
-- deleted with its approval and cascades on run delete, migration 0022).
--
-- Additive and idempotent. Written only while APPROVAL_EDIT_ENABLED is on in session-core, so the
-- migration is inert on its own: every existing row reads revision 0.

ALTER TABLE approval_continuation_descriptors
    ADD COLUMN IF NOT EXISTS revision SMALLINT NOT NULL DEFAULT 0
        CHECK (revision BETWEEN 0 AND 3),
    ADD COLUMN IF NOT EXISTS original_action_fingerprint TEXT NULL
        CHECK (original_action_fingerprint IS NULL OR length(original_action_fingerprint) = 64),
    ADD COLUMN IF NOT EXISTS revised_at TIMESTAMPTZ NULL;
