DROP TABLE IF EXISTS org_deletion_members;

DROP INDEX IF EXISTS idx_organizations_deletion_reminder_7d_pending;
DROP INDEX IF EXISTS idx_organizations_deletion_reminder_1d_pending;

ALTER TABLE organizations
  DROP COLUMN IF EXISTS deletion_reminder_7d_sent_at,
  DROP COLUMN IF EXISTS deletion_reminder_1d_sent_at;
