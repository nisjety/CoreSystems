-- WS09-S7: a room manager or owner may stop another member's live run only when
-- the organisation has explicitly allowed it. Ending someone else's turn is a
-- different power from speaking in the room (thread_create_entitled) or reading
-- it (thread_read_entitled), so it gets its own standing deny-by-default floor
-- and is never inherited from either. One entitlement-catalogue row
-- (entitlement_catalog.go) carries the bit; nothing else in SQL changes.
ALTER TABLE space_effect_policies
    ADD COLUMN IF NOT EXISTS run_control_entitled BOOLEAN NOT NULL DEFAULT FALSE;
