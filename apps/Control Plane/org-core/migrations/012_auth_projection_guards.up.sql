CREATE TABLE IF NOT EXISTS auth_organization_tombstones (
  org_id TEXT PRIMARY KEY,
  deleted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  deletion_reason TEXT NOT NULL DEFAULT 'auth_canonical_delete'
);

CREATE TABLE IF NOT EXISTS auth_membership_projection_versions (
  org_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  revision BIGINT NOT NULL,
  desired_action TEXT NOT NULL,
  applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (org_id, user_id),
  CONSTRAINT auth_membership_projection_action_check
    CHECK (desired_action IN ('upsert', 'remove'))
);

CREATE TABLE IF NOT EXISTS auth_organization_projection_versions (
  org_id TEXT PRIMARY KEY,
  revision BIGINT NOT NULL,
  applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT auth_organization_projection_revision_positive
    CHECK (revision > 0)
);

GRANT SELECT, INSERT, UPDATE ON auth_organization_tombstones TO org_core_app;
GRANT SELECT, INSERT, UPDATE ON auth_membership_projection_versions TO org_core_app;
GRANT SELECT, INSERT, UPDATE ON auth_organization_projection_versions TO org_core_app;

ALTER TABLE auth_organization_tombstones ENABLE ROW LEVEL SECURITY;
ALTER TABLE auth_membership_projection_versions ENABLE ROW LEVEL SECURITY;
ALTER TABLE auth_organization_projection_versions ENABLE ROW LEVEL SECURITY;

DROP POLICY IF EXISTS auth_organization_tombstones_scope ON auth_organization_tombstones;
CREATE POLICY auth_organization_tombstones_scope ON auth_organization_tombstones
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

DROP POLICY IF EXISTS auth_membership_projection_versions_scope ON auth_membership_projection_versions;
CREATE POLICY auth_membership_projection_versions_scope ON auth_membership_projection_versions
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

DROP POLICY IF EXISTS auth_organization_projection_versions_scope ON auth_organization_projection_versions;
CREATE POLICY auth_organization_projection_versions_scope ON auth_organization_projection_versions
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
