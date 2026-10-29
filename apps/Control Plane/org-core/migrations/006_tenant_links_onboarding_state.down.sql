-- Migration: 006_tenant_links_onboarding_state.down.sql

DROP INDEX IF EXISTS idx_org_onboarding_states_status;
DROP TABLE IF EXISTS org_onboarding_states;

DROP INDEX IF EXISTS idx_org_tenant_links_org_id;
DROP TABLE IF EXISTS org_tenant_links;

DROP INDEX IF EXISTS idx_organizations_primary_domain;

ALTER TABLE organizations
  DROP COLUMN IF EXISTS default_locale,
  DROP COLUMN IF EXISTS region,
  DROP COLUMN IF EXISTS primary_domain;
