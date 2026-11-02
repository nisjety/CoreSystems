-- WS7.H: append-only record of every web policy write. One row per revision: who
-- wrote it, when, the full document, and whether the change only tightened the
-- policy. The runtime role can insert and read and nothing else, so a history row
-- cannot be edited or removed (the cascade from organizations is the erasure path).
CREATE TABLE IF NOT EXISTS organization_web_policy_history (
  org_id     TEXT        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  revision   BIGINT      NOT NULL CHECK (revision > 0),
  policy     JSONB       NOT NULL CHECK (jsonb_typeof(policy) = 'object'),
  changed_by TEXT        NOT NULL CHECK (btrim(changed_by) <> ''),
  changed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  tightened  BOOLEAN     NOT NULL,
  PRIMARY KEY (org_id, revision)
);

REVOKE ALL ON organization_web_policy_history FROM org_core_app;
GRANT SELECT, INSERT ON organization_web_policy_history TO org_core_app;

ALTER TABLE organization_web_policy_history ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_web_policy_history_rls_isolation ON organization_web_policy_history;
CREATE POLICY organization_web_policy_history_rls_isolation
  ON organization_web_policy_history
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
