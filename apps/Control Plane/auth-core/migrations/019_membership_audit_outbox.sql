-- Append-only security audit intents for Auth-canonical membership lifecycle.
-- Projection state remains coalesced in organization_membership_outbox, while
-- every invite/create/role/remove audit survives rapid subsequent mutations.
CREATE TABLE IF NOT EXISTS organization_membership_audit_outbox (
  organization_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  member_id TEXT NOT NULL,
  invitation_id TEXT,
  invitation_causality_pending BOOLEAN NOT NULL DEFAULT FALSE,
  revision BIGINT NOT NULL,
  action TEXT NOT NULL,
  role TEXT NOT NULL,
  previous_role TEXT,
  applied_role TEXT,
  actor_user_id TEXT,
  actor_classification TEXT NOT NULL DEFAULT 'pending',
  published_at TIMESTAMPTZ,
  processing_at TIMESTAMPTZ,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (organization_id, user_id, revision),
  CONSTRAINT organization_membership_audit_action_check
    CHECK (action IN ('member_added', 'role_changed', 'member_removed')),
  CONSTRAINT organization_membership_audit_revision_positive
    CHECK (revision > 0),
  CONSTRAINT organization_membership_audit_actor_check CHECK (
    (actor_classification = 'pending' AND actor_user_id IS NULL) OR
    (actor_classification = 'verified_user' AND actor_user_id IS NOT NULL) OR
    (actor_classification = 'system_repair' AND actor_user_id IS NULL) OR
    (actor_classification = 'operator' AND actor_user_id IS NOT NULL)
  ),
  CONSTRAINT organization_membership_audit_transition_check CHECK (
    (action = 'member_added' AND previous_role IS NULL AND applied_role IS NOT NULL) OR
    (action = 'role_changed' AND previous_role IS NOT NULL AND applied_role IS NOT NULL) OR
    (action = 'member_removed' AND previous_role IS NOT NULL AND applied_role IS NULL)
  )
);

CREATE INDEX IF NOT EXISTS organization_membership_audit_outbox_pending
  ON organization_membership_audit_outbox(created_at, organization_id, user_id, revision)
  WHERE published_at IS NULL;

CREATE TABLE IF NOT EXISTS organization_invitation_audit_outbox (
  invitation_id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL,
  inviter_user_id TEXT NOT NULL,
  invitee_email TEXT NOT NULL,
  role TEXT NOT NULL,
  action TEXT NOT NULL DEFAULT 'member_invited',
  published_at TIMESTAMPTZ,
  processing_at TIMESTAMPTZ,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT organization_invitation_audit_action_check
    CHECK (action = 'member_invited')
);

CREATE INDEX IF NOT EXISTS organization_invitation_audit_outbox_pending
  ON organization_invitation_audit_outbox(created_at, invitation_id)
  WHERE published_at IS NULL;

CREATE OR REPLACE FUNCTION protect_organization_invitation_audit_outbox()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'organization invitation audit is append-only';
  END IF;
  IF OLD.invitation_id IS DISTINCT FROM NEW.invitation_id OR
     OLD.organization_id IS DISTINCT FROM NEW.organization_id OR
     OLD.inviter_user_id IS DISTINCT FROM NEW.inviter_user_id OR
     OLD.invitee_email IS DISTINCT FROM NEW.invitee_email OR
     OLD.role IS DISTINCT FROM NEW.role OR
     OLD.action IS DISTINCT FROM NEW.action OR
     OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'organization invitation audit identity is immutable';
  END IF;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS organization_invitation_audit_outbox_immutable
  ON organization_invitation_audit_outbox;
CREATE TRIGGER organization_invitation_audit_outbox_immutable
BEFORE UPDATE OR DELETE ON organization_invitation_audit_outbox
FOR EACH ROW EXECUTE FUNCTION protect_organization_invitation_audit_outbox();

CREATE OR REPLACE FUNCTION enqueue_organization_membership_projection()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
  target_org_id TEXT;
  target_user_id TEXT;
  target_member_id TEXT;
  target_role TEXT;
  target_previous_role TEXT;
  target_action TEXT;
  target_audit_action TEXT;
  target_revision BIGINT;
  target_invitation_id TEXT;
  invitation_match_count INTEGER;
BEGIN
  IF TG_OP = 'UPDATE' THEN
    IF OLD.organization_id IS DISTINCT FROM NEW.organization_id OR
       OLD.user_id IS DISTINCT FROM NEW.user_id OR
       OLD.id IS DISTINCT FROM NEW.id THEN
      RAISE EXCEPTION 'canonical membership identity is immutable';
    END IF;
    IF OLD.role IS NOT DISTINCT FROM NEW.role THEN
      RETURN NEW;
    END IF;
  END IF;

  IF TG_OP = 'DELETE' THEN
    target_org_id := OLD.organization_id;
    target_user_id := OLD.user_id;
    target_member_id := OLD.id;
    target_role := OLD.role;
    target_previous_role := OLD.role;
    target_action := 'remove';
    target_audit_action := 'member_removed';
  ELSIF TG_OP = 'UPDATE' THEN
    target_org_id := NEW.organization_id;
    target_user_id := NEW.user_id;
    target_member_id := NEW.id;
    target_role := NEW.role;
    target_previous_role := OLD.role;
    target_action := 'upsert';
    target_audit_action := 'role_changed';
  ELSE
    target_org_id := NEW.organization_id;
    target_user_id := NEW.user_id;
    target_member_id := NEW.id;
    target_role := NEW.role;
    target_previous_role := NULL;
    target_action := 'upsert';
    target_audit_action := 'member_added';
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
    updated_at = NOW()
  RETURNING revision INTO target_revision;

  IF target_audit_action = 'member_added' THEN
    SELECT MIN(repair.invitation_id), COUNT(*)::INTEGER
    INTO target_invitation_id, invitation_match_count
    FROM invitation_acceptance_repair repair
    JOIN "user" invited_user ON invited_user.id = target_user_id
    WHERE repair.organization_id = target_org_id
      AND repair.normalized_email = LOWER(BTRIM(invited_user.email))
      AND repair.state IN ('pending', 'processing', 'completed')
      AND (
        repair.repaired_member_id = target_member_id OR
        repair.repaired_member_id IS NULL
      );

    IF invitation_match_count <> 1 THEN
      target_invitation_id := NULL;
    END IF;
  END IF;

  INSERT INTO organization_membership_audit_outbox
    (organization_id, user_id, member_id, invitation_id,
     invitation_causality_pending, revision, action, role,
     previous_role, applied_role)
  VALUES (
    target_org_id, target_user_id, target_member_id, target_invitation_id,
    target_audit_action = 'member_added' AND invitation_match_count > 1,
    target_revision,
    target_audit_action, target_role, target_previous_role,
    CASE WHEN target_audit_action = 'member_removed' THEN NULL ELSE target_role END
  );

  RETURN COALESCE(NEW, OLD);
END;
$$;

DROP TRIGGER IF EXISTS member_projection_outbox_trigger ON member;
CREATE TRIGGER member_projection_outbox_trigger
AFTER INSERT OR UPDATE OF organization_id, user_id, role OR DELETE ON member
FOR EACH ROW EXECUTE FUNCTION enqueue_organization_membership_projection();

CREATE OR REPLACE FUNCTION protect_organization_membership_audit_outbox()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'organization membership audit is append-only';
  END IF;
  IF OLD.organization_id IS DISTINCT FROM NEW.organization_id OR
     OLD.user_id IS DISTINCT FROM NEW.user_id OR
     OLD.member_id IS DISTINCT FROM NEW.member_id OR
     (
       OLD.invitation_id IS DISTINCT FROM NEW.invitation_id AND NOT (
         OLD.invitation_id IS NULL AND NEW.invitation_id IS NOT NULL AND
         OLD.invitation_causality_pending = TRUE AND
         NEW.invitation_causality_pending = FALSE AND
         OLD.published_at IS NULL
       )
     ) OR
     (
       OLD.invitation_causality_pending IS DISTINCT FROM
         NEW.invitation_causality_pending AND NOT (
           OLD.invitation_causality_pending = TRUE AND
           NEW.invitation_causality_pending = FALSE AND
           OLD.invitation_id IS NULL AND NEW.invitation_id IS NOT NULL AND
           OLD.published_at IS NULL
         )
     ) OR
     OLD.revision IS DISTINCT FROM NEW.revision OR
     OLD.action IS DISTINCT FROM NEW.action OR
     OLD.role IS DISTINCT FROM NEW.role OR
     OLD.previous_role IS DISTINCT FROM NEW.previous_role OR
     OLD.applied_role IS DISTINCT FROM NEW.applied_role OR
     OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'organization membership audit identity is immutable';
  END IF;
  IF OLD.actor_user_id IS DISTINCT FROM NEW.actor_user_id AND NOT (
    OLD.actor_user_id IS NULL AND NEW.actor_user_id IS NOT NULL AND
    OLD.published_at IS NULL
  ) THEN
    RAISE EXCEPTION 'organization membership audit actor is immutable';
  END IF;
  IF OLD.actor_classification IS DISTINCT FROM NEW.actor_classification AND NOT (
    OLD.actor_classification = 'pending' AND
    NEW.actor_classification IN ('verified_user', 'system_repair', 'operator') AND
    OLD.published_at IS NULL
  ) THEN
    RAISE EXCEPTION 'organization membership audit actor classification is immutable';
  END IF;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS organization_membership_audit_outbox_immutable
  ON organization_membership_audit_outbox;
CREATE TRIGGER organization_membership_audit_outbox_immutable
BEFORE UPDATE OR DELETE ON organization_membership_audit_outbox
FOR EACH ROW EXECUTE FUNCTION protect_organization_membership_audit_outbox();

CREATE OR REPLACE FUNCTION enqueue_organization_invitation_audit()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  INSERT INTO organization_invitation_audit_outbox (
    invitation_id, organization_id, inviter_user_id,
    invitee_email, role, action
  ) VALUES (
    NEW.id, NEW.organization_id, NEW.inviter_id,
    LOWER(BTRIM(NEW.email)), COALESCE(NULLIF(BTRIM(NEW.role), ''), 'member'),
    'member_invited'
  )
  ON CONFLICT (invitation_id) DO NOTHING;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS organization_invitation_audit_outbox_trigger ON invitation;
CREATE TRIGGER organization_invitation_audit_outbox_trigger
AFTER INSERT ON invitation
FOR EACH ROW EXECUTE FUNCTION enqueue_organization_invitation_audit();

-- Migration 018 deliberately requires an operator-reviewed owner mapping. Its
-- UPDATE member statement creates a role_changed audit intent; bind that exact
-- revision to the immutable reviewed_by evidence before the transaction ends.
CREATE OR REPLACE FUNCTION classify_reviewed_owner_membership_audit()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
BEGIN
  UPDATE organization_membership_audit_outbox audit
  SET actor_user_id = NEW.reviewed_by,
      actor_classification = 'operator',
      updated_at = NOW()
  WHERE audit.organization_id = NEW.organization_id
    AND audit.user_id = NEW.owner_user_id
    AND audit.member_id = NEW.member_id
    AND audit.action = 'role_changed'
    AND audit.previous_role = NEW.previous_role
    AND audit.applied_role = NEW.applied_role
    AND audit.actor_classification = 'pending'
    AND audit.published_at IS NULL
    AND audit.revision = (
      SELECT projection.revision
      FROM organization_membership_outbox projection
      WHERE projection.organization_id = audit.organization_id
        AND projection.user_id = audit.user_id
    );
  IF NOT FOUND THEN
    RAISE EXCEPTION 'reviewed owner repair audit transition is unavailable';
  END IF;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS owner_repair_membership_audit_actor
  ON owner_invariant_repair_audit;
CREATE TRIGGER owner_repair_membership_audit_actor
AFTER INSERT ON owner_invariant_repair_audit
FOR EACH ROW EXECUTE FUNCTION classify_reviewed_owner_membership_audit();
