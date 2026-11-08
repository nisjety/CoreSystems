-- Reverses 0034_browser_console_tool.up.sql: removes only the seeded row of this one tool.
DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.browser.open'
  AND tool_name = 'browser.console'
  AND source = 'seed';
