CREATE TABLE IF NOT EXISTS session_audit_outbox (
    event_id TEXT PRIMARY KEY,
    subject TEXT NOT NULL,
    payload JSONB NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processing_at TIMESTAMPTZ,
    published_at TIMESTAMPTZ,
    terminal_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (length(event_id) BETWEEN 1 AND 128),
    CHECK (subject LIKE 'verevon.audit.v2.model.session-core.%')
);

CREATE INDEX IF NOT EXISTS session_audit_outbox_pending_idx
    ON session_audit_outbox (next_attempt_at, created_at)
    WHERE published_at IS NULL AND terminal_at IS NULL;

CREATE OR REPLACE FUNCTION claim_session_audit_event()
RETURNS TABLE(event_id TEXT, subject TEXT, payload JSONB, attempts INTEGER)
LANGUAGE sql
AS $$
    WITH candidate AS (
        SELECT pending.event_id
        FROM session_audit_outbox AS pending
        WHERE pending.published_at IS NULL
          AND pending.terminal_at IS NULL
          AND pending.next_attempt_at <= now()
          AND (pending.processing_at IS NULL OR pending.processing_at < now() - interval '1 minute')
        ORDER BY pending.next_attempt_at, pending.created_at
        FOR UPDATE SKIP LOCKED
        LIMIT 1
    )
    UPDATE session_audit_outbox AS outbox
    SET processing_at = now(), attempts = outbox.attempts + 1
    FROM candidate
    WHERE outbox.event_id = candidate.event_id
    RETURNING outbox.event_id, outbox.subject, outbox.payload, outbox.attempts;
$$;

REVOKE ALL ON FUNCTION claim_session_audit_event() FROM PUBLIC;
