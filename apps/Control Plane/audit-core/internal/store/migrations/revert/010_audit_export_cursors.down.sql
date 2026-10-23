-- Revert of 010_audit_export_cursors.sql. Run by hand (the service applies
-- up-migrations only), after the SIEM exporter is stopped. Only the exporter's
-- bookkeeping is lost: a re-enabled destination would start again from the
-- oldest retained row and the receiver de-duplicates on (org_id, chain_seq).
-- Also delete the schema_migrations row so a later re-apply is possible:
--   DELETE FROM schema_migrations WHERE version = '010_audit_export_cursors';

DROP TABLE IF EXISTS audit_export_cursors;
