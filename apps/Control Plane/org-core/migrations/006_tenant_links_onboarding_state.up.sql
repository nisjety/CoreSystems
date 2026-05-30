-- Migration: 006_tenant_links_onboarding_state.up.sql
-- Phase 1: tenant resolution + onboarding state readiness

-- Extend organization profile defaults for enterprise bootstrap
ALTER TABLE organizations
  ADD COLUMN IF NOT EXISTS primary_domain TEXT,
  ADD COLUMN IF NOT EXISTS region TEXT NOT NULL DEFAULT 'eu',
  ADD COLUMN IF NOT EXISTS default_locale TEXT NOT NULL DEFAULT 'nb-NO';

CREATE INDEX IF NOT EXISTS idx_organizations_primary_domain
  ON organizations(primary_domain)
  WHERE primary_domain IS NOT NULL;

-- Deterministic tenant mapping
CREATE TABLE IF NOT EXISTS org_tenant_links (
  id TEXT PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
  org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  provider TEXT NOT NULL DEFAULT 'microsoft',
  microsoft_tenant_id TEXT NOT NULL,
  verified BOOLEAN NOT NULL DEFAULT false,
  domains JSONB NOT NULL DEFAULT '[]'::jsonb,
  display_name_from_tenant TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE(provider, microsoft_tenant_id),
  UNIQUE(org_id, provider)
);

CREATE INDEX IF NOT EXISTS idx_org_tenant_links_org_id
  ON org_tenant_links(org_id);

-- Onboarding state machine per org
CREATE TABLE IF NOT EXISTS org_onboarding_states (
  org_id TEXT PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
  status TEXT NOT NULL DEFAULT 'CREATED', -- CREATED|PROFILE_READY|CONNECTORS_PENDING|COMPLETED
  steps JSONB NOT NULL DEFAULT '{}'::jsonb,
  last_updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_org_onboarding_states_status
  ON org_onboarding_states(status);
