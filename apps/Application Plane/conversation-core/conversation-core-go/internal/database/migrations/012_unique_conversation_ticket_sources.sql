-- 012: a support conversation may be attached to one existing ticket at most.
--
-- `conversation_tickets.conversation_id` remains the ticket's immutable
-- primary source. This partial index governs only secondary Inbox handoffs
-- represented by a `conversation_source` linked resource.

CREATE UNIQUE INDEX IF NOT EXISTS conversation_linked_resources_conversation_source_once_idx
    ON conversation_linked_resources (org_id, resource_id)
    WHERE resource_kind = 'conversation_source' AND resource_id <> '';
