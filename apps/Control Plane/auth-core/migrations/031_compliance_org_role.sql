-- D21i: 'compliance' becomes a fifth canonical organization role.
--
-- A compliance member has the least rights of any role (it reads the org and
-- its members and may view, set and lift legal holds in org-core); it never
-- administers membership. This migration only teaches the membership mutation
-- function that the role exists, so an owner or admin can grant, change and
-- remove it through the same audited, single-transaction path as every other
-- role, and a compliance member can leave or be SCIM-deprovisioned.
--
-- The actor gate is unchanged: only an owner or admin may change someone
-- else's role or remove them (a compliance actor is a valid member, so it may
-- still self-leave).
--
-- Idempotent: CREATE OR REPLACE of the 030 definition with the wider role set.

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
     p_previous_role NOT IN ('owner', 'admin', 'member', 'viewer', 'compliance') OR
     p_requested_role NOT IN ('owner', 'admin', 'member', 'viewer', 'compliance') OR
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
    IF NOT FOUND OR actor_role NOT IN ('owner', 'admin', 'member', 'viewer', 'compliance') THEN
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
