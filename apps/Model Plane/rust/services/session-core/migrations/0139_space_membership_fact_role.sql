-- 0139: the last applied Control membership fact also records the subject's state (build plan W3 D1, E-PRIO-02).
--
-- space_membership_fact_order (0132) keeps the Space authority revision of the newest fact applied per (Space, subject).
-- It now also keeps what that fact said: active, and the subject's role for an active member (NULL when the producer
-- did not state one, or for a removal). The retraction consumer's sweep reads it to retract the live declassified
-- shares of a subject whose newest fact left them below editor (viewer, or an unknown role) when the demotion event
-- itself was missed and is older than Control's 24 h re-announce window. A row written before this migration keeps
-- NULL in both columns and is never swept, so the sweep only acts on a state Session Core actually saw.
--
-- Written only by space_membership_nats.rs and space_membership_retract_nats.rs, under the subject's ordering lock, in
-- the same statement order as the revision. Ids and a role name only.
-- Reversal: revert/0139_space_membership_fact_role.down.sql.

ALTER TABLE space_membership_fact_order
    ADD COLUMN IF NOT EXISTS active BOOLEAN,
    ADD COLUMN IF NOT EXISTS role   TEXT;

-- The demotion sweep: active members with a recorded role below editor.
CREATE INDEX IF NOT EXISTS space_membership_fact_order_below_editor_idx
    ON space_membership_fact_order (space_ref, subject_id)
    WHERE active IS TRUE AND role IS NOT NULL AND role NOT IN ('editor', 'manager', 'owner');
