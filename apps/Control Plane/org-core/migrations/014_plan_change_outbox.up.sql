ALTER TABLE organizations
  ADD COLUMN IF NOT EXISTS plan_revision BIGINT NOT NULL DEFAULT 0;

DO $constraint$
BEGIN
  IF NOT EXISTS (
    SELECT 1 FROM pg_constraint
    WHERE conname = 'organizations_plan_revision_nonnegative'
      AND conrelid = 'organizations'::regclass
  ) THEN
    ALTER TABLE organizations
      ADD CONSTRAINT organizations_plan_revision_nonnegative
      CHECK (plan_revision >= 0);
  END IF;
END
$constraint$;

CREATE TABLE IF NOT EXISTS organization_plan_change_outbox (
  org_id TEXT NOT NULL,
  revision BIGINT NOT NULL,
  organization_name TEXT NOT NULL,
  previous_plan TEXT NOT NULL,
  new_plan TEXT NOT NULL,
  changed_by TEXT NOT NULL DEFAULT '',
  change_reason TEXT NOT NULL DEFAULT '',
  attempts INTEGER NOT NULL DEFAULT 0,
  processing_at TIMESTAMPTZ,
  published_at TIMESTAMPTZ,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (org_id, revision),
  CONSTRAINT organization_plan_change_outbox_revision_positive
    CHECK (revision > 0)
);

CREATE INDEX IF NOT EXISTS idx_organization_plan_change_outbox_pending
  ON organization_plan_change_outbox (created_at, org_id, revision)
  WHERE published_at IS NULL;

GRANT SELECT, INSERT, UPDATE, DELETE
  ON organization_plan_change_outbox TO org_core_app;

ALTER TABLE organization_plan_change_outbox ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_plan_change_outbox_rls_isolation
  ON organization_plan_change_outbox;
CREATE POLICY organization_plan_change_outbox_rls_isolation
  ON organization_plan_change_outbox
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
