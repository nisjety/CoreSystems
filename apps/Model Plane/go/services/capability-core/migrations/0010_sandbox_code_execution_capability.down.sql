-- Reverses 0010_sandbox_code_execution_capability.up.sql.
--
-- capability_scopes, capability_health, capability_versions, and skill_packages
-- carry foreign keys onto capabilities(id), so any rows an operator or health
-- authority attached to cap.command.sandbox are removed first. Scoping every
-- delete to this one capability id keeps cap.command.shell and the rest of the
-- 0008 execution-dispatch catalog untouched.

DELETE FROM capability_scopes WHERE capability_id = 'cap.command.sandbox';
DELETE FROM capability_health WHERE capability_id = 'cap.command.sandbox';
DELETE FROM capability_versions WHERE capability_id = 'cap.command.sandbox';
DELETE FROM skill_packages WHERE capability_id = 'cap.command.sandbox';

DELETE FROM capabilities
WHERE id = 'cap.command.sandbox'
  AND created_by = 'migration:0010_sandbox_code_execution_capability';
