-- Reverses 0015_background_process_capability.up.sql.
--
-- capability_scopes, capability_health, capability_versions, and skill_packages
-- carry foreign keys onto capabilities(id), so any rows an operator or health
-- authority attached to cap.process.background are removed first. Scoping every
-- delete to this one capability id keeps cap.command.sandbox (0010) and the rest
-- of the execution-dispatch catalog untouched — the process family is a
-- separate authority and reversing it must not disturb per-call sandboxing.
--
-- Rolling this back leaves any rows already in sandbox-manager's process
-- registry alone. That is correct: those rows are the record of processes that
-- actually ran, this migration never owned them, and the running host will
-- reconcile or expire them on its own terms.

DELETE FROM capability_scopes WHERE capability_id = 'cap.process.background';
DELETE FROM capability_health WHERE capability_id = 'cap.process.background';
DELETE FROM capability_versions WHERE capability_id = 'cap.process.background';
DELETE FROM skill_packages WHERE capability_id = 'cap.process.background';

DELETE FROM capabilities
WHERE id = 'cap.process.background'
  AND created_by = 'migration:0015_background_process_capability';
