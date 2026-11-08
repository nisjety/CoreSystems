ALTER TABLE integration_audit_events
    ADD COLUMN IF NOT EXISTS request_id TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
	ADD COLUMN IF NOT EXISTS processing_at TIMESTAMPTZ,
	ADD COLUMN IF NOT EXISTS published_at TIMESTAMPTZ,
	ADD COLUMN IF NOT EXISTS terminal_at TIMESTAMPTZ,
	ADD COLUMN IF NOT EXISTS last_error TEXT,
	ADD COLUMN IF NOT EXISTS legacy_pre_outbox BOOLEAN NOT NULL DEFAULT FALSE;

-- Rows that existed before this migration were already offered through the
-- former fire-and-forget HTTP forwarder. Blindly treating them as pending
-- would replay the entire historical ledger under new event IDs, defeating
-- downstream deduplication and starving current security events. Quarantine
-- them as acknowledged; operators may explicitly requeue only IDs proven
-- missing through the bounded function below.
UPDATE integration_audit_events
SET published_at = now(),
    legacy_pre_outbox = TRUE
WHERE published_at IS NULL;

CREATE INDEX IF NOT EXISTS integration_audit_events_pending_idx
    ON integration_audit_events (next_attempt_at, created_at)
    WHERE published_at IS NULL AND terminal_at IS NULL;

CREATE OR REPLACE FUNCTION claim_integration_audit_event()
RETURNS TABLE(
    id TEXT,
    organization_id TEXT,
    user_id TEXT,
    connection_id TEXT,
    event_type TEXT,
    provider_key TEXT,
    metadata JSONB,
    request_id TEXT,
    created_at TIMESTAMPTZ,
    attempts INTEGER
)
LANGUAGE sql
AS $$
    WITH candidate AS (
        SELECT pending.id
        FROM integration_audit_events AS pending
        WHERE pending.published_at IS NULL
          AND pending.terminal_at IS NULL
          AND pending.next_attempt_at <= now()
          AND (pending.processing_at IS NULL OR pending.processing_at < now() - interval '1 minute')
        ORDER BY pending.next_attempt_at, pending.created_at
        FOR UPDATE SKIP LOCKED
        LIMIT 1
    )
    UPDATE integration_audit_events AS event
    SET processing_at = now(), attempts = event.attempts + 1
    FROM candidate
    WHERE event.id = candidate.id
    RETURNING event.id, event.organization_id, event.user_id,
        event.connection_id, event.event_type, event.provider_key,
        event.metadata, event.request_id, event.created_at, event.attempts;
$$;

REVOKE ALL ON FUNCTION claim_integration_audit_event() FROM PUBLIC;

CREATE OR REPLACE FUNCTION requeue_legacy_integration_audit_events(requested_ids TEXT[])
RETURNS INTEGER
LANGUAGE plpgsql
AS $$
DECLARE
    requeued_count INTEGER;
BEGIN
    IF requested_ids IS NULL OR NOT (cardinality(requested_ids) BETWEEN 1 AND 1000) THEN
        RAISE EXCEPTION 'requested_ids must contain between 1 and 1000 event ids';
    END IF;

    WITH requested AS (
        SELECT DISTINCT unnest(requested_ids) AS id
    )
    UPDATE integration_audit_events AS event
    SET published_at = NULL,
        terminal_at = NULL,
        processing_at = NULL,
        attempts = 0,
        next_attempt_at = now(),
        last_error = NULL
    FROM requested
    WHERE event.id = requested.id
      AND event.legacy_pre_outbox IS TRUE;

    GET DIAGNOSTICS requeued_count = ROW_COUNT;
    RETURN requeued_count;
END;
$$;

REVOKE ALL ON FUNCTION requeue_legacy_integration_audit_events(TEXT[]) FROM PUBLIC;
