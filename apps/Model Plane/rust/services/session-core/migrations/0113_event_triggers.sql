-- 0113: event trigger records (Wave 4 EP-7, F-t3-047, slice W4-F T1).
--
-- A trigger binds one integration connection, one Space and one Space member, and names the connector
-- event type that may wake a budgeted run for that member. This slice stores the record and nothing
-- fires: a stored trigger is not authority (the cron rule: a long-lived record is never authority by
-- itself), so every later fire is re-authorised by Control through FireAuthorizer. `clock_enabled` is
-- stored only; nothing reads it yet.
--
-- Zero Data Retention: the row holds ids, a closed event type, a short label, a flag and a status. A
-- zero-retention or waived caller is refused on every RPC (`zdr_durable_mutation_forbidden`), so no ZDR
-- content is ever here. Erasure: rows go with their org (the gdpr.rs purge deletes them before runs and
-- threads). The table names no thread or run, so nothing cascades into it. Per-member erasure is not in
-- this slice.
--
-- Additive only: a new, empty table. The reversal is in revert/0113_event_triggers.down.sql.

CREATE TABLE IF NOT EXISTS event_triggers (
    id TEXT PRIMARY KEY CHECK (id LIKE 'trg\_%' AND length(id) <= 64),
    org_id TEXT NOT NULL CHECK (length(org_id) BETWEEN 1 AND 128),
    space_ref TEXT NOT NULL CHECK (length(space_ref) BETWEEN 1 AND 128),
    connection_id TEXT NOT NULL CHECK (length(connection_id) BETWEEN 1 AND 128),
    -- The Space member the trigger belongs to: the only reader and writer, and the authority a fire carries.
    member_user_id TEXT NOT NULL CHECK (length(member_user_id) BETWEEN 1 AND 256),
    event_type TEXT NOT NULL CHECK (event_type IN ('webhook_received')),
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    clock_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    status TEXT NOT NULL CHECK (status IN ('active', 'paused', 'disabled')),
    revision BIGINT NOT NULL DEFAULT 1 CHECK (revision >= 1),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- A member's own triggers (the listing, and the member check on every read and write).
CREATE INDEX IF NOT EXISTS event_triggers_member_idx
    ON event_triggers (org_id, member_user_id, space_ref, created_at DESC);

-- The fire path's lookup in a later slice: the live triggers of one connection.
CREATE INDEX IF NOT EXISTS event_triggers_connection_idx
    ON event_triggers (connection_id, status);
