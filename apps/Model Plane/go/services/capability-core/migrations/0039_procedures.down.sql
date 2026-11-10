-- Reverses 0039_procedures.up.sql.
--
-- DESTROYS every saved recipe: all procedures, all of their immutable versions and all tags. The
-- immutability trigger guards row deletes only, so dropping the tables is not blocked by it.
-- Turn PROCEDURES_API off and roll capability-core back to a build that does not read these tables
-- first; with the switch off nothing writes them.

DROP TRIGGER IF EXISTS procedure_versions_immutable_trg ON procedure_versions;
DROP FUNCTION IF EXISTS procedure_versions_immutable_fn();
DROP TABLE IF EXISTS procedure_tags;
DROP TABLE IF EXISTS procedure_versions;
DROP INDEX IF EXISTS procedures_owner_idx;
DROP TABLE IF EXISTS procedures;
