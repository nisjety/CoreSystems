-- A handoff receipt records only an operator request to open the separately
-- owned Chat surface. It is not evidence of a Chat action or customer delivery.
DROP INDEX IF EXISTS conversation_audit_ticket_activity_idx;
CREATE INDEX conversation_audit_ticket_activity_idx
    ON conversation_audit_events (org_id, (payload ->> 'ticket_id'), created_at DESC, id DESC)
    WHERE action IN (
      'ticket.created', 'ticket.updated', 'ticket.linked', 'ticket.macro_run',
      'ticket.checklist_created', 'ticket.checklist_item_updated',
      'ticket.side_conversation_created', 'ticket.side_conversation_message_added',
      'ticket.side_conversation_updated', 'ticket.chat_handoff_requested'
    );

DROP INDEX IF EXISTS conversation_audit_conversation_activity_idx;
CREATE INDEX conversation_audit_conversation_activity_idx
    ON conversation_audit_events (org_id, conversation_id, created_at DESC, id DESC)
    WHERE action IN (
      'conversation.created', 'message.received', 'message.sent', 'message.submitted',
      'note.created', 'outbound.delivery_recorded', 'status.changed', 'assignment.changed',
      'tag.added', 'tag.removed', 'ticket.created', 'ticket.updated', 'ticket.linked',
      'ticket.macro_run', 'ticket.checklist_created', 'ticket.checklist_item_updated',
      'ticket.side_conversation_created', 'ticket.side_conversation_message_added',
      'ticket.side_conversation_updated', 'ticket.chat_handoff_requested'
    );
