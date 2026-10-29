-- First-party inquiries remain in the existing person-data boundary. No answers
-- are copied into company lists, analytics, audit events or company exports.
ALTER TABLE provider_leads ADD COLUMN IF NOT EXISTS website_id TEXT;
ALTER TABLE provider_leads ADD COLUMN IF NOT EXISTS website_revision BIGINT;
ALTER TABLE provider_leads ADD COLUMN IF NOT EXISTS website_site TEXT;
ALTER TABLE provider_leads ADD COLUMN IF NOT EXISTS website_environment TEXT;
ALTER TABLE provider_leads ADD COLUMN IF NOT EXISTS submission_digest BYTEA;
CREATE INDEX IF NOT EXISTS idx_provider_leads_website_outcomes
    ON provider_leads(org_id,website_id,website_revision,website_environment,created_at)
    WHERE provider_key = 'website_inquiry';
