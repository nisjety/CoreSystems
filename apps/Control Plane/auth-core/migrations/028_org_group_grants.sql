-- D-A: an "account" grouping axis above org (docs/sovereign-rag-phased-plan.md,
-- Data Plane v2). Named org_group here, not "account" — Better Auth's own
-- `account` table (per-user OAuth-provider rows) already exists in this schema
-- and means something unrelated; this is the same class of collision "tenant"
-- had with `microsoft_tenant_id`, caught before it repeated.
--
-- An org_group is anchored to one host organization. The plan's "account (its
-- admin principal)" is that host org's own existing owner/admin members —
-- reusing `member`'s role check rather than a second membership system.
CREATE TABLE IF NOT EXISTS org_group (
  id UUID PRIMARY KEY,
  name TEXT NOT NULL,
  host_organization_id TEXT NOT NULL REFERENCES organization(id) ON DELETE CASCADE,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  -- One account per host org for now; a host wanting a second, separate
  -- grouping is a future extension, not something this feature needs today.
  CONSTRAINT org_group_host_unique UNIQUE (host_organization_id)
);

-- Two independent per-org opt-in grants toward one org_group. A row's
-- existence represents an active grant of some kind — revoking the last of
-- the two should delete the row, not leave both flags false, so an
-- org's grant status is a single query ("does a row exist") rather than
-- "does a row exist AND is at least one flag true."
CREATE TABLE IF NOT EXISTS org_group_grant (
  id UUID PRIMARY KEY,
  org_group_id UUID NOT NULL REFERENCES org_group(id) ON DELETE CASCADE,
  organization_id TEXT NOT NULL REFERENCES organization(id) ON DELETE CASCADE,
  data_access BOOLEAN NOT NULL DEFAULT FALSE,
  billing_consolidation BOOLEAN NOT NULL DEFAULT FALSE,
  granted_by TEXT NOT NULL REFERENCES "user"(id),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT org_group_grant_unique UNIQUE (org_group_id, organization_id),
  CONSTRAINT org_group_grant_has_a_grant CHECK (data_access OR billing_consolidation)
);

-- The decision endpoint's hot-path lookup is "does org X have an active
-- data-access grant" — by organization_id, not by org_group_id.
CREATE INDEX IF NOT EXISTS org_group_grant_organization_id_idx
  ON org_group_grant(organization_id)
  WHERE data_access;
