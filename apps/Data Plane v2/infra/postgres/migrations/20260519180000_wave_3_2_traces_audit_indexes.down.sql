DROP INDEX IF EXISTS idx_admin_audit_log_org;
DROP INDEX IF EXISTS idx_admin_audit_log_action;
DROP INDEX IF EXISTS idx_admin_audit_log_actor;
DROP INDEX IF EXISTS idx_admin_audit_log_time;
DROP TABLE IF EXISTS admin_audit_log;

ALTER TABLE retrieval_runs DROP COLUMN IF EXISTS zdr_actions_applied;

DROP INDEX IF EXISTS idx_retrieval_runs_mode_mix_gin;
DROP INDEX IF EXISTS idx_retrieval_runs_filters_gin;
DROP INDEX IF EXISTS idx_documents_metadata_gin;
DROP INDEX IF EXISTS idx_documents_extraction_trace_gin;
