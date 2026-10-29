-- Rollback for 20260508170000_add_access_audit_log.sql
DROP INDEX IF EXISTS idx_access_audit_cause;
DROP INDEX IF EXISTS idx_access_audit_user_created;
DROP INDEX IF EXISTS idx_access_audit_org_created;
DROP TABLE IF EXISTS access_audit_log;
