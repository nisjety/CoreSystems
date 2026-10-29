-- Reverses 0013_subagent_result_read_capability.up.sql.
--
-- capability_scopes, capability_health, capability_versions, and skill_packages
-- carry foreign keys onto capabilities(id), so any rows an operator or health
-- authority attached to cap.agent.lineage.read are removed first. Scoping every
-- delete to this one capability id leaves cap.agent.spawn and the rest of the
-- 0008 execution-dispatch catalog untouched.

DELETE FROM capability_scopes WHERE capability_id = 'cap.agent.lineage.read';
DELETE FROM capability_health WHERE capability_id = 'cap.agent.lineage.read';
DELETE FROM capability_versions WHERE capability_id = 'cap.agent.lineage.read';
DELETE FROM skill_packages WHERE capability_id = 'cap.agent.lineage.read';

DELETE FROM capabilities
WHERE id = 'cap.agent.lineage.read'
  AND created_by = 'migration:0013_subagent_result_read_capability';
