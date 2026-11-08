-- WS05-S8: SCIM deprovisioning through the membership mutation machinery.
--
-- Migration 029 widened the kind CHECK; this one teaches the two SQL functions
-- the new kind, so a deprovision keeps the same guarantees as any other removal:
-- one transaction for the operation row, the member delete, the projection
-- revision, the audit actor binding and the stored idempotent result.
--
-- What differs for 'scim_deprovision':
--   * the actor need not be a member (the identity provider acts through a SCIM
--     token; org-core passes the token creator as the audit actor);
--   * the last owner is still protected (owner_count check);
--   * the user's sessions that were active in this organisation are DELETED, not
--     just detached, so a deprovisioned person cannot keep working.
--
-- Idempotent: CREATE OR REPLACE.

CREATE OR REPLACE FUNCTION bind_membership_mutation_operation()
RETURNS TRIGGER
LANGUAGE plpgsql
AS $$
DECLARE
  operation_uuid UUID;
  operation organization_membership_mutation_operation%ROWTYPE;
  target_organization_id TEXT;
  target_user_id TEXT;
  target_member_id TEXT;
  target_previous_role TEXT;
  target_requested_role TEXT;
  target_audit_action TEXT;
  target_revision BIGINT;
BEGIN
  IF TG_OP = 'UPDATE' AND OLD.role IS NOT DISTINCT FROM NEW.role THEN
    RETURN NEW;
  END IF;

  IF TG_OP = 'DELETE' THEN
    target_organization_id := OLD.organization_id;
    target_user_id := OLD.user_id;
    target_member_id := OLD.id;
    target_previous_role := OLD.role;
    target_requested_role := OLD.role;
    target_audit_action := 'member_removed';
  ELSE
    target_organization_id := NEW.organization_id;
    target_user_id := NEW.user_id;
    target_member_id := NEW.id;
    target_previous_role := OLD.role;
    target_requested_role := NEW.role;
    target_audit_action := 'role_changed';
  END IF;

  BEGIN
    operation_uuid := NULLIF(
      current_setting('app.membership_operation_id', TRUE), ''
    )::UUID;
  EXCEPTION WHEN invalid_text_representation THEN
    RAISE EXCEPTION 'verified membership mutation operation is required';
  END;

  IF operation_uuid IS NULL THEN
    -- The reviewed historical-owner workflow is the only live-organization
    -- write outside the request operation. It is accepted only when its exact,
    -- unapplied operator-reviewed mapping matches this transition.
    IF TG_OP = 'UPDATE' AND target_requested_role = 'owner' AND EXISTS (
      SELECT 1
      FROM owner_invariant_reviewed_mapping mapping
      WHERE mapping.organization_id = target_organization_id
        AND mapping.owner_user_id = target_user_id
        AND mapping.expected_member_role = target_previous_role
        AND mapping.applied_at IS NULL
    ) THEN
      RETURN NEW;
    END IF;
    -- Parent-organization deletion has separate tombstone/audit evidence.
    IF TG_OP = 'DELETE' AND NOT EXISTS (
      SELECT 1 FROM organization canonical_org
      WHERE canonical_org.id = target_organization_id
    ) THEN
      RETURN OLD;
    END IF;
    RAISE EXCEPTION 'verified membership mutation operation is required';
  END IF;

  SELECT * INTO operation
  FROM organization_membership_mutation_operation
  WHERE operation_id = operation_uuid
  FOR UPDATE;

  IF NOT FOUND OR operation.state <> 'pending' OR
     operation.organization_id IS DISTINCT FROM target_organization_id OR
     operation.user_id IS DISTINCT FROM target_user_id OR
     operation.member_id IS DISTINCT FROM target_member_id OR
     operation.previous_role IS DISTINCT FROM target_previous_role OR
     operation.requested_role IS DISTINCT FROM target_requested_role OR
     (target_audit_action = 'role_changed' AND operation.kind <> 'role_change') OR
     (target_audit_action = 'member_removed' AND
       operation.kind NOT IN ('admin_remove', 'self_leave', 'scim_deprovision')) THEN
    RAISE EXCEPTION 'membership mutation operation does not match transition';
  END IF;

  SELECT projection.revision INTO STRICT target_revision
  FROM organization_membership_outbox projection
  WHERE projection.organization_id = target_organization_id
    AND projection.user_id = target_user_id;

  IF target_revision <> operation.observed_revision + 1 THEN
    RAISE EXCEPTION 'membership mutation projection revision conflicted';
  END IF;

  UPDATE organization_membership_audit_outbox audit
  SET actor_user_id = operation.actor_user_id,
      actor_classification = 'verified_user',
      actor_resolution_last_error = NULL,
      actor_resolution_dead_lettered_at = NULL,
      updated_at = NOW()
  WHERE audit.organization_id = target_organization_id
    AND audit.user_id = target_user_id
    AND audit.member_id = target_member_id
    AND audit.revision = target_revision
    AND audit.action = target_audit_action
    AND audit.previous_role = target_previous_role
    AND (
      (target_audit_action = 'role_changed' AND
        audit.applied_role = target_requested_role) OR
      (target_audit_action = 'member_removed' AND audit.applied_role IS NULL)
    )
    AND audit.actor_classification IN ('pending', 'unresolved')
    AND audit.published_at IS NULL;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'exact membership audit transition is unavailable';
  END IF;

  RETURN COALESCE(NEW, OLD);
END;
$$;

CREATE OR REPLACE FUNCTION apply_membership_mutation(
  p_operation_id UUID,
  p_organization_id TEXT,
  p_user_id TEXT,
  p_member_id TEXT,
  p_kind TEXT,
  p_observed_revision BIGINT,
  p_previous_role TEXT,
  p_requested_role TEXT,
  p_actor_user_id TEXT
)
RETURNS JSONB
LANGUAGE plpgsql
AS $$
DECLARE
  existing organization_membership_mutation_operation%ROWTYPE;
  actor_role TEXT;
  canonical_role TEXT;
  canonical_revision BIGINT;
  owner_count INTEGER;
  target_audit_revision BIGINT;
  mutation_result JSONB;
BEGIN
  IF p_operation_id IS NULL OR
     NULLIF(BTRIM(p_organization_id), '') IS NULL OR
     NULLIF(BTRIM(p_user_id), '') IS NULL OR
     NULLIF(BTRIM(p_member_id), '') IS NULL OR
     p_kind NOT IN ('role_change', 'admin_remove', 'self_leave', 'scim_deprovision') OR
     p_observed_revision < 1 OR
     p_previous_role NOT IN ('owner', 'admin', 'member', 'viewer') OR
     p_requested_role NOT IN ('owner', 'admin', 'member', 'viewer') OR
     NULLIF(BTRIM(p_actor_user_id), '') IS NULL OR
     (p_kind IN ('admin_remove', 'self_leave', 'scim_deprovision') AND
       p_requested_role <> p_previous_role) THEN
    RAISE EXCEPTION 'invalid membership mutation operation';
  END IF;

  SELECT * INTO existing
  FROM organization_membership_mutation_operation
  WHERE operation_id = p_operation_id;
  IF FOUND THEN
    IF existing.organization_id = p_organization_id AND
       existing.user_id = p_user_id AND
       existing.member_id = p_member_id AND
       existing.kind = p_kind AND
       existing.observed_revision = p_observed_revision AND
       existing.previous_role = p_previous_role AND
       existing.requested_role = p_requested_role AND
       existing.actor_user_id = p_actor_user_id AND
       existing.state = 'completed' THEN
      RETURN existing.result;
    END IF;
    RAISE EXCEPTION 'membership operation id was reused with different input';
  END IF;

  -- One lock serializes all membership authority decisions for an organization,
  -- including last-owner checks and operation-id retry races.
  PERFORM 1 FROM organization
  WHERE id = p_organization_id
  FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'organization not found';
  END IF;

  SELECT * INTO existing
  FROM organization_membership_mutation_operation
  WHERE operation_id = p_operation_id;
  IF FOUND THEN
    IF existing.organization_id = p_organization_id AND
       existing.user_id = p_user_id AND
       existing.member_id = p_member_id AND
       existing.kind = p_kind AND
       existing.observed_revision = p_observed_revision AND
       existing.previous_role = p_previous_role AND
       existing.requested_role = p_requested_role AND
       existing.actor_user_id = p_actor_user_id AND
       existing.state = 'completed' THEN
      RETURN existing.result;
    END IF;
    RAISE EXCEPTION 'membership operation id was reused with different input';
  END IF;

  -- A SCIM deprovision is requested by the identity provider through a token
  -- an owner or admin created; org-core passes that creator as the audit actor
  -- and the SCIM edge, not a member, is the authority. No member row is needed.
  IF p_kind <> 'scim_deprovision' THEN
    SELECT actor.role INTO actor_role
    FROM member actor
    WHERE actor.organization_id = p_organization_id
      AND actor.user_id = p_actor_user_id
    FOR UPDATE;
    IF NOT FOUND OR actor_role NOT IN ('owner', 'admin', 'member', 'viewer') THEN
      RAISE EXCEPTION 'membership actor is not authorized';
    END IF;
  END IF;

  SELECT target.role, projection.revision
  INTO canonical_role, canonical_revision
  FROM member target
  JOIN organization_membership_outbox projection
    ON projection.organization_id = target.organization_id
   AND projection.user_id = target.user_id
  WHERE target.id = p_member_id
    AND target.organization_id = p_organization_id
    AND target.user_id = p_user_id
  FOR UPDATE OF target, projection;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'membership target not found';
  END IF;
  IF canonical_role IS DISTINCT FROM p_previous_role OR
     canonical_revision IS DISTINCT FROM p_observed_revision THEN
    RAISE EXCEPTION 'membership mutation conflict';
  END IF;

  IF p_kind = 'self_leave' THEN
    IF p_actor_user_id <> p_user_id THEN
      RAISE EXCEPTION 'self-leave actor is not authorized';
    END IF;
  ELSIF p_kind = 'scim_deprovision' THEN
    NULL;
  ELSE
    IF actor_role NOT IN ('owner', 'admin') THEN
      RAISE EXCEPTION 'membership actor is not authorized';
    END IF;
    IF (p_previous_role = 'owner' OR p_requested_role = 'owner') AND
       actor_role <> 'owner' THEN
      RAISE EXCEPTION 'only an owner may mutate owner membership';
    END IF;
  END IF;

  IF p_previous_role = 'owner' AND (
       p_kind IN ('admin_remove', 'self_leave', 'scim_deprovision') OR
       (p_kind = 'role_change' AND p_requested_role <> 'owner')
     ) THEN
    SELECT COUNT(*)::INTEGER INTO owner_count
    FROM member candidate
    WHERE candidate.organization_id = p_organization_id
      AND candidate.role = 'owner';
    IF owner_count <= 1 THEN
      RAISE EXCEPTION 'cannot remove or demote the only owner';
    END IF;
  END IF;

  mutation_result := jsonb_build_object(
    'member', jsonb_build_object(
      'id', p_member_id,
      'organizationId', p_organization_id,
      'userId', p_user_id,
      'role', CASE WHEN p_kind = 'role_change'
        THEN p_requested_role ELSE p_previous_role END
    ),
    'mutationApplied', p_kind <> 'role_change' OR
      p_requested_role <> p_previous_role,
    'revision', CASE
      WHEN p_kind = 'role_change' AND p_requested_role = p_previous_role
        THEN p_observed_revision
      ELSE p_observed_revision + 1
    END
  );

  IF p_kind = 'role_change' AND p_requested_role = p_previous_role THEN
    INSERT INTO organization_membership_mutation_operation (
      operation_id, organization_id, user_id, member_id, kind,
      observed_revision, previous_role, requested_role, actor_user_id,
      state, mutation_applied, result, completed_at
    ) VALUES (
      p_operation_id, p_organization_id, p_user_id, p_member_id, p_kind,
      p_observed_revision, p_previous_role, p_requested_role, p_actor_user_id,
      'completed', FALSE, mutation_result, NOW()
    );
    RETURN mutation_result;
  END IF;

  INSERT INTO organization_membership_mutation_operation (
    operation_id, organization_id, user_id, member_id, kind,
    observed_revision, previous_role, requested_role, actor_user_id,
    state, mutation_applied
  ) VALUES (
    p_operation_id, p_organization_id, p_user_id, p_member_id, p_kind,
    p_observed_revision, p_previous_role, p_requested_role, p_actor_user_id,
    'pending', TRUE
  );
  PERFORM set_config('app.membership_operation_id', p_operation_id::TEXT, TRUE);

  IF p_kind = 'role_change' THEN
    UPDATE member SET role = p_requested_role
    WHERE id = p_member_id AND organization_id = p_organization_id
      AND user_id = p_user_id AND role = p_previous_role;
  ELSE
    DELETE FROM member
    WHERE id = p_member_id AND organization_id = p_organization_id
      AND user_id = p_user_id AND role = p_previous_role;
  END IF;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'membership mutation conflict';
  END IF;

  target_audit_revision := p_observed_revision + 1;
  IF p_kind IN ('admin_remove', 'self_leave') THEN
    UPDATE session
    SET active_organization_id = NULL
    WHERE user_id = p_user_id
      AND active_organization_id = p_organization_id;
  ELSIF p_kind = 'scim_deprovision' THEN
    -- End the sessions that were working inside this organisation, in the same
    -- transaction as the removal. Sessions in other organisations are untouched.
    DELETE FROM session
    WHERE user_id = p_user_id
      AND active_organization_id = p_organization_id;
  END IF;

  IF NOT EXISTS (
    SELECT 1 FROM organization_membership_audit_outbox audit
    WHERE audit.organization_id = p_organization_id
      AND audit.user_id = p_user_id
      AND audit.member_id = p_member_id
      AND audit.revision = target_audit_revision
      AND audit.actor_user_id = p_actor_user_id
      AND audit.actor_classification = 'verified_user'
  ) THEN
    RAISE EXCEPTION 'membership audit actor binding was not committed';
  END IF;

  UPDATE organization_membership_mutation_operation
  SET state = 'completed', audit_revision = target_audit_revision,
      result = mutation_result, completed_at = NOW()
  WHERE operation_id = p_operation_id AND state = 'pending';
  IF NOT FOUND THEN
    RAISE EXCEPTION 'membership operation completion conflicted';
  END IF;

  RETURN mutation_result;
END;
$$;

REVOKE ALL ON FUNCTION apply_membership_mutation(
  UUID, TEXT, TEXT, TEXT, TEXT, BIGINT, TEXT, TEXT, TEXT
) FROM PUBLIC;
