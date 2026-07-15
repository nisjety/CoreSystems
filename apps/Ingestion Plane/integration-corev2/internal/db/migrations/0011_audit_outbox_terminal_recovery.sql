CREATE OR REPLACE FUNCTION requeue_terminal_integration_audit_events(requested_ids TEXT[])
RETURNS INTEGER
LANGUAGE plpgsql
AS $$
DECLARE
    requeued_count INTEGER;
BEGIN
    IF requested_ids IS NULL OR NOT (cardinality(requested_ids) BETWEEN 1 AND 100) THEN
        RAISE EXCEPTION 'requested_ids must contain between 1 and 100 event ids';
    END IF;

    WITH requested AS (
        SELECT DISTINCT unnest(requested_ids) AS id
    )
    UPDATE integration_audit_events AS event
    SET terminal_at = NULL,
        processing_at = NULL,
        attempts = 0,
        next_attempt_at = now(),
        last_error = NULL
    FROM requested
    WHERE event.id = requested.id
      AND event.published_at IS NULL
      AND event.terminal_at IS NOT NULL;

    GET DIAGNOSTICS requeued_count = ROW_COUNT;
    RETURN requeued_count;
END;
$$;

REVOKE ALL ON FUNCTION requeue_terminal_integration_audit_events(TEXT[]) FROM PUBLIC;
