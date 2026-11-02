-- Rollback of the organisation's website column.
ALTER TABLE organizations
  DROP COLUMN IF EXISTS website_updated_at,
  DROP COLUMN IF EXISTS website_url;
