-- Rollback Brønnøysund Register verification fields

DROP INDEX IF EXISTS idx_organizations_org_number;
DROP INDEX IF EXISTS idx_organizations_verification_status;

ALTER TABLE organizations
  DROP COLUMN IF EXISTS brreg_data,
  DROP COLUMN IF EXISTS verification_status,
  DROP COLUMN IF EXISTS org_number;
