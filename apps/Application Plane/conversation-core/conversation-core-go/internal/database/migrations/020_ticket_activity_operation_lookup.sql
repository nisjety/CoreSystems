-- Include the existing macro and checklist controls in the bounded activity
-- projection. This is an additional index so installations that already ran
-- migration 019 receive the broader predicate without rewriting history.
CREATE INDEX IF NOT EXISTS conversation_audit_ticket_operation_activity_idx
    ON conversation_audit_events (org_id, (payload ->> 'ticket_id'), created_at DESC, id DESC)
    WHERE action IN (
        'ticket.created',
        'ticket.updated',
        'ticket.linked',
        'ticket.macro_run',
        'ticket.checklist_created',
        'ticket.checklist_item_updated'
    );
