-- Reverses 0039_plugin_supply_chain.up.sql.
--
-- DESTROYS every component list, kill-list entry, source pin, manifest digest, install record and
-- catalogue approval.
-- Turn PLUGIN_LOADER_ENABLED off and roll capability-core back to a build that does not read these
-- columns before running this file.

DROP TABLE IF EXISTS plugin_components;
DROP TABLE IF EXISTS plugin_kill_list;

ALTER TABLE plugin_packages DROP CONSTRAINT IF EXISTS plugin_packages_source_check;
ALTER TABLE plugin_packages
    DROP COLUMN IF EXISTS catalog_approved,
    DROP COLUMN IF EXISTS installed_at,
    DROP COLUMN IF EXISTS manifest_digest,
    DROP COLUMN IF EXISTS source_pin,
    DROP COLUMN IF EXISTS source_ref,
    DROP COLUMN IF EXISTS source_kind;
