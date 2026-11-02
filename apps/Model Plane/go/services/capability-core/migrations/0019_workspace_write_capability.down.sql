-- Reverses 0019_workspace_write_capability.up.sql.
--
-- The seeded tool rows go first, then the rows that carry foreign keys onto
-- capabilities(id). Every delete is scoped to cap.workspace.write, so the read
-- capability (0018) and the rest of the execution-dispatch catalogue are untouched.
--
-- Rolling this back removes the capability row the two write tools are bound to,
-- so the policy gate refuses them again. A write a run already made stays in that
-- run's overlay where its upload put it; this migration never owned that state.

DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.workspace.write'
  AND source = 'seed';

DELETE FROM capability_scopes WHERE capability_id = 'cap.workspace.write';
DELETE FROM capability_health WHERE capability_id = 'cap.workspace.write';
DELETE FROM capability_versions WHERE capability_id = 'cap.workspace.write';
DELETE FROM skill_packages WHERE capability_id = 'cap.workspace.write';

DELETE FROM capabilities
WHERE id = 'cap.workspace.write'
  AND created_by = 'migration:0019_workspace_write_capability';
