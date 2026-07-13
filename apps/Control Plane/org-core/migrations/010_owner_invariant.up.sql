-- Every live organization must have at least one active owner. The checks are
-- deferred until COMMIT so organization + first owner can be inserted in the
-- same transaction while every partial transaction is rejected.

CREATE OR REPLACE FUNCTION assert_live_organization_has_owner(target_org_id TEXT)
RETURNS VOID
LANGUAGE plpgsql
AS $$
BEGIN
  -- Serialize owner changes per organization to prevent two concurrent owner
  -- removals from each observing the other's still-uncommitted owner.
  PERFORM 1 FROM organizations WHERE id = target_org_id FOR UPDATE;

  IF EXISTS (
    SELECT 1
    FROM organizations o
    WHERE o.id = target_org_id
      AND o.deleted_at IS NULL
      AND o.status <> 'deleted'
  ) AND NOT EXISTS (
    SELECT 1
    FROM organization_members m
    WHERE m.org_id = target_org_id
      AND m.status = 'active'
      AND 'owner' = ANY(string_to_array(m.role, ','))
  ) THEN
    RAISE EXCEPTION 'live organization % must have an active owner', target_org_id
      USING ERRCODE = '23514';
  END IF;

END;
$$;

CREATE OR REPLACE FUNCTION enforce_live_organization_owner()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_TABLE_NAME = 'organization_members' THEN
    IF TG_OP IN ('UPDATE', 'DELETE') THEN
      PERFORM assert_live_organization_has_owner(OLD.org_id);
    END IF;
    IF TG_OP IN ('INSERT', 'UPDATE') AND (TG_OP <> 'UPDATE' OR NEW.org_id <> OLD.org_id) THEN
      PERFORM assert_live_organization_has_owner(NEW.org_id);
    END IF;
  ELSE
    PERFORM assert_live_organization_has_owner(COALESCE(NEW.id, OLD.id));
  END IF;

  RETURN COALESCE(NEW, OLD);
END;
$$;

DROP TRIGGER IF EXISTS organizations_require_owner ON organizations;
CREATE CONSTRAINT TRIGGER organizations_require_owner
AFTER INSERT OR UPDATE OF status, deleted_at ON organizations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_live_organization_owner();

DROP TRIGGER IF EXISTS organization_members_preserve_owner ON organization_members;
CREATE CONSTRAINT TRIGGER organization_members_preserve_owner
AFTER INSERT OR UPDATE OR DELETE ON organization_members
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_live_organization_owner();

DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM organizations o
    WHERE o.deleted_at IS NULL
      AND o.status <> 'deleted'
      AND NOT EXISTS (
        SELECT 1 FROM organization_members m
        WHERE m.org_id = o.id AND m.status = 'active'
          AND 'owner' = ANY(string_to_array(m.role, ','))
      )
  ) THEN
    RAISE EXCEPTION 'existing live organizations without owners must be reconciled before migration';
  END IF;
END;
$$;
