-- Reverses 0029_ask_user_tool_class.up.sql: removes only the seeded row of this one tool.
DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.memory.index'
  AND tool_name = 'ask_user'
  AND source = 'seed';
