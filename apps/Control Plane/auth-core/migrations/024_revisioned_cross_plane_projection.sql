-- Add a durable cross-plane publication checkpoint without changing the
-- checksum-ledgered organization outbox migration. Historical deletion rows
-- receive a conservative revision and are republished during reconciliation.
ALTER TABLE organization_deletion_outbox
  ADD COLUMN IF NOT EXISTS revision BIGINT,
  ADD COLUMN IF NOT EXISTS event_synced_at TIMESTAMPTZ;

UPDATE organization_deletion_outbox deletion
SET revision = COALESCE(
  (
    SELECT projection.revision + 1
    FROM organization_projection_outbox projection
    WHERE projection.organization_id = deletion.organization_id
  ),
  1
)
WHERE deletion.revision IS NULL;

ALTER TABLE organization_deletion_outbox
  ALTER COLUMN revision SET NOT NULL;

-- Completed legacy rows have never emitted the canonical revisioned removal.
-- Reopen only that checkpoint; the existing billing/org timestamps prevent
-- those already-completed sinks from running again.
UPDATE organization_deletion_outbox
SET completed_at = NULL,
    processing_at = NULL,
    last_error = NULL,
    updated_at = NOW()
WHERE completed_at IS NOT NULL
  AND event_synced_at IS NULL;

ALTER TABLE organization_deletion_outbox
  ADD CONSTRAINT organization_deletion_outbox_revision_positive
  CHECK (revision > 0);

CREATE OR REPLACE FUNCTION enqueue_organization_deletion()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
  deletion_revision BIGINT;
BEGIN
  SELECT revision + 1
  INTO deletion_revision
  FROM organization_projection_outbox
  WHERE organization_id = OLD.id;

  IF deletion_revision IS NULL THEN
    RAISE EXCEPTION 'organization projection revision is unavailable for deletion';
  END IF;

  INSERT INTO organization_deletion_outbox (organization_id, name, revision)
  VALUES (OLD.id, OLD.name, deletion_revision)
  ON CONFLICT (organization_id) DO UPDATE SET
    name = EXCLUDED.name,
    revision = GREATEST(
      organization_deletion_outbox.revision,
      EXCLUDED.revision
    ),
    billing_synced_at = NULL,
    org_synced_at = NULL,
    event_synced_at = NULL,
    completed_at = NULL,
    processing_at = NULL,
    last_error = NULL,
    updated_at = NOW();
  RETURN OLD;
END;
$$;

CREATE OR REPLACE FUNCTION reject_deleted_organization_id_reuse()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM organization_deletion_outbox deletion
    WHERE deletion.organization_id = NEW.id
  ) THEN
    RAISE EXCEPTION 'deleted organization identifiers cannot be reused';
  END IF;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS organization_id_reuse_guard ON organization;
CREATE TRIGGER organization_id_reuse_guard
BEFORE INSERT ON organization
FOR EACH ROW EXECUTE FUNCTION reject_deleted_organization_id_reuse();
