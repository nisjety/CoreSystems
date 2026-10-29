-- Migration: 005_enterprise_identity_memberships.down.sql

DROP INDEX IF EXISTS idx_user_org_memberships_status;
DROP INDEX IF EXISTS idx_user_org_memberships_org_id;
DROP INDEX IF EXISTS idx_user_org_memberships_user_id;
DROP TABLE IF EXISTS user_org_memberships;

DROP INDEX IF EXISTS idx_provider_accounts_token_ref;
DROP INDEX IF EXISTS idx_provider_accounts_microsoft_tenant_id;

ALTER TABLE provider_accounts
  DROP COLUMN IF EXISTS last_synced_at,
  DROP COLUMN IF EXISTS token_ref,
  DROP COLUMN IF EXISTS scopes_granted,
  DROP COLUMN IF EXISTS email_from_provider,
  DROP COLUMN IF EXISTS microsoft_tenant_id;
