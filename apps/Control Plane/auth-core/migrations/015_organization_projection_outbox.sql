CREATE TABLE IF NOT EXISTS organization_projection_outbox (
  organization_id TEXT PRIMARY KEY REFERENCES organization(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  slug TEXT,
  metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
  owner_user_id TEXT,
  revision BIGINT NOT NULL DEFAULT 1,
  published_at TIMESTAMPTZ,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  processing_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS organization_projection_outbox_pending
  ON organization_projection_outbox(created_at)
  WHERE published_at IS NULL AND owner_user_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS organization_membership_outbox (
  -- Deliberately not an FK: org deletion cascades canonical members first and
  -- their AFTER DELETE triggers must still be able to persist removal intent.
  organization_id TEXT NOT NULL,
  -- Deliberately not an FK: a user deletion must not erase the durable
  -- membership-removal intent before Org Core consumes it.
  user_id TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'member',
  desired_action TEXT NOT NULL,
  revision BIGINT NOT NULL DEFAULT 1,
  synced_at TIMESTAMPTZ,
  processing_at TIMESTAMPTZ,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (organization_id, user_id),
  CONSTRAINT organization_membership_outbox_action_check
    CHECK (desired_action IN ('upsert', 'remove'))
);

CREATE INDEX IF NOT EXISTS organization_membership_outbox_pending
  ON organization_membership_outbox(updated_at)
  WHERE synced_at IS NULL;

CREATE TABLE IF NOT EXISTS organization_deletion_outbox (
  organization_id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  billing_synced_at TIMESTAMPTZ,
  org_synced_at TIMESTAMPTZ,
  completed_at TIMESTAMPTZ,
  processing_at TIMESTAMPTZ,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS organization_deletion_outbox_pending
  ON organization_deletion_outbox(created_at)
  WHERE completed_at IS NULL;

CREATE OR REPLACE FUNCTION enqueue_organization_projection()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  INSERT INTO organization_projection_outbox
    (organization_id, name, slug, metadata)
  VALUES (
    NEW.id,
    NEW.name,
    NEW.slug,
    CASE
      WHEN NEW.metadata IS NULL OR BTRIM(NEW.metadata) = '' THEN '{}'::jsonb
      ELSE NEW.metadata::jsonb
    END
  )
  ON CONFLICT (organization_id) DO UPDATE SET
    name = EXCLUDED.name,
    slug = EXCLUDED.slug,
    metadata = EXCLUDED.metadata,
    revision = organization_projection_outbox.revision + 1,
    published_at = NULL,
    processing_at = NULL,
    last_error = NULL,
    updated_at = NOW();
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS organization_projection_outbox_trigger ON organization;
CREATE TRIGGER organization_projection_outbox_trigger
AFTER INSERT OR UPDATE OF name, slug, metadata ON organization
FOR EACH ROW EXECUTE FUNCTION enqueue_organization_projection();

CREATE OR REPLACE FUNCTION enqueue_organization_membership_projection()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
  target_org_id TEXT;
  target_user_id TEXT;
  target_role TEXT;
  target_action TEXT;
BEGIN
  IF TG_OP = 'DELETE' THEN
    target_org_id := OLD.organization_id;
    target_user_id := OLD.user_id;
    target_role := OLD.role;
    target_action := 'remove';
  ELSE
    target_org_id := NEW.organization_id;
    target_user_id := NEW.user_id;
    target_role := NEW.role;
    target_action := 'upsert';
  END IF;

  INSERT INTO organization_membership_outbox
    (organization_id, user_id, role, desired_action)
  VALUES (target_org_id, target_user_id, target_role, target_action)
  ON CONFLICT (organization_id, user_id) DO UPDATE SET
    role = EXCLUDED.role,
    desired_action = EXCLUDED.desired_action,
    revision = organization_membership_outbox.revision + 1,
    synced_at = NULL,
    processing_at = NULL,
    last_error = NULL,
    updated_at = NOW();

  RETURN COALESCE(NEW, OLD);
END;
$$;

DROP TRIGGER IF EXISTS member_projection_outbox_trigger ON member;
CREATE TRIGGER member_projection_outbox_trigger
AFTER INSERT OR UPDATE OF organization_id, user_id, role OR DELETE ON member
FOR EACH ROW EXECUTE FUNCTION enqueue_organization_membership_projection();

CREATE OR REPLACE FUNCTION enqueue_organization_deletion()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  INSERT INTO organization_deletion_outbox (organization_id, name)
  VALUES (OLD.id, OLD.name)
  ON CONFLICT (organization_id) DO UPDATE SET
    name = EXCLUDED.name,
    billing_synced_at = NULL,
    org_synced_at = NULL,
    completed_at = NULL,
    processing_at = NULL,
    last_error = NULL,
    updated_at = NOW();
  RETURN OLD;
END;
$$;

DROP TRIGGER IF EXISTS organization_deletion_outbox_trigger ON organization;
CREATE TRIGGER organization_deletion_outbox_trigger
BEFORE DELETE ON organization
FOR EACH ROW EXECUTE FUNCTION enqueue_organization_deletion();

-- Backfill current canonical state so deploy also repairs events lost before
-- the outbox existed. Ownerless rows intentionally keep owner_user_id NULL and
-- are handled by the orphan cleanup grace policy.
INSERT INTO organization_projection_outbox
  (organization_id, name, slug, metadata, owner_user_id)
SELECT
  o.id,
  o.name,
  o.slug,
  CASE
    WHEN o.metadata IS NULL OR BTRIM(o.metadata) = '' THEN '{}'::jsonb
    ELSE o.metadata::jsonb
  END,
  (
    SELECT m.user_id
    FROM member m
    WHERE m.organization_id = o.id
      AND 'owner' = ANY(string_to_array(m.role, ','))
    ORDER BY m.created_at
    LIMIT 1
  )
FROM organization o
ON CONFLICT (organization_id) DO NOTHING;

INSERT INTO organization_membership_outbox
  (organization_id, user_id, role, desired_action)
SELECT m.organization_id, m.user_id, m.role, 'upsert'
FROM member m
ON CONFLICT (organization_id, user_id) DO NOTHING;
