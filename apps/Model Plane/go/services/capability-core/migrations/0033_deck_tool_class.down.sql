-- Reverses 0033_deck_tool_class.up.sql: removes only the seeded row of this one tool.
DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.command.sandbox'
  AND tool_name = 'deck'
  AND source = 'seed';
