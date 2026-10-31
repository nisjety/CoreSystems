-- Reverses 0024_provider_read_tool.up.sql: removes only the seeded row of this one tool.
DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.tool.provider.read'
  AND tool_name = 'read_provider_action'
  AND source = 'seed';
