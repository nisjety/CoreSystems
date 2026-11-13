-- Reverses 0041_custom_rest_capabilities.up.sql: removes only the two custom REST capabilities this migration
-- created and the seeded rows of their tools.
--
-- capability_scopes, capability_health, capability_versions and skill_packages carry foreign keys onto
-- capabilities(id), so any rows attached to these two ids are removed first. Every delete is scoped to these
-- ids, so the rest of the catalogue is untouched.

DELETE FROM capability_tools
WHERE org_id = 'global'
  AND capability_id IN ('cap.custom_rest.read', 'cap.custom_rest.write')
  AND source = 'seed';

DELETE FROM capability_scopes WHERE capability_id IN ('cap.custom_rest.read', 'cap.custom_rest.write');
DELETE FROM capability_health WHERE capability_id IN ('cap.custom_rest.read', 'cap.custom_rest.write');
DELETE FROM capability_versions WHERE capability_id IN ('cap.custom_rest.read', 'cap.custom_rest.write');
DELETE FROM skill_packages WHERE capability_id IN ('cap.custom_rest.read', 'cap.custom_rest.write');

DELETE FROM capabilities
WHERE id IN ('cap.custom_rest.read', 'cap.custom_rest.write')
  AND created_by = 'migration:0041_custom_rest_capabilities';
