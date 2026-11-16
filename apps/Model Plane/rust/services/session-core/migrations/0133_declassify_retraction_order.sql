-- 0133: the Space authority revision each declassified share was authorised at, and the revision of the last removal or
-- demotion whose copies Session Core retracted, per (Space, subject) (W3-DEC-ORDER2).
--
-- memory_declassifications.authority_revision is the Control authority revision the verified declassify decision binds.
-- It is the counter the membership facts carry as space_seq (membership_ops.go states SpaceSeq as the revision after the
-- change). A removal or demotion at revision R retracts the subject's shares whose authority_revision is below R: those
-- were authorised before the change. A share at or above R was authorised after it and survives, even after a later
-- re-add. NULL marks a share written before this migration: no revision was recorded for it, so any removal or demotion
-- retracts it (fail closed). Existing rows are not backfilled.
--
-- space_membership_retract_marks holds, per (Space, subject), the highest revision of a removal or demotion whose
-- retraction has run. A declassify whose decision revision is below it is refused: the change it predates has already
-- retracted its copies, so the share would survive the retraction. Written only by space_membership_retract_nats.rs and
-- its sweep, in one transaction with the retraction, under the subject's ordering lock and the (org, Space, subject)
-- lock that declassify shares. Its org_id is data, keyed as the revocation projection is.
-- Reversal: revert/0133_declassify_retraction_order.down.sql.

ALTER TABLE memory_declassifications
    ADD COLUMN IF NOT EXISTS authority_revision BIGINT
        CHECK (authority_revision IS NULL OR authority_revision >= 0);

CREATE TABLE IF NOT EXISTS space_membership_retract_marks (
    space_ref   TEXT NOT NULL,
    subject_id  TEXT NOT NULL,
    org_id      TEXT NOT NULL,
    retract_seq BIGINT NOT NULL CHECK (retract_seq >= 0),
    applied_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (space_ref, subject_id)
);
