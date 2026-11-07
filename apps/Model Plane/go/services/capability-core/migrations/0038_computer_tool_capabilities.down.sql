-- Reverses 0038_computer_tool_capabilities.up.sql: removes only the two computer capabilities this migration
-- created and the seeded rows of their tools.
--
-- capability_scopes, capability_health, capability_versions and skill_packages carry foreign keys onto
-- capabilities(id), so any rows attached to these two ids are removed first. Every delete is scoped to these
-- ids, so the rest of the catalogue is untouched.

DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id IN ('cap.computer.observe', 'cap.computer.act')
  AND source = 'seed';

DELETE FROM capability_scopes WHERE capability_id IN ('cap.computer.observe', 'cap.computer.act');
DELETE FROM capability_health WHERE capability_id IN ('cap.computer.observe', 'cap.computer.act');
DELETE FROM capability_versions WHERE capability_id IN ('cap.computer.observe', 'cap.computer.act');
DELETE FROM skill_packages WHERE capability_id IN ('cap.computer.observe', 'cap.computer.act');

DELETE FROM capabilities
WHERE id IN ('cap.computer.observe', 'cap.computer.act')
  AND created_by = 'migration:0038_computer_tool_capabilities';
