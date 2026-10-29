DROP TRIGGER IF EXISTS quickwit_admin_audit_immutable ON quickwit_admin_job_audit;
DROP FUNCTION IF EXISTS reject_quickwit_admin_audit_mutation();
DROP TABLE IF EXISTS quickwit_admin_job_audit;
DROP TRIGGER IF EXISTS quickwit_admin_job_request_immutable ON quickwit_admin_jobs;
DROP FUNCTION IF EXISTS protect_quickwit_admin_job_request();
DROP TABLE IF EXISTS quickwit_admin_jobs;
