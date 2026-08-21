-- Reverses 0011_conversation_ticket_action_capability.up.sql without touching
-- any other execution dispatch capability or a capability later adopted by an
-- operator under another creator.

DELETE FROM capability_scopes WHERE capability_id = 'cap.tool.ticket.create';
DELETE FROM capability_health WHERE capability_id = 'cap.tool.ticket.create';
DELETE FROM capability_versions WHERE capability_id = 'cap.tool.ticket.create';
DELETE FROM skill_packages WHERE capability_id = 'cap.tool.ticket.create';

DELETE FROM capabilities
WHERE id = 'cap.tool.ticket.create'
  AND created_by = 'migration:0011_conversation_ticket_action_capability';
