DROP TRIGGER IF EXISTS reject_auth_tombstoned_organization_insert_trigger
  ON organizations;
DROP FUNCTION IF EXISTS reject_auth_tombstoned_organization_insert();

ALTER TABLE auth_organization_tombstones
  DROP CONSTRAINT IF EXISTS auth_organization_tombstone_erasure_checkpoint_check,
  DROP CONSTRAINT IF EXISTS auth_organization_tombstone_revision_safe,
  DROP COLUMN IF EXISTS deletion_receipt,
  DROP COLUMN IF EXISTS erasure_completed_at,
  DROP COLUMN IF EXISTS revision;
