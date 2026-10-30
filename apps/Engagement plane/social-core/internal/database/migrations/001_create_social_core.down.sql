DROP INDEX IF EXISTS social_audit_events_org_created_idx;
DROP TABLE IF EXISTS social_audit_events;

DROP INDEX IF EXISTS social_publish_attempts_org_provider_idx;
DROP INDEX IF EXISTS social_publish_attempts_job_idx;
DROP TABLE IF EXISTS social_publish_attempts;

DROP INDEX IF EXISTS social_publish_jobs_post_idx;
DROP INDEX IF EXISTS social_publish_jobs_queue_idx;
DROP INDEX IF EXISTS social_publish_jobs_org_idempotency_unique;
DROP TABLE IF EXISTS social_publish_jobs;

DROP INDEX IF EXISTS social_posts_platforms_gin_idx;
DROP INDEX IF EXISTS social_posts_org_updated_idx;
DROP INDEX IF EXISTS social_posts_org_status_scheduled_idx;
DROP INDEX IF EXISTS social_approvals_org_campaign_idx;
DROP INDEX IF EXISTS social_approvals_org_state_idx;
DROP INDEX IF EXISTS social_approvals_org_post_pending_unique;
DROP TABLE IF EXISTS social_approvals;
DROP TABLE IF EXISTS social_posts;

DROP INDEX IF EXISTS social_campaigns_org_status_idx;
DROP TABLE IF EXISTS social_campaigns;

DROP INDEX IF EXISTS social_accounts_org_provider_idx;
DROP INDEX IF EXISTS social_accounts_org_provider_connection_unique;
DROP TABLE IF EXISTS social_accounts;
