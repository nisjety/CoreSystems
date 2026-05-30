-- Migration: 005_enterprise_identity_memberships.up.sql
-- Phase 1: enterprise identity readiness for zero-input onboarding

-- Extend provider_accounts with enterprise identity synchronization fields
ALTER TABLE provider_accounts
  ADD COLUMN IF NOT EXISTS microsoft_tenant_id TEXT,
  ADD COLUMN IF NOT EXISTS email_from_provider TEXT,
  ADD COLUMN IF NOT EXISTS scopes_granted JSONB NOT NULL DEFAULT '[]'::jsonb,
  ADD COLUMN IF NOT EXISTS token_ref TEXT,
  ADD COLUMN IF NOT EXISTS last_synced_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_provider_accounts_microsoft_tenant_id
  ON provider_accounts(microsoft_tenant_id);

CREATE INDEX IF NOT EXISTS idx_provider_accounts_token_ref
  ON provider_accounts(token_ref)
  WHERE token_ref IS NOT NULL;

-- User-org memberships in user-core domain (idempotent/lookup friendly)
CREATE TABLE IF NOT EXISTS user_org_memberships (
  id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  org_id TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'member', -- owner|admin|member|viewer
  status TEXT NOT NULL DEFAULT 'active', -- active|invited|pending|suspended
  invited_by TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(user_id, org_id)
);

CREATE INDEX IF NOT EXISTS idx_user_org_memberships_user_id
  ON user_org_memberships(user_id);

CREATE INDEX IF NOT EXISTS idx_user_org_memberships_org_id
  ON user_org_memberships(org_id);

CREATE INDEX IF NOT EXISTS idx_user_org_memberships_status
  ON user_org_memberships(status);
