-- Reverses 0021_skill_bundles.up.sql.
--
-- Dropping the tables loses every uploaded skill bundle. Turn SKILL_BUNDLES_ENABLED off
-- first: with it off no route or RPC reads these tables, so nothing changes at the drop.
-- Blobs go with their org's tables, and the audit rows in registry_audit_log remain.

DROP TABLE IF EXISTS skill_bundle_blobs;
DROP TABLE IF EXISTS skill_bundle_files;
DROP TABLE IF EXISTS skill_bundles;
