-- Reverts 045. Fails loudly when a non-ticket reservation row exists, on purpose:
-- those rows must be settled before the narrower check can return.
ALTER TABLE space_owner_effect_reservations
    DROP CONSTRAINT IF EXISTS space_owner_effect_reservations_action_id_check,
    ADD CONSTRAINT space_owner_effect_reservations_action_id_check CHECK (action_id = 'tickets.create');
