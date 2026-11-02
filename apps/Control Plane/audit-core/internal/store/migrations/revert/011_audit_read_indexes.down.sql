-- Revert of 011_audit_read_indexes.sql. Run by hand (the service applies
-- up-migrations only). Only read speed is lost; no data changes. Also delete
-- the schema_migrations row so a later re-apply is possible:
--   DELETE FROM schema_migrations WHERE version = '011_audit_read_indexes';

DROP INDEX IF EXISTS idx_audit_events_org_plane_ts;
DROP INDEX IF EXISTS idx_audit_events_org_user_ts;
DROP INDEX IF EXISTS idx_audit_events_org_ts_id;
