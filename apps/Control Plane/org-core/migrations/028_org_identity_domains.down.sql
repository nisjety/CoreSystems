-- Reverses 028. The identity audit outbox and the SSO policy hold no audit
-- content that is not also in audit-core once published; unpublished rows are
-- lost, so drain the outbox before rolling back.
DROP TABLE IF EXISTS organization_sso_policy;
DROP TABLE IF EXISTS organization_identity_audit_outbox;

ALTER TABLE organization_domains DROP CONSTRAINT IF EXISTS organization_domain_verified_method_check;
ALTER TABLE organization_domains DROP CONSTRAINT IF EXISTS organization_domain_method_check;

ALTER TABLE organization_domains
  DROP COLUMN IF EXISTS revoked_by,
  DROP COLUMN IF EXISTS revoked_at,
  DROP COLUMN IF EXISTS last_check_reason,
  DROP COLUMN IF EXISTS last_check_at,
  DROP COLUMN IF EXISTS verification_expires_at,
  DROP COLUMN IF EXISTS verification_token;
