CREATE TABLE IF NOT EXISTS organization_interactive_retention_outbox (
  event_id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  processing_at TIMESTAMPTZ,
  published_at TIMESTAMPTZ,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT organization_interactive_retention_outbox_org_id_nonempty
    CHECK (btrim(org_id) <> '')
);

CREATE INDEX IF NOT EXISTS idx_organization_interactive_retention_outbox_pending
  ON organization_interactive_retention_outbox (created_at, event_id)
  WHERE published_at IS NULL;

GRANT SELECT, INSERT, UPDATE, DELETE
  ON organization_interactive_retention_outbox TO org_core_app;
GRANT USAGE, SELECT
  ON SEQUENCE organization_interactive_retention_outbox_event_id_seq TO org_core_app;

ALTER TABLE organization_interactive_retention_outbox ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_interactive_retention_outbox_rls_isolation
  ON organization_interactive_retention_outbox;
CREATE POLICY organization_interactive_retention_outbox_rls_isolation
  ON organization_interactive_retention_outbox
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
