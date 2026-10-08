-- A recovery path for terminal audit-outbox rows.
--
-- A row becomes terminal after MAX_AUDIT_DISPATCH_ATTEMPTS failed publishes and
-- the drainer never claims it again. Until now that was a dead end: on this
-- stack 4,992 `tool_action` rows ran out of attempts against a broker that had
-- no stream for their subject (the stream was never provisioned), and the only
-- way back was hand-written SQL against a production table.
--
-- This gives the way back a name, an audit trail and a guard:
--
--   SELECT requeue_session_audit_terminal(
--       'verevon.audit.v2.model.session-core.tool_action', 10);   -- canary
--   SELECT requeue_session_audit_terminal(
--       'verevon.audit.v2.model.session-core.tool_action', 10000); -- the rest
--
-- It touches only rows that are terminal AND unpublished, only those whose
-- subject matches (exact, or a LIKE pattern), never more than the limit, and it
-- is safe to run twice (a second call finds nothing terminal and returns 0).
-- It keeps `last_error` so the evidence of why the row failed survives until a
-- publish succeeds (the drainer clears it then), and counts every requeue so a
-- row that keeps coming back is visible. user-core's `user_audit_outbox` uses
-- the same two columns for the same purpose.
--
-- Replaying is idempotent downstream: audit-core keys an audit event on
-- (source_bus, source_producer, event_id) and ignores an identical repeat, so a
-- requeued row that had in fact been delivered is stored once.

ALTER TABLE session_audit_outbox
    ADD COLUMN IF NOT EXISTS requeue_count INTEGER NOT NULL DEFAULT 0
        CHECK (requeue_count >= 0),
    ADD COLUMN IF NOT EXISTS requeued_at TIMESTAMPTZ;

CREATE OR REPLACE FUNCTION requeue_session_audit_terminal(
    p_subject TEXT,
    p_limit INTEGER DEFAULT 1000
)
RETURNS INTEGER
LANGUAGE plpgsql
AS $$
DECLARE
    requeued INTEGER;
BEGIN
    IF p_subject IS NULL OR length(p_subject) = 0 THEN
        RAISE EXCEPTION 'requeue_session_audit_terminal: a subject (or LIKE pattern) is required';
    END IF;
    IF p_limit IS NULL OR p_limit < 1 OR p_limit > 100000 THEN
        RAISE EXCEPTION 'requeue_session_audit_terminal: the limit must be between 1 and 100000';
    END IF;

    WITH picked AS (
        SELECT stuck.event_id
        FROM session_audit_outbox AS stuck
        WHERE stuck.published_at IS NULL
          AND stuck.terminal_at IS NOT NULL
          AND stuck.subject LIKE p_subject
        ORDER BY stuck.created_at, stuck.event_id
        LIMIT p_limit
        FOR UPDATE SKIP LOCKED
    )
    UPDATE session_audit_outbox AS outbox
    SET terminal_at = NULL,
        processing_at = NULL,
        attempts = 0,
        next_attempt_at = now(),
        requeue_count = outbox.requeue_count + 1,
        requeued_at = now()
    FROM picked
    WHERE outbox.event_id = picked.event_id;

    GET DIAGNOSTICS requeued = ROW_COUNT;
    RETURN requeued;
END;
$$;

REVOKE ALL ON FUNCTION requeue_session_audit_terminal(TEXT, INTEGER) FROM PUBLIC;
