-- 045_owner_effect_reservation_actions: widen the owner-effect reservation
-- action_id check from tickets.create alone to any dotted action id
-- (P2-cp-05-rel1, RUN_ACTION_TARGETS table mode).
--
-- The Go table stays the admission authority: decision.Validate(mode) runs
-- before any insert, so this check only bounds the shape of the id.
ALTER TABLE space_owner_effect_reservations
    DROP CONSTRAINT IF EXISTS space_owner_effect_reservations_action_id_check,
    ADD CONSTRAINT space_owner_effect_reservations_action_id_check
        CHECK (char_length(action_id) <= 128 AND action_id ~ '^[a-z][a-z0-9_]*(\.[a-z0-9_]+)+$');
