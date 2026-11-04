-- WS7.H: the organisation's web policy of record (outcome C of ADR-1).
--
-- org-core stores the org layer of Quarry's tighten-only web policy: per-capability
-- modes, the robots and proxy ceilings and the domain rules. Auth Core reads it at
-- token mint (WS7.I) and folds it into the web_policy claim; Quarry stays the
-- enforcer. Nothing here enforces anything by itself, and nothing reads it until
-- WS7.I ships.
--
-- The document holds capability keys, modes, ceilings and host names: no URL, no
-- page content, no credential. It is organisation configuration and goes with the
-- organisation (ON DELETE CASCADE is the erasure path, as in 003 and 023).
--
-- A missing row is revision 0 and the empty document. Rows are never deleted by the
-- runtime role, so a revision cannot reset and an If-Match token cannot be reused.
CREATE TABLE IF NOT EXISTS organization_web_policy (
  org_id     TEXT        PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
  policy     JSONB       NOT NULL CHECK (jsonb_typeof(policy) = 'object'),
  revision   BIGINT      NOT NULL CHECK (revision > 0),
  rev_digest TEXT        NOT NULL CHECK (rev_digest ~ '^[0-9a-f]{12}$'),
  updated_by TEXT        NOT NULL CHECK (btrim(updated_by) <> ''),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Migration 009 default-grants all four privileges on every later table, whatever a
-- migration's own GRANT says (see 024), so state the effective set explicitly.
REVOKE ALL ON organization_web_policy FROM org_core_app;
GRANT SELECT, INSERT, UPDATE ON organization_web_policy TO org_core_app;

ALTER TABLE organization_web_policy ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_web_policy_rls_isolation ON organization_web_policy;
CREATE POLICY organization_web_policy_rls_isolation
  ON organization_web_policy
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
