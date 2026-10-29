-- Ticket-side conversations are internal coordination records. They carry no
-- customer channel, recipient, provider reference, or outbound delivery state.
CREATE TABLE IF NOT EXISTS conversation_ticket_side_conversations (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    ticket_id TEXT NOT NULL REFERENCES conversation_tickets(id) ON DELETE CASCADE,
    subject TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    created_by_user_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (char_length(subject) BETWEEN 1 AND 160),
    CHECK (position(E'\n' IN subject) = 0 AND position(E'\r' IN subject) = 0),
    CHECK (status IN ('open', 'closed'))
);

CREATE INDEX IF NOT EXISTS conversation_ticket_side_conversations_ticket_idx
    ON conversation_ticket_side_conversations (org_id, ticket_id, updated_at DESC, id DESC);

CREATE TABLE IF NOT EXISTS conversation_ticket_side_conversation_messages (
    id TEXT PRIMARY KEY,
    org_id TEXT NOT NULL,
    side_conversation_id TEXT NOT NULL REFERENCES conversation_ticket_side_conversations(id) ON DELETE CASCADE,
    body_text TEXT NOT NULL,
    created_by_user_id TEXT NOT NULL DEFAULT '',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CHECK (char_length(body_text) BETWEEN 1 AND 4000)
);

CREATE INDEX IF NOT EXISTS conversation_ticket_side_conversation_messages_thread_idx
    ON conversation_ticket_side_conversation_messages (org_id, side_conversation_id, created_at ASC, id ASC);

-- Keep the bounded ticket and Inbox activity projections aligned with the
-- new, body-free audit records. Do not store internal message content in audit.
DROP INDEX IF EXISTS conversation_audit_ticket_activity_idx;
CREATE INDEX conversation_audit_ticket_activity_idx
    ON conversation_audit_events (org_id, (payload ->> 'ticket_id'), created_at DESC, id DESC)
    WHERE action IN (
      'ticket.created', 'ticket.updated', 'ticket.linked', 'ticket.macro_run',
      'ticket.checklist_created', 'ticket.checklist_item_updated',
      'ticket.side_conversation_created', 'ticket.side_conversation_message_added',
      'ticket.side_conversation_updated'
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
      'ticket.side_conversation_updated'
    );
