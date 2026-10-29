-- Add Brønnøysund Register verification fields to organizations

-- org_number: Norwegian organisasjonsnummer (9-digit string)
ALTER TABLE organizations
  ADD COLUMN IF NOT EXISTS org_number TEXT;

-- verification_status: 'unverified' | 'verified'
ALTER TABLE organizations
  ADD COLUMN IF NOT EXISTS verification_status TEXT NOT NULL DEFAULT 'unverified';

-- brreg_data: raw snapshot of the Enhetsregisteret entity at time of verification
ALTER TABLE organizations
  ADD COLUMN IF NOT EXISTS brreg_data JSONB;

CREATE INDEX IF NOT EXISTS idx_organizations_org_number
  ON organizations(org_number)
  WHERE org_number IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_organizations_verification_status
  ON organizations(verification_status);
