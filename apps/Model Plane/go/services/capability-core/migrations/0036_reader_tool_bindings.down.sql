-- Reverses 0036_reader_tool_bindings.up.sql: removes only the seeded rows of the two reader tools.
DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.tool.http'
  AND tool_name IN ('read_page', 'find_in_page')
  AND source = 'seed';
