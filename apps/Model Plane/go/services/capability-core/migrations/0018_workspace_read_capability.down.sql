-- Reverses 0018_workspace_read_capability.up.sql.
--
-- The seeded tool rows go first, then the rows that carry foreign keys onto
-- capabilities(id). Every delete is scoped to cap.workspace.read, so the
-- execution-dispatch catalogue and the other read capabilities are untouched.
--
-- Nothing is reconciled on the workspace side: these tools write nothing, so
-- rolling this back leaves no state behind that execution-core must clean up.

DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.workspace.read'
  AND source = 'seed';

DELETE FROM capability_scopes WHERE capability_id = 'cap.workspace.read';
DELETE FROM capability_health WHERE capability_id = 'cap.workspace.read';
DELETE FROM capability_versions WHERE capability_id = 'cap.workspace.read';
DELETE FROM skill_packages WHERE capability_id = 'cap.workspace.read';

DELETE FROM capabilities
WHERE id = 'cap.workspace.read'
  AND created_by = 'migration:0018_workspace_read_capability';
