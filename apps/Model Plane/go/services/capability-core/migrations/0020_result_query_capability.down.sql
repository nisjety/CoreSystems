-- Reverses 0020_result_query_capability.up.sql.
--
-- capability_tools, capability_scopes, capability_health, capability_versions and
-- skill_packages carry foreign keys onto capabilities(id), so the rows attached to
-- cap.result.query are removed first. Every delete is scoped to this one id, so
-- the rest of the execution-dispatch catalogue is untouched.

DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id = 'cap.result.query'
  AND source = 'seed';
DELETE FROM capability_scopes WHERE capability_id = 'cap.result.query';
DELETE FROM capability_health WHERE capability_id = 'cap.result.query';
DELETE FROM capability_versions WHERE capability_id = 'cap.result.query';
DELETE FROM skill_packages WHERE capability_id = 'cap.result.query';

DELETE FROM capabilities
WHERE id = 'cap.result.query'
  AND created_by = 'migration:0020_result_query_capability';
