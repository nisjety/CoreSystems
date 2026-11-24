-- Reverses 0048_space_directory_resolve_capability.up.sql. It removes the space.directory.resolve admission row and
-- capability row the migration created, and nothing else. A capability an operator has since adopted under another
-- creator is kept.

DELETE FROM agent_action_admissions
WHERE action_id = 'space.directory.resolve';

DELETE FROM capability_scopes
WHERE capability_id = 'cap.tool.space.directory.resolve';
DELETE FROM capability_health
WHERE capability_id = 'cap.tool.space.directory.resolve';
DELETE FROM capability_versions
WHERE capability_id = 'cap.tool.space.directory.resolve';
DELETE FROM skill_packages
WHERE capability_id = 'cap.tool.space.directory.resolve';

DELETE FROM capabilities
WHERE id = 'cap.tool.space.directory.resolve'
  AND created_by = 'migration:0048_space_directory_resolve_capability';
