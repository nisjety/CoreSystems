-- 0132: the last Control membership fact applied per (Space, subject), by the Space's authority revision (W3-DEC-ORDER).
--
-- Control states every membership fact on controlplane.space.membership_changed with space_seq: the Space's authority
-- revision at the change (the ledger row, the inline publish and the re-announce all carry it; membership_events_outbox.go).
-- A revision advances under the Space's revision row lock, in the same transaction as the membership write, so a later
-- change to a subject always carries a larger space_seq. Session Core applies a fact only when its space_seq is newer
-- than the value stored here, so a stale active=true delivered after a removal cannot reopen a room, and a stale removal
-- cannot revoke a member who was re-added.
--
-- Its own table, not a column on space_membership_revocations: a re-activation deletes that row, and the ordering must
-- outlive it. Keyed as the revocation projection is (space_ref, subject_id); org_id is data. Written only by
-- space_membership_nats.rs and space_membership_retract_nats.rs, under the subject's advisory lock.
-- Reversal: revert/0132_space_membership_fact_order.down.sql.

CREATE TABLE IF NOT EXISTS space_membership_fact_order (
    space_ref  TEXT NOT NULL,
    subject_id TEXT NOT NULL,
    org_id     TEXT NOT NULL,
    space_seq  BIGINT NOT NULL,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (space_ref, subject_id)
);
