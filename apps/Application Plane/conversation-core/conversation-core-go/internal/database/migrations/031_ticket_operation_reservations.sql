-- Add the owner-side half of owner-effect-reservation-v1. A pending or
-- cancelled operation carries only immutable commitments; ticket/audit IDs
-- remain NULL until Control has committed the matching reservation and the
-- local grant is rechecked in the final owner transaction.
ALTER TABLE conversation_ticket_operations
    ALTER COLUMN ticket_id DROP NOT NULL,
    ALTER COLUMN audit_event_id DROP NOT NULL;

ALTER TABLE conversation_ticket_operations
    ADD COLUMN IF NOT EXISTS action_schema_hash TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS payload_digest TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS decision_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS grant_ref TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS control_reservation_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS terminal_reason TEXT NOT NULL DEFAULT '';

ALTER TABLE conversation_ticket_operations
    DROP CONSTRAINT IF EXISTS conversation_ticket_operations_status_check;
ALTER TABLE conversation_ticket_operations
    ADD CONSTRAINT conversation_ticket_operations_status_check
    CHECK (status IN ('pending_control_commit', 'reserved', 'completed', 'cancelled', 'unknown'));

ALTER TABLE conversation_ticket_operations
    ADD CONSTRAINT conversation_ticket_operations_reservation_visibility_check
    CHECK (
        -- Legacy human operations have no reservation facts. A new
        -- reservation-backed operation must have all commitments plus the
        -- committed Control receipt before it can be visible.
        (status = 'completed' AND ticket_id IS NOT NULL AND audit_event_id IS NOT NULL AND
         ((action_schema_hash = '' AND payload_digest = '' AND decision_ref = '' AND grant_ref = '' AND control_reservation_id = '')
          OR (action_schema_hash <> '' AND payload_digest <> '' AND decision_ref <> '' AND grant_ref <> '' AND control_reservation_id <> '')))
        OR
        (status IN ('pending_control_commit', 'reserved', 'cancelled', 'unknown')
         AND ticket_id IS NULL AND audit_event_id IS NULL)
    );

CREATE UNIQUE INDEX IF NOT EXISTS conversation_ticket_operations_control_reservation_unique
    ON conversation_ticket_operations (control_reservation_id)
    WHERE control_reservation_id <> '';
