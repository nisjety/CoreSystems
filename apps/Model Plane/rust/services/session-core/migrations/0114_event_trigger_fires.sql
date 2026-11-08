-- 0114: event trigger fire records (Wave 4 EP-7, F-t3-047, slice W4-F T2).
--
-- A row is one fire of one trigger for one webhook event. The capability-core consumer of integration-corev2's
-- webhook_received event writes it as pending, keyed on (trigger, webhook event id) so a redelivered event adds
-- nothing. The capability-core sweeper decides it: the trigger must still be active, the trigger and its org must be
-- under their hourly budgets, and Control must authorise the fire afresh (the cron rule). The row then reads
-- authorized, or refused with an outcome. Nothing in this table starts a run; the wake gate reads authorized rows in
-- a later slice.
--
-- Zero Data Retention: the row holds ids, a closed status and outcome, an attempt count and two times. The webhook
-- body is never fetched and never stored here. A trigger exists only for a non-ZDR, non-waived member, and Control
-- refuses a ZDR decision, so no ZDR content reaches this table. Erasure: rows go with their trigger (ON DELETE
-- CASCADE), and the org purge deletes the triggers.
--
-- Additive only: a new, empty table. The reversal is in revert/0114_event_trigger_fires.down.sql.

CREATE TABLE IF NOT EXISTS event_trigger_fires (
    id TEXT PRIMARY KEY CHECK (id LIKE 'trgfire\_%' AND length(id) <= 64),
    trigger_id TEXT NOT NULL REFERENCES event_triggers(id) ON DELETE CASCADE,
    org_id TEXT NOT NULL CHECK (length(org_id) BETWEEN 1 AND 128),
    webhook_event_id TEXT NOT NULL CHECK (length(webhook_event_id) BETWEEN 1 AND 128),
    status TEXT NOT NULL CHECK (status IN ('pending', 'authorized', 'refused')),
    outcome TEXT NOT NULL DEFAULT '' CHECK (length(outcome) <= 64),
    attempts INT NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    settled_at TIMESTAMPTZ NULL,
    -- One fire per trigger per webhook event: the dedupe key the consumer relies on.
    CONSTRAINT event_trigger_fires_once UNIQUE (trigger_id, webhook_event_id),
    -- A fire is settled exactly when it is not pending.
    CONSTRAINT event_trigger_fires_settled CHECK ((status = 'pending') = (settled_at IS NULL))
);

-- The sweep's claim: the pending fires, oldest first.
CREATE INDEX IF NOT EXISTS event_trigger_fires_pending_idx
    ON event_trigger_fires (created_at, id) WHERE status = 'pending';

-- The budgets: one org's and one trigger's authorised fires in the window.
CREATE INDEX IF NOT EXISTS event_trigger_fires_org_idx
    ON event_trigger_fires (org_id, status, settled_at);

CREATE INDEX IF NOT EXISTS event_trigger_fires_trigger_idx
    ON event_trigger_fires (trigger_id, status, settled_at);
