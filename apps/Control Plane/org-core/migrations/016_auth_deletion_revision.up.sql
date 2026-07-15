-- Bind the permanent Auth deletion tombstone to one canonical aggregate
-- revision. The completion checkpoint distinguishes an exact completed retry
-- (a no-op) from a tombstone whose dependent erasure must still be resumed.
ALTER TABLE auth_organization_tombstones
  ADD COLUMN IF NOT EXISTS revision BIGINT,
  ADD COLUMN IF NOT EXISTS erasure_completed_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS deletion_receipt JSONB;

-- Existing tombstones predate revisioned deletion. Derive the only safe retry
-- revision from the last durable Auth organization projection. When no
-- projection evidence exists, revision 1 is a conservative permanent fence.
DO $deletion_revision_preflight$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM auth_organization_tombstones tombstone
    JOIN auth_organization_projection_versions projection
      ON projection.org_id = tombstone.org_id
    WHERE tombstone.revision IS NULL
      AND projection.revision >= 9007199254740991
  ) THEN
    RAISE EXCEPTION
      'historical organization tombstone cannot advance beyond the safe revision range';
  END IF;
END;
$deletion_revision_preflight$;

UPDATE auth_organization_tombstones tombstone
SET revision = COALESCE((
  SELECT projection.revision + 1
  FROM auth_organization_projection_versions projection
  WHERE projection.org_id = tombstone.org_id
), 1)
WHERE tombstone.revision IS NULL;

ALTER TABLE auth_organization_tombstones
  ALTER COLUMN revision SET NOT NULL;

ALTER TABLE auth_organization_tombstones
  ADD CONSTRAINT auth_organization_tombstone_revision_safe
    CHECK (revision > 0 AND revision <= 9007199254740991) NOT VALID,
  ADD CONSTRAINT auth_organization_tombstone_erasure_checkpoint_check CHECK (
    (erasure_completed_at IS NULL AND deletion_receipt IS NULL) OR
    (
      erasure_completed_at IS NOT NULL AND
      deletion_receipt IS NOT NULL AND
      deletion_receipt @> '{"success": true}'::JSONB
    )
  ) NOT VALID;

ALTER TABLE auth_organization_tombstones
  VALIDATE CONSTRAINT auth_organization_tombstone_revision_safe;
ALTER TABLE auth_organization_tombstones
  VALIDATE CONSTRAINT auth_organization_tombstone_erasure_checkpoint_check;

-- Migration 013 granted uniform CRUD privileges to every scoped table. The
-- runtime may create a tombstone and write its erasure checkpoint, but its
-- identity, revision, and row lifetime are immutable.
REVOKE INSERT, UPDATE, DELETE ON auth_organization_tombstones FROM org_core_app;
GRANT INSERT (org_id, revision)
  ON auth_organization_tombstones TO org_core_app;
GRANT UPDATE (erasure_completed_at, deletion_receipt)
  ON auth_organization_tombstones TO org_core_app;

-- Repository checks remain the normal error-reporting path. This database
-- fence also prevents an operator/import path from reusing a permanently
-- deleted Auth organization id.
CREATE OR REPLACE FUNCTION reject_auth_tombstoned_organization_insert()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, public
AS $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM public.auth_organization_tombstones tombstone
    WHERE tombstone.org_id = NEW.id
  ) THEN
    RAISE EXCEPTION 'organization id is permanently tombstoned'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;

REVOKE ALL ON FUNCTION reject_auth_tombstoned_organization_insert() FROM PUBLIC;

DROP TRIGGER IF EXISTS reject_auth_tombstoned_organization_insert_trigger
  ON organizations;
CREATE TRIGGER reject_auth_tombstoned_organization_insert_trigger
BEFORE INSERT ON organizations
FOR EACH ROW EXECUTE FUNCTION reject_auth_tombstoned_organization_insert();
