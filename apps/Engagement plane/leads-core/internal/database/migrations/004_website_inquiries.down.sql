DROP INDEX IF EXISTS idx_provider_leads_website_outcomes;
ALTER TABLE provider_leads DROP COLUMN IF EXISTS submission_digest;
ALTER TABLE provider_leads DROP COLUMN IF EXISTS website_environment;
ALTER TABLE provider_leads DROP COLUMN IF EXISTS website_site;
ALTER TABLE provider_leads DROP COLUMN IF EXISTS website_revision;
ALTER TABLE provider_leads DROP COLUMN IF EXISTS website_id;
