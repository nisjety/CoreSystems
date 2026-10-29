-- Canonical, bounded Inbox work-activity lookups. The projection deliberately
-- reads only allow-listed audit actions; this index keeps tenant-scoped,
-- per-conversation timelines inexpensive as the audit log grows.
CREATE INDEX IF NOT EXISTS conversation_audit_conversation_activity_idx
  ON conversation_audit_events (org_id, conversation_id, created_at DESC, id DESC)
  WHERE action IN (
    'conversation.created',
    'message.received',
    'message.sent',
    'message.submitted',
    'note.created',
    'outbound.delivery_recorded',
    'status.changed',
    'assignment.changed',
    'tag.added',
    'tag.removed',
    'ticket.created',
    'ticket.updated',
    'ticket.linked',
    'ticket.macro_run',
    'ticket.checklist_created',
    'ticket.checklist_item_updated'
  );
