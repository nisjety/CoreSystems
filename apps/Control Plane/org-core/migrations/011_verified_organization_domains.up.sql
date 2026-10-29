CREATE TABLE IF NOT EXISTS organization_domains (
  org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  normalized_domain TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  verification_method TEXT,
  verified_at TIMESTAMPTZ,
  verified_by TEXT,
  auto_invite_enabled BOOLEAN NOT NULL DEFAULT false,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (org_id, normalized_domain),
  CONSTRAINT organization_domain_status_check
    CHECK (status IN ('pending', 'verified', 'revoked')),
  CONSTRAINT organization_domain_normalized_check
    CHECK (normalized_domain = LOWER(RTRIM(BTRIM(normalized_domain), '.')))
);

-- A company domain can route to at most one verified organization. Pending
-- claims may coexist while DNS/IdP proof is being resolved.
CREATE UNIQUE INDEX IF NOT EXISTS organization_domains_one_verified_owner
  ON organization_domains(normalized_domain)
  WHERE status = 'verified';

CREATE INDEX IF NOT EXISTS organization_domains_org_id
  ON organization_domains(org_id);

GRANT SELECT, INSERT, UPDATE, DELETE ON organization_domains TO org_core_app;
ALTER TABLE organization_domains ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_domains_rls_isolation ON organization_domains;
CREATE POLICY organization_domains_rls_isolation ON organization_domains
  USING (
    org_id = current_setting('app.current_org', true)
    OR COALESCE(current_setting('app.current_org', true), '') = ''
  )
  WITH CHECK (
    org_id = current_setting('app.current_org', true)
    OR COALESCE(current_setting('app.current_org', true), '') = ''
  );
