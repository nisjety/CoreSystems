-- Ticket activity is read by ticket ID from a bounded audit projection. The
-- expression index keeps that lookup tenant-scoped without returning raw JSON.
CREATE INDEX IF NOT EXISTS conversation_audit_ticket_activity_idx
    ON conversation_audit_events (org_id, (payload ->> 'ticket_id'), created_at DESC, id DESC)
    WHERE action IN ('ticket.created', 'ticket.updated', 'ticket.linked');
