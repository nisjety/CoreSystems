-- Flow C: 30-day org-deletion ledger + reminder tracking.
--
-- org_deletion_members records, per active member captured at soft-delete
-- time, the GDPR export/acknowledge checkpoints during the 30-day grace
-- window opened by DELETE /orgs/:id/gdpr/soft-delete. Restore (POST
-- /orgs/:id/gdpr/restore) deletes all ledger rows for the org wholesale.
--
-- ON DELETE CASCADE mirrors organization_members' FK: gdpr_hard_delete_organization()
-- (migrations/003_gdpr_hard_delete.up.sql) never lists organization_members or
-- this table explicitly — cleanup happens via cascade when the organizations
-- row itself is deleted, so no proc change is required here.
CREATE TABLE IF NOT EXISTS org_deletion_members (
  org_id          TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  user_id         TEXT NOT NULL,
  notified_at     TIMESTAMPTZ,
  exported_at     TIMESTAMPTZ,
  acknowledged_at TIMESTAMPTZ,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (org_id, user_id)
);

-- Supports a future "your orgs pending deletion" lookup keyed by user_id
-- (notification-core / user-core), mirroring organization_members' user_id index.
CREATE INDEX IF NOT EXISTS idx_org_deletion_members_user_id
  ON org_deletion_members (user_id);

-- The two organizations columns record whether the 7-day/1-day
-- velion.org.deletion.reminder events have already fired, so the cron sweep
-- never double-sends a reminder for the same org.
ALTER TABLE organizations
  ADD COLUMN IF NOT EXISTS deletion_reminder_7d_sent_at TIMESTAMPTZ NULL,
  ADD COLUMN IF NOT EXISTS deletion_reminder_1d_sent_at TIMESTAMPTZ NULL;

-- Partial indexes so ListOrgsNeeding7DayReminder / ListOrgsNeeding1DayReminder
-- find "pending deletion, not yet reminded" organizations without a
-- sequential scan of the whole table.
CREATE INDEX IF NOT EXISTS idx_organizations_deletion_reminder_7d_pending
  ON organizations (deleted_at)
  WHERE deleted_at IS NOT NULL AND deletion_reminder_7d_sent_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_organizations_deletion_reminder_1d_pending
  ON organizations (deleted_at)
  WHERE deleted_at IS NOT NULL AND deletion_reminder_1d_sent_at IS NULL;

-- org_deletion_members is a mutable ledger (not an audit outbox), so — unlike
-- migration 017's organization_gdpr_audit_outbox — org_core_app needs full
-- CRUD, following migration 014's organization_plan_change_outbox precedent.
GRANT SELECT, INSERT, UPDATE, DELETE ON org_deletion_members TO org_core_app;

ALTER TABLE org_deletion_members ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_deletion_members_rls_isolation ON org_deletion_members;
CREATE POLICY org_deletion_members_rls_isolation
  ON org_deletion_members
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
