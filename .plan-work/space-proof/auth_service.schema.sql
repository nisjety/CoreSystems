--
-- PostgreSQL database dump
--

\restrict 9eVTSLgl1eKnAbERkJOVGbeCk1VbMqWLCGxeFXvBrtq5avLMNcNK0spyOaRPrwd

-- Dumped from database version 15.18
-- Dumped by pg_dump version 15.18

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: acquire_auth_gdpr_identity_locks(text, text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.acquire_auth_gdpr_identity_locks(target_user_id text, target_email text) RETURNS void
    LANGUAGE plpgsql
    AS $$
DECLARE
  lock_label TEXT;
BEGIN
  FOR lock_label IN
    SELECT identity.label
    FROM (
      SELECT 'auth-gdpr:email:' || LOWER(BTRIM(target_email)) AS label
      WHERE NULLIF(BTRIM(target_email), '') IS NOT NULL
      UNION
      SELECT 'auth-gdpr:user:' || BTRIM(target_user_id) AS label
      WHERE NULLIF(BTRIM(target_user_id), '') IS NOT NULL
    ) identity
    ORDER BY identity.label
  LOOP
    PERFORM pg_advisory_xact_lock(hashtextextended(lock_label, 0));
  END LOOP;
END;
$$;


--
-- Name: apply_membership_mutation(uuid, text, text, text, text, bigint, text, text, text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.apply_membership_mutation(p_operation_id uuid, p_organization_id text, p_user_id text, p_member_id text, p_kind text, p_observed_revision bigint, p_previous_role text, p_requested_role text, p_actor_user_id text) RETURNS jsonb
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
     p_kind NOT IN ('role_change', 'admin_remove', 'self_leave') OR
     p_observed_revision < 1 OR
     p_previous_role NOT IN ('owner', 'admin', 'member', 'viewer') OR
     p_requested_role NOT IN ('owner', 'admin', 'member', 'viewer') OR
     NULLIF(BTRIM(p_actor_user_id), '') IS NULL OR
     (p_kind IN ('admin_remove', 'self_leave') AND
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

  SELECT actor.role INTO actor_role
  FROM member actor
  WHERE actor.organization_id = p_organization_id
    AND actor.user_id = p_actor_user_id
  FOR UPDATE;
  IF NOT FOUND OR actor_role NOT IN ('owner', 'admin', 'member', 'viewer') THEN
    RAISE EXCEPTION 'membership actor is not authorized';
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
       p_kind IN ('admin_remove', 'self_leave') OR
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


--
-- Name: apply_reviewed_owner_repairs(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.apply_reviewed_owner_repairs() RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
  issue_count INTEGER;
  applied_count INTEGER := 0;
  affected INTEGER;
  candidate RECORD;
BEGIN
  -- Freeze the canonical evidence while the report is evaluated and mappings
  -- are applied. This is an explicit operator workflow, not a request path.
  LOCK TABLE organization IN SHARE ROW EXCLUSIVE MODE;
  LOCK TABLE member IN SHARE ROW EXCLUSIVE MODE;
  LOCK TABLE owner_invariant_reviewed_mapping IN SHARE ROW EXCLUSIVE MODE;

  SELECT COUNT(*)::INTEGER
  INTO issue_count
  FROM owner_invariant_preflight_report;

  IF issue_count > 0 THEN
    RAISE EXCEPTION 'OWNER_INVARIANT_PREFLIGHT_FAILED: % unresolved issue(s); inspect owner_invariant_preflight_report', issue_count
      USING ERRCODE = 'check_violation';
  END IF;

  FOR candidate IN
    SELECT
      mapping.mapping_id,
      mapping.organization_id,
      mapping.owner_user_id,
      mapping.expected_member_role,
      mapping.reviewed_by,
      mapping.reviewed_at,
      canonical_member.id AS member_id,
      organization.name,
      organization.slug,
      organization.metadata
    FROM owner_invariant_reviewed_mapping mapping
    JOIN organization
      ON organization.id = mapping.organization_id
     AND organization.created_at = mapping.expected_organization_created_at
    JOIN member canonical_member
      ON canonical_member.organization_id = mapping.organization_id
     AND canonical_member.user_id = mapping.owner_user_id
     AND canonical_member.role = mapping.expected_member_role
    WHERE mapping.applied_at IS NULL
    ORDER BY mapping.mapping_id
  LOOP
    UPDATE member
    SET role = 'owner'
    WHERE id = candidate.member_id
      AND organization_id = candidate.organization_id
      AND user_id = candidate.owner_user_id
      AND role = candidate.expected_member_role
      AND NOT EXISTS (
        SELECT 1 FROM member existing_owner
        WHERE existing_owner.organization_id = candidate.organization_id
          AND 'owner' = ANY(
            regexp_split_to_array(
              COALESCE(existing_owner.role, ''),
              '[[:space:]]*,[[:space:]]*'
            )
          )
      );
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 1 THEN
      RAISE EXCEPTION 'OWNER_INVARIANT_PREFLIGHT_STALE: mapping % no longer matches canonical membership', candidate.mapping_id
        USING ERRCODE = 'check_violation';
    END IF;

    INSERT INTO organization_projection_outbox (
      organization_id, name, slug, metadata, owner_user_id
    ) VALUES (
      candidate.organization_id,
      candidate.name,
      candidate.slug,
      CASE
        WHEN candidate.metadata IS NULL OR BTRIM(candidate.metadata) = ''
          THEN '{}'::jsonb
        ELSE candidate.metadata::jsonb
      END,
      candidate.owner_user_id
    )
    ON CONFLICT (organization_id) DO UPDATE SET
      owner_user_id = EXCLUDED.owner_user_id,
      revision = organization_projection_outbox.revision + 1,
      published_at = NULL,
      processing_at = NULL,
      last_error = NULL,
      updated_at = NOW();

    UPDATE owner_invariant_reviewed_mapping
    SET applied_at = NOW(), applied_member_id = candidate.member_id
    WHERE mapping_id = candidate.mapping_id AND applied_at IS NULL;
    GET DIAGNOSTICS affected = ROW_COUNT;
    IF affected <> 1 THEN
      RAISE EXCEPTION 'OWNER_INVARIANT_PREFLIGHT_STALE: mapping % was concurrently applied', candidate.mapping_id
        USING ERRCODE = 'check_violation';
    END IF;

    INSERT INTO owner_invariant_repair_audit (
      mapping_id, organization_id, owner_user_id, member_id,
      previous_role, applied_role, reviewed_by, reviewed_at
    ) VALUES (
      candidate.mapping_id, candidate.organization_id,
      candidate.owner_user_id, candidate.member_id,
      candidate.expected_member_role, 'owner',
      candidate.reviewed_by, candidate.reviewed_at
    );
    applied_count := applied_count + 1;
  END LOOP;

  RETURN applied_count;
END;
$$;


--
-- Name: bind_invitation_repair_on_member_insert(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.bind_invitation_repair_on_member_insert() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  UPDATE invitation_acceptance_repair r
  SET repaired_member_id = NEW.id,
      inserted_member = FALSE,
      updated_at = NOW()
  FROM "user" u
  WHERE u.id = NEW.user_id
    AND r.organization_id = NEW.organization_id
    AND r.normalized_email = LOWER(BTRIM(u.email))
    AND r.repaired_member_id IS NULL
    AND r.state IN ('pending', 'processing')
    AND r.invitation_id = (
      SELECT MIN(candidate.invitation_id)
      FROM invitation_acceptance_repair candidate
      WHERE candidate.organization_id = NEW.organization_id
        AND candidate.normalized_email = LOWER(BTRIM(u.email))
        AND candidate.repaired_member_id IS NULL
        AND candidate.state IN ('pending', 'processing')
      HAVING COUNT(*) = 1
    );
  RETURN NEW;
END;
$$;


--
-- Name: bind_membership_mutation_operation(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.bind_membership_mutation_operation() RETURNS trigger
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
       operation.kind NOT IN ('admin_remove', 'self_leave')) THEN
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


--
-- Name: classify_reviewed_owner_membership_audit(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.classify_reviewed_owner_membership_audit() RETURNS trigger
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


--
-- Name: clear_auth_outbox_claims_after_erasure(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.clear_auth_outbox_claims_after_erasure() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
DECLARE
  target_user_id TEXT;
  target_email TEXT;
BEGIN
  PERFORM set_config(
    'search_path', quote_ident(TG_TABLE_SCHEMA) || ',pg_catalog', TRUE
  );
  IF TG_OP = 'DELETE' THEN
    target_user_id := OLD.id;
    target_email := OLD.email;
  ELSIF OLD.id = NEW.id AND
        OLD.email IS DISTINCT FROM NEW.email AND
        NEW.name = 'Deleted User' AND
        NEW.banned IS TRUE AND
        NEW.ban_reason = 'GDPR deletion request' AND
        NEW.email LIKE 'deleted_%@anonymized.local' THEN
    target_user_id := OLD.id;
    target_email := OLD.email;
  ELSE
    RETURN NEW;
  END IF;

  UPDATE organization_projection_outbox
  SET claim_token = NULL, processing_at = NULL, updated_at = NOW()
  WHERE owner_user_id = target_user_id;
  UPDATE organization_membership_outbox
  SET claim_token = NULL, processing_at = NULL, updated_at = NOW()
  WHERE user_id = target_user_id;
  UPDATE organization_membership_audit_outbox
  SET claim_token = NULL, processing_at = NULL, updated_at = NOW()
  WHERE user_id = target_user_id OR actor_user_id = target_user_id;
  UPDATE organization_invitation_audit_outbox
  SET claim_token = NULL, processing_at = NULL, updated_at = NOW()
  WHERE inviter_user_id = target_user_id
     OR invitee_email = LOWER(BTRIM(target_email));
  UPDATE auth_identity_event_outbox
  SET claim_token = NULL, processing_at = NULL, updated_at = NOW()
  WHERE user_id = target_user_id;

  IF TG_OP = 'DELETE' THEN
    RETURN OLD;
  END IF;
  RETURN NEW;
END;
$$;


--
-- Name: enqueue_auth_provider_linked_event(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enqueue_auth_provider_linked_event() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
  auth_user "user"%ROWTYPE;
BEGIN
  IF lower(NEW.provider_id) IN ('credential', 'email-password', 'password') THEN
    RETURN NEW;
  END IF;

  SELECT * INTO STRICT auth_user FROM "user" WHERE id = NEW.user_id;

  -- OAuth signups insert the user before the account in the same transaction.
  -- Enrich the still-pending registration event with the real provider before
  -- either event can be claimed by the delivery worker.
  UPDATE auth_identity_event_outbox
  SET payload = jsonb_set(payload, '{provider}', to_jsonb(NEW.provider_id)),
      updated_at = NOW()
  WHERE user_id = NEW.user_id
    AND event_type = 'user_registered'
    AND published_at IS NULL
    AND dead_lettered_at IS NULL
    AND created_at = transaction_timestamp();

  INSERT INTO auth_identity_event_outbox (event_id, event_type, user_id, payload)
  VALUES (
    'account:' || NEW.id || ':provider_linked',
    'provider_linked',
    NEW.user_id,
    jsonb_strip_nulls(jsonb_build_object(
      'userId', NEW.user_id,
      'email', lower(trim(auth_user.email)),
      'name', auth_user.name,
      'provider', NEW.provider_id,
      'providerAccountId', NEW.account_id,
      'scopesGranted', CASE
        WHEN NEW.scope IS NULL OR trim(NEW.scope) = '' THEN NULL
        ELSE to_jsonb(regexp_split_to_array(trim(NEW.scope), E'\\s+'))
      END
    ))
  )
  ON CONFLICT (event_id) DO NOTHING;
  RETURN NEW;
END;
$$;


--
-- Name: enqueue_auth_user_registered_event(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enqueue_auth_user_registered_event() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  INSERT INTO auth_identity_event_outbox (event_id, event_type, user_id, payload)
  VALUES (
    'user:' || NEW.id || ':registered',
    'user_registered',
    NEW.id,
    jsonb_build_object(
      'userId', NEW.id,
      'email', lower(trim(NEW.email)),
      'name', NEW.name,
      'provider', 'email',
      'emailVerified', NEW.email_verified
    )
  )
  ON CONFLICT (event_id) DO NOTHING;
  RETURN NEW;
END;
$$;


--
-- Name: enqueue_organization_deletion(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enqueue_organization_deletion() RETURNS trigger
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


--
-- Name: enqueue_organization_invitation_audit(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enqueue_organization_invitation_audit() RETURNS trigger
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


--
-- Name: enqueue_organization_membership_projection(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enqueue_organization_membership_projection() RETURNS trigger
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


--
-- Name: enqueue_organization_projection(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.enqueue_organization_projection() RETURNS trigger
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


--
-- Name: erase_auth_outbox_pii_on_user_change(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.erase_auth_outbox_pii_on_user_change() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
DECLARE
  erased_user_id TEXT;
  erased_email TEXT;
  target_user_id TEXT;
  target_email TEXT;
BEGIN
  PERFORM set_config(
    'search_path', quote_ident(TG_TABLE_SCHEMA) || ',pg_catalog', TRUE
  );

  IF TG_OP = 'DELETE' THEN
    target_user_id := OLD.id;
    target_email := OLD.email;
  ELSIF OLD.id = NEW.id AND
        OLD.email IS DISTINCT FROM NEW.email AND
        NEW.name = 'Deleted User' AND
        NEW.banned IS TRUE AND
        NEW.ban_reason = 'GDPR deletion request' AND
        NEW.email LIKE 'deleted_%@anonymized.local' THEN
    target_user_id := OLD.id;
    target_email := OLD.email;
  ELSE
    RETURN NEW;
  END IF;

  erased_user_id := 'erased:' || md5(target_user_id);
  erased_email :=
    'erased+' || md5(
      target_user_id || ':' || LOWER(BTRIM(target_email))
    ) || '@invalid.local';
  PERFORM set_config('app.gdpr_erasure_user_id', target_user_id, TRUE);
  PERFORM set_config('app.gdpr_erasure_email', target_email, TRUE);

  -- Pending and applied review mappings are workflow state rather than audit
  -- evidence. ON DELETE CASCADE handles the hard-delete path; this explicit
  -- removal covers the softer anonymization path as well.
  DELETE FROM owner_invariant_reviewed_mapping
  WHERE owner_user_id = target_user_id;

  UPDATE owner_invariant_repair_audit
  SET owner_user_id = CASE
        WHEN owner_user_id = target_user_id THEN erased_user_id
        ELSE owner_user_id
      END,
      owner_user_erased_at = CASE
        WHEN owner_user_id = target_user_id THEN NOW()
        ELSE owner_user_erased_at
      END,
      reviewed_by = CASE
        WHEN reviewed_by = target_user_id OR
             LOWER(BTRIM(reviewed_by)) = LOWER(BTRIM(target_email))
          THEN 'erased:' || md5(
            target_user_id || ':reviewer:' || LOWER(BTRIM(reviewed_by))
          )
        ELSE reviewed_by
      END,
      reviewer_erased_at = CASE
        WHEN reviewed_by = target_user_id OR
             LOWER(BTRIM(reviewed_by)) = LOWER(BTRIM(target_email))
          THEN NOW()
        ELSE reviewer_erased_at
      END
  WHERE owner_user_id = target_user_id
     OR reviewed_by = target_user_id
     OR LOWER(BTRIM(reviewed_by)) = LOWER(BTRIM(target_email));

  -- Never republish a deleted owner as the organization owner. A reviewed
  -- owner repair will advance the projection revision after the preflight is
  -- resolved.
  UPDATE organization_projection_outbox
  SET owner_user_id = NULL,
      published_at = NULL,
      processing_at = NULL,
      last_error = 'owner removed by GDPR erasure; reviewed owner repair required',
      updated_at = NOW()
  WHERE owner_user_id = target_user_id;

  -- Hard deletion has already removed canonical memberships, whose trigger
  -- creates the exact Org Core removal intent. Keep that raw identifier only
  -- until the exact revision is acknowledged, then purge it durably.
  IF TG_OP = 'DELETE' THEN
    UPDATE organization_membership_outbox
    SET gdpr_erasure_requested_at = COALESCE(
          gdpr_erasure_requested_at, NOW()
        ),
        processing_at = NULL,
        updated_at = NOW()
    WHERE user_id = target_user_id
      AND desired_action = 'remove';
  END IF;

  UPDATE organization_membership_audit_outbox
  SET user_id = CASE
        WHEN user_id = target_user_id THEN erased_user_id
        ELSE user_id
      END,
      subject_erased_at = CASE
        WHEN user_id = target_user_id THEN NOW()
        ELSE subject_erased_at
      END,
      actor_user_id = CASE
        WHEN actor_user_id = target_user_id THEN NULL
        ELSE actor_user_id
      END,
      actor_classification = CASE
        WHEN actor_user_id = target_user_id THEN 'erased_actor'
        WHEN user_id = target_user_id AND actor_classification = 'pending'
          THEN 'unresolved'
        ELSE actor_classification
      END,
      actor_erased_at = CASE
        WHEN actor_user_id = target_user_id THEN NOW()
        ELSE actor_erased_at
      END,
      actor_resolution_dead_lettered_at = CASE
        WHEN user_id = target_user_id AND actor_classification = 'pending'
          THEN NOW()
        ELSE actor_resolution_dead_lettered_at
      END,
      actor_resolution_last_error = CASE
        WHEN user_id = target_user_id AND actor_classification = 'pending'
          THEN 'actor evidence removed by GDPR erasure'
        ELSE actor_resolution_last_error
      END,
      processing_at = NULL,
      updated_at = NOW()
  WHERE user_id = target_user_id OR actor_user_id = target_user_id;

  UPDATE organization_invitation_audit_outbox
  SET inviter_user_id = CASE
        WHEN inviter_user_id = target_user_id THEN erased_user_id
        ELSE inviter_user_id
      END,
      inviter_erased_at = CASE
        WHEN inviter_user_id = target_user_id THEN NOW()
        ELSE inviter_erased_at
      END,
      invitee_email = CASE
        WHEN invitee_email = LOWER(BTRIM(target_email)) THEN erased_email
        ELSE invitee_email
      END,
      invitee_erased_at = CASE
        WHEN invitee_email = LOWER(BTRIM(target_email)) THEN NOW()
        ELSE invitee_erased_at
      END,
      processing_at = NULL,
      updated_at = NOW()
  WHERE inviter_user_id = target_user_id
     OR invitee_email = LOWER(BTRIM(target_email));

  -- Identity events are delivery intents, not compliance audit evidence. Once
  -- the subject is erased there is no legitimate downstream delivery left.
  DELETE FROM auth_identity_event_outbox
  WHERE user_id = target_user_id;

  IF TG_OP = 'DELETE' THEN
    RETURN OLD;
  END IF;
  RETURN NEW;
END;
$$;


--
-- Name: fence_direct_auth_user_erasure(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.fence_direct_auth_user_erasure() RETURNS trigger
    LANGUAGE plpgsql SECURITY DEFINER
    AS $$
BEGIN
  PERFORM set_config(
    'search_path', quote_ident(TG_TABLE_SCHEMA) || ',pg_catalog', TRUE
  );
  IF TG_OP = 'DELETE' THEN
    PERFORM acquire_auth_gdpr_identity_locks(OLD.id, OLD.email);
    RETURN OLD;
  END IF;
  IF OLD.id = NEW.id AND
     OLD.email IS DISTINCT FROM NEW.email AND
     NEW.name = 'Deleted User' AND
     NEW.banned IS TRUE AND
     NEW.ban_reason = 'GDPR deletion request' AND
     NEW.email LIKE 'deleted_%@anonymized.local' THEN
    PERFORM acquire_auth_gdpr_identity_locks(OLD.id, OLD.email);
  END IF;
  RETURN NEW;
END;
$$;


--
-- Name: gdpr_anonymize_user(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.gdpr_anonymize_user(user_id_param text) RETURNS jsonb
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path TO 'pg_catalog', 'public'
    AS $$
DECLARE
  target_email TEXT;
BEGIN
  SELECT email INTO target_email
  FROM "user"
  WHERE id = user_id_param;
  PERFORM acquire_auth_gdpr_identity_locks(user_id_param, target_email);
  RETURN gdpr_anonymize_user_unfenced(user_id_param);
END;
$$;


--
-- Name: gdpr_anonymize_user_unfenced(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.gdpr_anonymize_user_unfenced(user_id_param text) RETURNS jsonb
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path TO 'pg_catalog', 'public'
    AS $$
DECLARE
    result JSONB;
    anonymized_email TEXT;
BEGIN
    -- Generate anonymized email
    anonymized_email := 'deleted_' || user_id_param || '@anonymized.local';
    
    -- Anonymize user data
    UPDATE "user" 
    SET 
        name = 'Deleted User',
        email = anonymized_email,
        phone_number = NULL,
        image = NULL,
        banned = true,
        ban_reason = 'GDPR deletion request',
        updated_at = NOW()
    WHERE id = user_id_param;
    
    -- Remove sessions
    DELETE FROM session WHERE user_id = user_id_param;
    
    -- Remove OAuth accounts
    DELETE FROM account WHERE user_id = user_id_param;
    
    -- Remove 2FA
    DELETE FROM two_factor WHERE user_id = user_id_param;
    
    -- Remove passkeys
    DELETE FROM passkey WHERE user_id = user_id_param;
    
    -- Remove API keys
    DELETE FROM apikey
    WHERE user_id = user_id_param
       OR (config_id = 'user-keys' AND reference_id = user_id_param);
    
    result := jsonb_build_object(
        'success', true,
        'user_id', user_id_param,
        'anonymized_at', NOW(),
        'anonymized_email', anonymized_email
    );
    
    RETURN result;
    
EXCEPTION
    WHEN OTHERS THEN
        RETURN jsonb_build_object(
            'success', false,
            'error', SQLERRM,
            'user_id', user_id_param
        );
END;
$$;


--
-- Name: gdpr_hard_delete_user(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.gdpr_hard_delete_user(user_id_param text) RETURNS jsonb
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path TO 'pg_catalog', 'public'
    AS $$
DECLARE
  target_email TEXT;
BEGIN
  SELECT email INTO target_email
  FROM "user"
  WHERE id = user_id_param;
  PERFORM acquire_auth_gdpr_identity_locks(user_id_param, target_email);
  RETURN gdpr_hard_delete_user_unfenced(user_id_param);
END;
$$;


--
-- Name: gdpr_hard_delete_user_unfenced(text); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.gdpr_hard_delete_user_unfenced(user_id_param text) RETURNS jsonb
    LANGUAGE plpgsql SECURITY DEFINER
    SET search_path TO 'pg_catalog', 'public'
    AS $$
DECLARE
    result JSONB;
    deleted_records JSONB;
BEGIN
    -- Initialize result
    deleted_records := jsonb_build_object(
        'user_id', user_id_param,
        'deleted_at', NOW()
    );
    
    -- Delete from all related tables (cascade should handle most, but explicit for audit)
    -- Sessions
    WITH deleted_sessions AS (
        DELETE FROM session WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'sessions' FROM deleted_sessions;
    
    -- Accounts (OAuth connections)
    WITH deleted_accounts AS (
        DELETE FROM account WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'accounts' FROM deleted_accounts;
    
    -- Two-factor
    WITH deleted_2fa AS (
        DELETE FROM two_factor WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'two_factor' FROM deleted_2fa;
    
    -- Passkeys
    WITH deleted_passkeys AS (
        DELETE FROM passkey WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'passkeys' FROM deleted_passkeys;
    
    -- API Keys
    WITH deleted_apikeys AS (
        DELETE FROM apikey
        WHERE user_id = user_id_param
           OR (config_id = 'user-keys' AND reference_id = user_id_param)
        RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'apikeys' FROM deleted_apikeys;
    
    -- Organization members (removes from all orgs)
    WITH deleted_members AS (
        DELETE FROM member WHERE user_id = user_id_param RETURNING id, organization_id
    )
    SELECT jsonb_agg(jsonb_build_object('id', id, 'org_id', organization_id)) 
    INTO deleted_records->'memberships' FROM deleted_members;
    
    -- Team members
    WITH deleted_team_members AS (
        DELETE FROM team_member WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'team_memberships' FROM deleted_team_members;
    
    -- OAuth consents
    WITH deleted_consents AS (
        DELETE FROM oauth_consent WHERE user_id = user_id_param RETURNING id
    )
    SELECT jsonb_agg(id) INTO deleted_records->'oauth_consents' FROM deleted_consents;
    
    -- Finally, delete the user record itself
    DELETE FROM "user" WHERE id = user_id_param;
    
    result := jsonb_build_object(
        'success', true,
        'user_id', user_id_param,
        'deleted_at', NOW(),
        'deleted_records', deleted_records
    );
    
    RETURN result;
    
EXCEPTION
    WHEN OTHERS THEN
        RETURN jsonb_build_object(
            'success', false,
            'error', SQLERRM,
            'user_id', user_id_param
        );
END;
$$;


--
-- Name: protect_membership_mutation_operation(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.protect_membership_mutation_operation() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'membership mutation operation is append-only';
  END IF;
  IF OLD.operation_id IS DISTINCT FROM NEW.operation_id OR
     OLD.organization_id IS DISTINCT FROM NEW.organization_id OR
     OLD.user_id IS DISTINCT FROM NEW.user_id OR
     OLD.member_id IS DISTINCT FROM NEW.member_id OR
     OLD.kind IS DISTINCT FROM NEW.kind OR
     OLD.observed_revision IS DISTINCT FROM NEW.observed_revision OR
     OLD.previous_role IS DISTINCT FROM NEW.previous_role OR
     OLD.requested_role IS DISTINCT FROM NEW.requested_role OR
     OLD.actor_user_id IS DISTINCT FROM NEW.actor_user_id OR
     OLD.mutation_applied IS DISTINCT FROM NEW.mutation_applied OR
     OLD.created_at IS DISTINCT FROM NEW.created_at OR
     OLD.state <> 'pending' OR NEW.state <> 'completed' OR
     OLD.audit_revision IS NOT NULL OR NEW.audit_revision IS NULL OR
     OLD.result IS NOT NULL OR NEW.result IS NULL OR
     OLD.completed_at IS NOT NULL OR NEW.completed_at IS NULL THEN
    RAISE EXCEPTION 'membership mutation operation result is immutable';
  END IF;
  RETURN NEW;
END;
$$;


--
-- Name: protect_organization_invitation_audit_outbox(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.protect_organization_invitation_audit_outbox() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
  inviter_erasure BOOLEAN;
  invitee_erasure BOOLEAN;
  erasure_trigger_context BOOLEAN;
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'organization invitation audit is append-only';
  END IF;

  erasure_trigger_context :=
    pg_trigger_depth() > 1 AND
    NULLIF(current_setting('app.gdpr_erasure_user_id', TRUE), '') IS NOT NULL;
  inviter_erasure :=
    erasure_trigger_context AND
    current_setting('app.gdpr_erasure_user_id', TRUE) = OLD.inviter_user_id AND
    OLD.inviter_user_id IS DISTINCT FROM NEW.inviter_user_id AND
    OLD.inviter_erased_at IS NULL AND
    NEW.inviter_erased_at IS NOT NULL AND
    NEW.inviter_user_id = 'erased:' || md5(OLD.inviter_user_id);
  invitee_erasure :=
    erasure_trigger_context AND
    OLD.invitee_email IS DISTINCT FROM NEW.invitee_email AND
    OLD.invitee_erased_at IS NULL AND
    NEW.invitee_erased_at IS NOT NULL AND
    NEW.invitee_email =
      'erased+' || md5(
        current_setting('app.gdpr_erasure_user_id', TRUE) || ':' ||
        LOWER(BTRIM(OLD.invitee_email))
      ) || '@invalid.local';

  IF OLD.invitation_id IS DISTINCT FROM NEW.invitation_id OR
     OLD.organization_id IS DISTINCT FROM NEW.organization_id OR
     (OLD.inviter_user_id IS DISTINCT FROM NEW.inviter_user_id AND
      NOT inviter_erasure) OR
     (OLD.invitee_email IS DISTINCT FROM NEW.invitee_email AND
      NOT invitee_erasure) OR
     OLD.role IS DISTINCT FROM NEW.role OR
     OLD.action IS DISTINCT FROM NEW.action OR
     OLD.created_at IS DISTINCT FROM NEW.created_at THEN
    RAISE EXCEPTION 'organization invitation audit identity is immutable';
  END IF;
  IF OLD.inviter_erased_at IS DISTINCT FROM NEW.inviter_erased_at AND
     NOT inviter_erasure THEN
    RAISE EXCEPTION 'organization invitation audit inviter erasure is immutable';
  END IF;
  IF OLD.invitee_erased_at IS DISTINCT FROM NEW.invitee_erased_at AND
     NOT invitee_erasure THEN
    RAISE EXCEPTION 'organization invitation audit invitee erasure is immutable';
  END IF;
  RETURN NEW;
END;
$$;


--
-- Name: protect_organization_membership_audit_outbox(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.protect_organization_membership_audit_outbox() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
  subject_erasure BOOLEAN;
  actor_erasure BOOLEAN;
  erasure_trigger_context BOOLEAN;
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'organization membership audit is append-only';
  END IF;

  erasure_trigger_context :=
    pg_trigger_depth() > 1 AND
    NULLIF(current_setting('app.gdpr_erasure_user_id', TRUE), '') IS NOT NULL;
  subject_erasure :=
    erasure_trigger_context AND
    current_setting('app.gdpr_erasure_user_id', TRUE) = OLD.user_id AND
    OLD.user_id IS DISTINCT FROM NEW.user_id AND
    OLD.subject_erased_at IS NULL AND
    NEW.subject_erased_at IS NOT NULL AND
    NEW.user_id = 'erased:' || md5(OLD.user_id);
  actor_erasure :=
    erasure_trigger_context AND
    current_setting('app.gdpr_erasure_user_id', TRUE) = OLD.actor_user_id AND
    OLD.actor_user_id IS NOT NULL AND
    NEW.actor_user_id IS NULL AND
    OLD.actor_erased_at IS NULL AND
    NEW.actor_erased_at IS NOT NULL AND
    NEW.actor_classification = 'erased_actor';

  IF OLD.organization_id IS DISTINCT FROM NEW.organization_id OR
     (OLD.user_id IS DISTINCT FROM NEW.user_id AND NOT subject_erasure) OR
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
  IF OLD.subject_erased_at IS DISTINCT FROM NEW.subject_erased_at AND
     NOT subject_erasure THEN
    RAISE EXCEPTION 'organization membership audit subject erasure is immutable';
  END IF;
  IF OLD.actor_erased_at IS DISTINCT FROM NEW.actor_erased_at AND
     NOT actor_erasure THEN
    RAISE EXCEPTION 'organization membership audit actor erasure is immutable';
  END IF;
  IF OLD.actor_user_id IS DISTINCT FROM NEW.actor_user_id AND NOT (
    (
      OLD.actor_user_id IS NULL AND NEW.actor_user_id IS NOT NULL AND
      (
        (OLD.actor_classification = 'pending' AND
         NEW.actor_classification IN ('verified_user', 'operator')) OR
        (OLD.actor_classification = 'unresolved' AND
         NEW.actor_classification = 'verified_user')
      ) AND
      OLD.published_at IS NULL
    ) OR actor_erasure
  ) THEN
    RAISE EXCEPTION 'organization membership audit actor is immutable';
  END IF;
  IF OLD.actor_classification IS DISTINCT FROM NEW.actor_classification AND NOT (
    (
      (
        (OLD.actor_classification = 'pending' AND
         NEW.actor_classification IN (
           'verified_user', 'system_repair', 'operator', 'unresolved'
         )) OR
        (OLD.actor_classification = 'unresolved' AND
         NEW.actor_classification = 'verified_user')
      ) AND
      OLD.published_at IS NULL
    ) OR (
      OLD.actor_classification IN ('verified_user', 'operator') AND
      NEW.actor_classification = 'erased_actor' AND actor_erasure
    )
  ) THEN
    RAISE EXCEPTION 'organization membership audit actor classification is immutable';
  END IF;
  RETURN NEW;
END;
$$;


--
-- Name: purge_completed_gdpr_membership_outbox(integer); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.purge_completed_gdpr_membership_outbox(p_limit integer DEFAULT 200) RETURNS integer
    LANGUAGE plpgsql
    AS $$
DECLARE
  purged INTEGER;
BEGIN
  IF p_limit < 1 OR p_limit > 1000 THEN
    RAISE EXCEPTION 'GDPR membership outbox purge limit must be between 1 and 1000';
  END IF;

  WITH candidates AS (
    SELECT pending.organization_id, pending.user_id
    FROM organization_membership_outbox pending
    WHERE pending.gdpr_erasure_requested_at IS NOT NULL
      AND pending.desired_action = 'remove'
      AND pending.synced_at IS NOT NULL
    ORDER BY pending.synced_at, pending.organization_id, pending.user_id
    LIMIT p_limit
    FOR UPDATE SKIP LOCKED
  ), deleted AS (
    DELETE FROM organization_membership_outbox outbox
    USING candidates
    WHERE outbox.organization_id = candidates.organization_id
      AND outbox.user_id = candidates.user_id
      AND outbox.gdpr_erasure_requested_at IS NOT NULL
      AND outbox.desired_action = 'remove'
      AND outbox.synced_at IS NOT NULL
    RETURNING 1
  )
  SELECT COUNT(*)::INTEGER INTO purged FROM deleted;
  RETURN purged;
END;
$$;


--
-- Name: recover_pending_membership_audit_actors(integer, integer); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.recover_pending_membership_audit_actors(p_limit integer DEFAULT 200, p_max_attempts integer DEFAULT 3) RETURNS TABLE(retried integer, dead_lettered integer)
    LANGUAGE plpgsql
    AS $$
BEGIN
  IF p_limit < 1 OR p_limit > 1000 THEN
    RAISE EXCEPTION 'membership audit actor recovery limit must be between 1 and 1000';
  END IF;
  IF p_max_attempts < 1 OR p_max_attempts > 20 THEN
    RAISE EXCEPTION 'membership audit actor recovery attempts must be between 1 and 20';
  END IF;

  RETURN QUERY
  WITH candidates AS (
    SELECT candidate.organization_id, candidate.user_id, candidate.revision
    FROM organization_membership_audit_outbox candidate
    WHERE candidate.published_at IS NULL
      AND candidate.actor_classification = 'pending'
      AND candidate.actor_resolution_dead_lettered_at IS NULL
      AND candidate.actor_resolution_not_before <= NOW()
      AND (
        candidate.processing_at IS NULL OR
        candidate.processing_at < NOW() - INTERVAL '5 minutes'
      )
    ORDER BY candidate.actor_resolution_not_before,
             candidate.organization_id, candidate.user_id, candidate.revision
    LIMIT p_limit
    FOR UPDATE SKIP LOCKED
  ), updated AS (
    UPDATE organization_membership_audit_outbox audit
    SET actor_resolution_attempts = audit.actor_resolution_attempts + 1,
        actor_resolution_last_error = CASE
          WHEN audit.actor_resolution_attempts + 1 >= p_max_attempts
            THEN 'verified actor evidence unavailable after ' ||
                 p_max_attempts || ' attempts'
          ELSE 'awaiting verified actor evidence (attempt ' ||
               (audit.actor_resolution_attempts + 1) || ' of ' ||
               p_max_attempts || ')'
        END,
        actor_classification = CASE
          WHEN audit.actor_resolution_attempts + 1 >= p_max_attempts
            THEN 'unresolved'
          ELSE audit.actor_classification
        END,
        actor_resolution_dead_lettered_at = CASE
          WHEN audit.actor_resolution_attempts + 1 >= p_max_attempts
            THEN NOW()
          ELSE NULL
        END,
        actor_resolution_not_before = CASE
          WHEN audit.actor_resolution_attempts + 1 >= p_max_attempts
            THEN audit.actor_resolution_not_before
          ELSE NOW() + INTERVAL '1 minute'
        END,
        updated_at = NOW()
    FROM candidates
    WHERE audit.organization_id = candidates.organization_id
      AND audit.user_id = candidates.user_id
      AND audit.revision = candidates.revision
      AND audit.actor_classification = 'pending'
      AND audit.published_at IS NULL
    RETURNING audit.actor_classification
  )
  SELECT
    COUNT(*) FILTER (WHERE actor_classification = 'pending')::INTEGER,
    COUNT(*) FILTER (WHERE actor_classification = 'unresolved')::INTEGER
  FROM updated;
END;
$$;


--
-- Name: reject_deleted_organization_id_reuse(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.reject_deleted_organization_id_reuse() RETURNS trigger
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


--
-- Name: reject_owner_invariant_audit_mutation(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.reject_owner_invariant_audit_mutation() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
DECLARE
  erasure_trigger_context BOOLEAN;
  owner_erasure BOOLEAN;
  reviewer_erasure BOOLEAN;
  target_user_id TEXT;
  target_email TEXT;
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'owner invariant repair audit is append-only';
  END IF;

  target_user_id := current_setting('app.gdpr_erasure_user_id', TRUE);
  target_email := current_setting('app.gdpr_erasure_email', TRUE);
  erasure_trigger_context :=
    pg_trigger_depth() > 1 AND NULLIF(target_user_id, '') IS NOT NULL;
  owner_erasure :=
    erasure_trigger_context AND
    OLD.owner_user_id = target_user_id AND
    NEW.owner_user_id = 'erased:' || md5(OLD.owner_user_id) AND
    OLD.owner_user_erased_at IS NULL AND
    NEW.owner_user_erased_at IS NOT NULL;
  reviewer_erasure :=
    erasure_trigger_context AND
    (
      OLD.reviewed_by = target_user_id OR
      (
        NULLIF(target_email, '') IS NOT NULL AND
        LOWER(BTRIM(OLD.reviewed_by)) = LOWER(BTRIM(target_email))
      )
    ) AND
    NEW.reviewed_by = 'erased:' || md5(
      target_user_id || ':reviewer:' || LOWER(BTRIM(OLD.reviewed_by))
    ) AND
    OLD.reviewer_erased_at IS NULL AND
    NEW.reviewer_erased_at IS NOT NULL;

  IF OLD.audit_id IS DISTINCT FROM NEW.audit_id OR
     OLD.mapping_id IS DISTINCT FROM NEW.mapping_id OR
     OLD.organization_id IS DISTINCT FROM NEW.organization_id OR
     (OLD.owner_user_id IS DISTINCT FROM NEW.owner_user_id AND
      NOT owner_erasure) OR
     OLD.member_id IS DISTINCT FROM NEW.member_id OR
     OLD.previous_role IS DISTINCT FROM NEW.previous_role OR
     OLD.applied_role IS DISTINCT FROM NEW.applied_role OR
     (OLD.reviewed_by IS DISTINCT FROM NEW.reviewed_by AND
      NOT reviewer_erasure) OR
     OLD.reviewed_at IS DISTINCT FROM NEW.reviewed_at OR
     OLD.applied_at IS DISTINCT FROM NEW.applied_at THEN
    RAISE EXCEPTION 'owner invariant repair audit is append-only';
  END IF;
  IF OLD.owner_user_erased_at IS DISTINCT FROM NEW.owner_user_erased_at AND
     NOT owner_erasure THEN
    RAISE EXCEPTION 'owner invariant repair audit owner erasure is immutable';
  END IF;
  IF OLD.reviewer_erased_at IS DISTINCT FROM NEW.reviewer_erased_at AND
     NOT reviewer_erasure THEN
    RAISE EXCEPTION 'owner invariant repair audit reviewer erasure is immutable';
  END IF;
  RETURN NEW;
END;
$$;


--
-- Name: supersede_invitation_repair_on_member_delete(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.supersede_invitation_repair_on_member_delete() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  UPDATE invitation_acceptance_repair
  SET state = 'superseded', processing_at = NULL,
      last_error = NULL, updated_at = NOW()
  WHERE repaired_member_id = OLD.id
    AND state <> 'superseded';
  RETURN OLD;
END;
$$;


--
-- Name: track_invitation_acceptance_repair(); Type: FUNCTION; Schema: public; Owner: -
--

CREATE FUNCTION public.track_invitation_acceptance_repair() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
  IF TG_OP = 'DELETE' THEN
    -- An invitation is acceptance workflow evidence, not the membership
    -- authority after acceptance. In particular, deleting an inviter cascades
    -- their invitation rows; that must never revoke an invitee's established
    -- canonical membership. Explicit membership removal remains the sole
    -- revocation path.
    UPDATE invitation_acceptance_repair
    SET state = 'superseded', processing_at = NULL,
        last_error = NULL, updated_at = NOW()
    WHERE invitation_id = OLD.id
      AND state <> 'superseded';
    RETURN OLD;
  END IF;

  IF OLD.status = 'pending' AND NEW.status = 'accepted' THEN
    INSERT INTO invitation_acceptance_repair (
      invitation_id, organization_id, normalized_email, invited_role,
      state, not_before
    ) VALUES (
      NEW.id,
      NEW.organization_id,
      LOWER(BTRIM(NEW.email)),
      COALESCE(NULLIF(BTRIM(NEW.role), ''), 'member'),
      'pending',
      NOW() + INTERVAL '30 seconds'
    )
    ON CONFLICT (invitation_id) DO UPDATE SET
      organization_id = EXCLUDED.organization_id,
      normalized_email = EXCLUDED.normalized_email,
      invited_role = EXCLUDED.invited_role,
      state = 'pending',
      not_before = EXCLUDED.not_before,
      processing_at = NULL,
      attempts = 0,
      last_error = NULL,
      repaired_member_id = NULL,
      inserted_member = FALSE,
      completed_at = NULL,
      updated_at = NOW();
  ELSIF OLD.status = 'accepted'
    AND NEW.status IN ('pending', 'canceled', 'rejected') THEN
    -- Only compensate a membership created by an in-flight repair whose
    -- acceptance is concurrently rolled back. Once the repair completed, the
    -- canonical membership is independent from invitation workflow state and
    -- may be revoked only through Auth's membership authority.
    DELETE FROM member m
    USING invitation_acceptance_repair r
    WHERE r.invitation_id = NEW.id
      AND r.inserted_member = TRUE
      AND r.state IN ('pending', 'processing')
      AND r.completed_at IS NULL
      AND r.repaired_member_id = m.id;

    UPDATE invitation_acceptance_repair
    SET state = 'superseded', processing_at = NULL,
        last_error = NULL, updated_at = NOW()
    WHERE invitation_id = NEW.id
      AND state <> 'superseded';
  END IF;

  RETURN NEW;
END;
$$;


SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: invitation; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.invitation (
    id text NOT NULL,
    organization_id text NOT NULL,
    email text NOT NULL,
    role text,
    status text DEFAULT 'pending'::text NOT NULL,
    expires_at timestamp without time zone NOT NULL,
    inviter_id text NOT NULL,
    team_id text,
    created_at timestamp without time zone DEFAULT now() NOT NULL
);


--
-- Name: invitation_acceptance_repair; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.invitation_acceptance_repair (
    invitation_id text NOT NULL,
    organization_id text NOT NULL,
    normalized_email text NOT NULL,
    invited_role text DEFAULT 'member'::text NOT NULL,
    state text DEFAULT 'pending'::text NOT NULL,
    not_before timestamp with time zone DEFAULT (now() + '00:00:30'::interval) NOT NULL,
    processing_at timestamp with time zone,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    repaired_member_id text,
    inserted_member boolean DEFAULT false NOT NULL,
    completed_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT invitation_acceptance_repair_attempts_check CHECK ((attempts >= 0)),
    CONSTRAINT invitation_acceptance_repair_state_check CHECK ((state = ANY (ARRAY['pending'::text, 'processing'::text, 'completed'::text, 'superseded'::text, 'dead_letter'::text])))
);


--
-- Name: member; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.member (
    id text NOT NULL,
    organization_id text NOT NULL,
    user_id text NOT NULL,
    role text DEFAULT 'member'::text NOT NULL,
    created_at timestamp without time zone NOT NULL
);


--
-- Name: organization_membership_audit_outbox; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organization_membership_audit_outbox (
    organization_id text NOT NULL,
    user_id text NOT NULL,
    member_id text NOT NULL,
    invitation_id text,
    invitation_causality_pending boolean DEFAULT false NOT NULL,
    revision bigint NOT NULL,
    action text NOT NULL,
    role text NOT NULL,
    previous_role text,
    applied_role text,
    actor_user_id text,
    actor_classification text DEFAULT 'pending'::text NOT NULL,
    published_at timestamp with time zone,
    processing_at timestamp with time zone,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    actor_resolution_not_before timestamp with time zone DEFAULT (now() + '00:02:00'::interval) NOT NULL,
    actor_resolution_attempts integer DEFAULT 0 NOT NULL,
    actor_resolution_last_error text,
    actor_resolution_dead_lettered_at timestamp with time zone,
    subject_erased_at timestamp with time zone,
    actor_erased_at timestamp with time zone,
    claim_token text,
    CONSTRAINT organization_membership_audit_action_check CHECK ((action = ANY (ARRAY['member_added'::text, 'role_changed'::text, 'member_removed'::text]))),
    CONSTRAINT organization_membership_audit_actor_check CHECK ((((actor_classification = 'pending'::text) AND (actor_user_id IS NULL)) OR ((actor_classification = 'verified_user'::text) AND (actor_user_id IS NOT NULL)) OR ((actor_classification = 'system_repair'::text) AND (actor_user_id IS NULL)) OR ((actor_classification = 'operator'::text) AND (actor_user_id IS NOT NULL)) OR ((actor_classification = 'unresolved'::text) AND (actor_user_id IS NULL)) OR ((actor_classification = 'erased_actor'::text) AND (actor_user_id IS NULL)))),
    CONSTRAINT organization_membership_audit_actor_erasure_check CHECK ((((actor_classification = 'erased_actor'::text) AND (actor_erased_at IS NOT NULL)) OR ((actor_classification <> 'erased_actor'::text) AND (actor_erased_at IS NULL)))),
    CONSTRAINT organization_membership_audit_actor_resolution_attempts_check CHECK ((actor_resolution_attempts >= 0)),
    CONSTRAINT organization_membership_audit_actor_resolution_state_check CHECK ((((actor_classification = 'unresolved'::text) AND (actor_resolution_dead_lettered_at IS NOT NULL)) OR ((actor_classification <> 'unresolved'::text) AND (actor_resolution_dead_lettered_at IS NULL)))),
    CONSTRAINT organization_membership_audit_revision_positive CHECK ((revision > 0)),
    CONSTRAINT organization_membership_audit_subject_erasure_check CHECK (((subject_erased_at IS NULL) OR (user_id ~~ 'erased:%'::text))),
    CONSTRAINT organization_membership_audit_transition_check CHECK ((((action = 'member_added'::text) AND (previous_role IS NULL) AND (applied_role IS NOT NULL)) OR ((action = 'role_changed'::text) AND (previous_role IS NOT NULL) AND (applied_role IS NOT NULL)) OR ((action = 'member_removed'::text) AND (previous_role IS NOT NULL) AND (applied_role IS NULL))))
);


--
-- Name: user; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public."user" (
    id text NOT NULL,
    name text NOT NULL,
    email text NOT NULL,
    email_verified boolean DEFAULT false NOT NULL,
    image text,
    phone_number text,
    phone_number_verified boolean,
    two_factor_enabled boolean DEFAULT false,
    role text,
    banned boolean DEFAULT false,
    ban_reason text,
    ban_expires timestamp without time zone,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL
);


--
-- Name: accepted_invitation_membership_gap_report; Type: VIEW; Schema: public; Owner: -
--

CREATE VIEW public.accepted_invitation_membership_gap_report AS
 SELECT invitation.id AS invitation_id,
    invitation.organization_id,
    lower(btrim(invitation.email)) AS normalized_email,
    invited_user.id AS matched_user_id,
        CASE
            WHEN (invited_user.id IS NULL) THEN 'accepted_invitee_missing'::text
            ELSE 'accepted_membership_missing'::text
        END AS issue
   FROM (public.invitation
     LEFT JOIN public."user" invited_user ON ((lower(btrim(invited_user.email)) = lower(btrim(invitation.email)))))
  WHERE ((invitation.status = 'accepted'::text) AND ((invited_user.id IS NULL) OR (NOT (EXISTS ( SELECT 1
           FROM public.member canonical_member
          WHERE ((canonical_member.organization_id = invitation.organization_id) AND (canonical_member.user_id = invited_user.id)))))) AND (NOT (EXISTS ( SELECT 1
           FROM (public.invitation_acceptance_repair repair
             JOIN public.organization_membership_audit_outbox removal ON (((removal.organization_id = repair.organization_id) AND (removal.user_id = invited_user.id) AND (removal.member_id = repair.repaired_member_id) AND (removal.action = 'member_removed'::text))))
          WHERE ((repair.invitation_id = invitation.id) AND (repair.organization_id = invitation.organization_id) AND (repair.normalized_email = lower(btrim(invitation.email))) AND (repair.state = 'superseded'::text) AND (repair.repaired_member_id IS NOT NULL))))));


--
-- Name: account; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.account (
    id text NOT NULL,
    account_id text NOT NULL,
    provider_id text NOT NULL,
    user_id text NOT NULL,
    access_token text,
    refresh_token text,
    id_token text,
    access_token_expires_at timestamp without time zone,
    refresh_token_expires_at timestamp without time zone,
    scope text,
    password text,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL
);


--
-- Name: apikey; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.apikey (
    id text NOT NULL,
    name text,
    start text,
    prefix text,
    key text NOT NULL,
    config_id text DEFAULT 'user-keys'::text NOT NULL,
    reference_id text NOT NULL,
    user_id text,
    refill_interval integer,
    refill_amount integer,
    last_refill_at timestamp without time zone,
    enabled boolean DEFAULT true,
    rate_limit_enabled boolean DEFAULT true,
    rate_limit_time_window integer DEFAULT 86400000,
    rate_limit_max integer DEFAULT 10,
    request_count integer DEFAULT 0,
    remaining integer,
    last_request timestamp without time zone,
    expires_at timestamp without time zone,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    permissions text,
    metadata text
);


--
-- Name: auth_identity_event_outbox; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.auth_identity_event_outbox (
    event_id text NOT NULL,
    event_type text NOT NULL,
    user_id text NOT NULL,
    payload jsonb NOT NULL,
    published_at timestamp with time zone,
    dead_lettered_at timestamp with time zone,
    processing_at timestamp with time zone,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    claim_token text,
    CONSTRAINT auth_identity_event_outbox_attempts_check CHECK ((attempts >= 0)),
    CONSTRAINT auth_identity_event_outbox_event_type_check CHECK ((event_type = ANY (ARRAY['user_registered'::text, 'provider_linked'::text])))
);


--
-- Name: auth_schema_migrations; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.auth_schema_migrations (
    version text NOT NULL,
    checksum text NOT NULL,
    applied_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: oauth_access_token; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.oauth_access_token (
    id text NOT NULL,
    access_token text,
    refresh_token text,
    access_token_expires_at timestamp without time zone,
    refresh_token_expires_at timestamp without time zone,
    client_id text,
    user_id text,
    scopes text,
    created_at timestamp without time zone,
    updated_at timestamp without time zone
);


--
-- Name: oauth_application; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.oauth_application (
    id text NOT NULL,
    client_id text,
    client_secret text,
    name text,
    icon text,
    redirect_ur_ls text,
    metadata text,
    authentication_scheme text,
    type text,
    disabled boolean DEFAULT false,
    user_id text,
    created_at timestamp without time zone,
    updated_at timestamp without time zone
);


--
-- Name: oauth_consent; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.oauth_consent (
    id text NOT NULL,
    user_id text,
    client_id text,
    scopes text,
    consent_given boolean,
    created_at timestamp without time zone,
    updated_at timestamp without time zone
);


--
-- Name: org_group; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.org_group (
    id uuid NOT NULL,
    name text NOT NULL,
    host_organization_id text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL
);


--
-- Name: org_group_grant; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.org_group_grant (
    id uuid NOT NULL,
    org_group_id uuid NOT NULL,
    organization_id text NOT NULL,
    data_access boolean DEFAULT false NOT NULL,
    billing_consolidation boolean DEFAULT false NOT NULL,
    granted_by text NOT NULL,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT org_group_grant_has_a_grant CHECK ((data_access OR billing_consolidation))
);


--
-- Name: organization; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organization (
    id text NOT NULL,
    name text NOT NULL,
    slug text,
    logo text,
    metadata text,
    created_at timestamp without time zone NOT NULL
);


--
-- Name: organization_deletion_outbox; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organization_deletion_outbox (
    organization_id text NOT NULL,
    name text NOT NULL,
    billing_synced_at timestamp with time zone,
    org_synced_at timestamp with time zone,
    completed_at timestamp with time zone,
    processing_at timestamp with time zone,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    revision bigint NOT NULL,
    event_synced_at timestamp with time zone,
    CONSTRAINT organization_deletion_outbox_revision_positive CHECK ((revision > 0))
);


--
-- Name: organization_invitation_audit_outbox; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organization_invitation_audit_outbox (
    invitation_id text NOT NULL,
    organization_id text NOT NULL,
    inviter_user_id text NOT NULL,
    invitee_email text NOT NULL,
    role text NOT NULL,
    action text DEFAULT 'member_invited'::text NOT NULL,
    published_at timestamp with time zone,
    processing_at timestamp with time zone,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    inviter_erased_at timestamp with time zone,
    invitee_erased_at timestamp with time zone,
    claim_token text,
    CONSTRAINT organization_invitation_audit_action_check CHECK ((action = 'member_invited'::text)),
    CONSTRAINT organization_invitation_audit_invitee_erasure_check CHECK (((invitee_erased_at IS NULL) OR (invitee_email ~~ 'erased+%@invalid.local'::text))),
    CONSTRAINT organization_invitation_audit_inviter_erasure_check CHECK (((inviter_erased_at IS NULL) OR (inviter_user_id ~~ 'erased:%'::text)))
);


--
-- Name: organization_membership_mutation_operation; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organization_membership_mutation_operation (
    operation_id uuid NOT NULL,
    organization_id text NOT NULL,
    user_id text NOT NULL,
    member_id text NOT NULL,
    kind text NOT NULL,
    observed_revision bigint NOT NULL,
    previous_role text NOT NULL,
    requested_role text NOT NULL,
    actor_user_id text NOT NULL,
    state text NOT NULL,
    mutation_applied boolean NOT NULL,
    audit_revision bigint,
    result jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    completed_at timestamp with time zone,
    CONSTRAINT organization_membership_mutation_operation_completion_check CHECK ((((state = 'pending'::text) AND mutation_applied AND (audit_revision IS NULL) AND (result IS NULL) AND (completed_at IS NULL)) OR ((state = 'completed'::text) AND (result IS NOT NULL) AND (completed_at IS NOT NULL) AND ((mutation_applied AND (audit_revision = (observed_revision + 1))) OR ((NOT mutation_applied) AND (audit_revision IS NULL)))))),
    CONSTRAINT organization_membership_mutation_operation_kind_check CHECK ((kind = ANY (ARRAY['role_change'::text, 'admin_remove'::text, 'self_leave'::text]))),
    CONSTRAINT organization_membership_mutation_operation_revision_check CHECK ((observed_revision > 0)),
    CONSTRAINT organization_membership_mutation_operation_state_check CHECK ((state = ANY (ARRAY['pending'::text, 'completed'::text])))
);


--
-- Name: organization_membership_outbox; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organization_membership_outbox (
    organization_id text NOT NULL,
    user_id text NOT NULL,
    role text DEFAULT 'member'::text NOT NULL,
    desired_action text NOT NULL,
    revision bigint DEFAULT 1 NOT NULL,
    synced_at timestamp with time zone,
    processing_at timestamp with time zone,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    gdpr_erasure_requested_at timestamp with time zone,
    claim_token text,
    CONSTRAINT organization_membership_outbox_action_check CHECK ((desired_action = ANY (ARRAY['upsert'::text, 'remove'::text]))),
    CONSTRAINT organization_membership_outbox_gdpr_removal_check CHECK (((gdpr_erasure_requested_at IS NULL) OR (desired_action = 'remove'::text)))
);


--
-- Name: organization_projection_outbox; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.organization_projection_outbox (
    organization_id text NOT NULL,
    name text NOT NULL,
    slug text,
    metadata jsonb DEFAULT '{}'::jsonb NOT NULL,
    owner_user_id text,
    revision bigint DEFAULT 1 NOT NULL,
    published_at timestamp with time zone,
    attempts integer DEFAULT 0 NOT NULL,
    last_error text,
    processing_at timestamp with time zone,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    updated_at timestamp with time zone DEFAULT now() NOT NULL,
    claim_token text
);


--
-- Name: owner_invariant_reviewed_mapping; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.owner_invariant_reviewed_mapping (
    mapping_id text NOT NULL,
    organization_id text NOT NULL,
    owner_user_id text NOT NULL,
    expected_organization_created_at timestamp without time zone NOT NULL,
    expected_member_role text NOT NULL,
    reviewed_by text NOT NULL,
    reviewed_at timestamp with time zone NOT NULL,
    applied_at timestamp with time zone,
    applied_member_id text,
    CONSTRAINT owner_repair_mapping_id_check CHECK ((btrim(mapping_id) <> ''::text)),
    CONSTRAINT owner_repair_reviewer_check CHECK ((btrim(reviewed_by) <> ''::text)),
    CONSTRAINT owner_repair_role_check CHECK ((btrim(expected_member_role) <> ''::text))
);


--
-- Name: owner_invariant_preflight_report; Type: VIEW; Schema: public; Owner: -
--

CREATE VIEW public.owner_invariant_preflight_report AS
 WITH organization_state AS (
         SELECT o.id AS organization_id,
            o.created_at,
            (count(m.id) FILTER (WHERE ('owner'::text = ANY (regexp_split_to_array(COALESCE(m.role, ''::text), '[[:space:]]*,[[:space:]]*'::text)))))::integer AS owner_count
           FROM (public.organization o
             LEFT JOIN public.member m ON ((m.organization_id = o.id)))
          GROUP BY o.id, o.created_at
        ), pending_mapping AS (
         SELECT owner_invariant_reviewed_mapping.organization_id,
            (count(*))::integer AS mapping_count,
            min(owner_invariant_reviewed_mapping.owner_user_id) AS owner_user_id,
            min(owner_invariant_reviewed_mapping.expected_organization_created_at) AS expected_organization_created_at,
            min(owner_invariant_reviewed_mapping.expected_member_role) AS expected_member_role
           FROM public.owner_invariant_reviewed_mapping
          WHERE (owner_invariant_reviewed_mapping.applied_at IS NULL)
          GROUP BY owner_invariant_reviewed_mapping.organization_id
        ), classified AS (
         SELECT state.organization_id,
            state.owner_count,
            COALESCE(mapping.mapping_count, 0) AS mapping_count,
            mapping.owner_user_id,
                CASE
                    WHEN ((state.owner_count = 0) AND (COALESCE(mapping.mapping_count, 0) = 0)) THEN 'ownerless_unmapped'::text
                    WHEN ((state.owner_count = 0) AND (mapping.mapping_count > 1)) THEN 'ownerless_ambiguous_mapping'::text
                    WHEN ((state.owner_count >= 1) AND (COALESCE(mapping.mapping_count, 0) > 0)) THEN 'stale_already_owned'::text
                    WHEN ((state.owner_count = 0) AND (mapping.mapping_count = 1) AND (mapping.expected_organization_created_at <> state.created_at)) THEN 'stale_organization_precondition'::text
                    WHEN ((state.owner_count = 0) AND (mapping.mapping_count = 1) AND (NOT (EXISTS ( SELECT 1
                       FROM public.member candidate
                      WHERE ((candidate.organization_id = state.organization_id) AND (candidate.user_id = mapping.owner_user_id)))))) THEN 'stale_member_missing'::text
                    WHEN ((state.owner_count = 0) AND (mapping.mapping_count = 1) AND (NOT (EXISTS ( SELECT 1
                       FROM public.member candidate
                      WHERE ((candidate.organization_id = state.organization_id) AND (candidate.user_id = mapping.owner_user_id) AND (candidate.role = mapping.expected_member_role)))))) THEN 'stale_member_role'::text
                    ELSE NULL::text
                END AS issue
           FROM (organization_state state
             LEFT JOIN pending_mapping mapping ON ((mapping.organization_id = state.organization_id)))
        )
 SELECT classified.organization_id,
    classified.issue,
    classified.owner_count,
    classified.mapping_count,
    classified.owner_user_id
   FROM classified
  WHERE (classified.issue IS NOT NULL);


--
-- Name: owner_invariant_repair_audit; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.owner_invariant_repair_audit (
    audit_id bigint NOT NULL,
    mapping_id text NOT NULL,
    organization_id text NOT NULL,
    owner_user_id text NOT NULL,
    member_id text NOT NULL,
    previous_role text NOT NULL,
    applied_role text NOT NULL,
    reviewed_by text NOT NULL,
    reviewed_at timestamp with time zone NOT NULL,
    applied_at timestamp with time zone DEFAULT now() NOT NULL,
    owner_user_erased_at timestamp with time zone,
    reviewer_erased_at timestamp with time zone,
    CONSTRAINT owner_invariant_repair_audit_owner_erasure_check CHECK (((owner_user_erased_at IS NULL) OR (owner_user_id ~~ 'erased:%'::text))),
    CONSTRAINT owner_invariant_repair_audit_reviewer_erasure_check CHECK (((reviewer_erased_at IS NULL) OR (reviewed_by ~~ 'erased:%'::text)))
);


--
-- Name: owner_invariant_repair_audit_audit_id_seq; Type: SEQUENCE; Schema: public; Owner: -
--

ALTER TABLE public.owner_invariant_repair_audit ALTER COLUMN audit_id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.owner_invariant_repair_audit_audit_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: passkey; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.passkey (
    id text NOT NULL,
    name text,
    public_key text NOT NULL,
    user_id text NOT NULL,
    credential_i_d text NOT NULL,
    counter integer NOT NULL,
    device_type text NOT NULL,
    backed_up boolean NOT NULL,
    transports text,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    aaguid text
);


--
-- Name: privacy_consent; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.privacy_consent (
    id text NOT NULL,
    user_id text,
    session_id text,
    analytics boolean DEFAULT false NOT NULL,
    marketing boolean DEFAULT false NOT NULL,
    necessary boolean DEFAULT true NOT NULL,
    created_at timestamp without time zone DEFAULT now() NOT NULL,
    updated_at timestamp without time zone DEFAULT now() NOT NULL
);


--
-- Name: rate_limit; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.rate_limit (
    id text NOT NULL,
    key text,
    count integer,
    last_request bigint
);


--
-- Name: session; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.session (
    id text NOT NULL,
    expires_at timestamp without time zone NOT NULL,
    token text NOT NULL,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone NOT NULL,
    ip_address text,
    user_agent text,
    user_id text NOT NULL,
    active_organization_id text,
    active_team_id text,
    impersonated_by text
);


--
-- Name: sso_provider; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.sso_provider (
    id text NOT NULL,
    issuer text NOT NULL,
    domain text NOT NULL,
    provider_id text NOT NULL,
    oidc_config text,
    saml_config text,
    user_id text,
    organization_id text
);


--
-- Name: team; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.team (
    id text NOT NULL,
    name text NOT NULL,
    organization_id text NOT NULL,
    created_at timestamp without time zone NOT NULL,
    updated_at timestamp without time zone
);


--
-- Name: team_member; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.team_member (
    id text NOT NULL,
    team_id text NOT NULL,
    user_id text NOT NULL,
    created_at timestamp without time zone NOT NULL
);


--
-- Name: two_factor; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.two_factor (
    id text NOT NULL,
    user_id text NOT NULL,
    secret text NOT NULL,
    backup_codes text NOT NULL
);


--
-- Name: verification; Type: TABLE; Schema: public; Owner: -
--

CREATE TABLE public.verification (
    id text NOT NULL,
    identifier text NOT NULL,
    value text NOT NULL,
    expires_at timestamp without time zone NOT NULL,
    created_at timestamp without time zone DEFAULT now(),
    updated_at timestamp without time zone DEFAULT now()
);


--
-- Name: account account_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.account
    ADD CONSTRAINT account_pkey PRIMARY KEY (id);


--
-- Name: apikey apikey_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.apikey
    ADD CONSTRAINT apikey_pkey PRIMARY KEY (id);


--
-- Name: auth_identity_event_outbox auth_identity_event_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.auth_identity_event_outbox
    ADD CONSTRAINT auth_identity_event_outbox_pkey PRIMARY KEY (event_id);


--
-- Name: auth_schema_migrations auth_schema_migrations_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.auth_schema_migrations
    ADD CONSTRAINT auth_schema_migrations_pkey PRIMARY KEY (version);


--
-- Name: invitation_acceptance_repair invitation_acceptance_repair_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invitation_acceptance_repair
    ADD CONSTRAINT invitation_acceptance_repair_pkey PRIMARY KEY (invitation_id);


--
-- Name: invitation invitation_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invitation
    ADD CONSTRAINT invitation_pkey PRIMARY KEY (id);


--
-- Name: member member_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.member
    ADD CONSTRAINT member_pkey PRIMARY KEY (id);


--
-- Name: oauth_access_token oauth_access_token_access_token_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_access_token
    ADD CONSTRAINT oauth_access_token_access_token_key UNIQUE (access_token);


--
-- Name: oauth_access_token oauth_access_token_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_access_token
    ADD CONSTRAINT oauth_access_token_pkey PRIMARY KEY (id);


--
-- Name: oauth_access_token oauth_access_token_refresh_token_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_access_token
    ADD CONSTRAINT oauth_access_token_refresh_token_key UNIQUE (refresh_token);


--
-- Name: oauth_application oauth_application_client_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_application
    ADD CONSTRAINT oauth_application_client_id_key UNIQUE (client_id);


--
-- Name: oauth_application oauth_application_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_application
    ADD CONSTRAINT oauth_application_pkey PRIMARY KEY (id);


--
-- Name: oauth_consent oauth_consent_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.oauth_consent
    ADD CONSTRAINT oauth_consent_pkey PRIMARY KEY (id);


--
-- Name: org_group_grant org_group_grant_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.org_group_grant
    ADD CONSTRAINT org_group_grant_pkey PRIMARY KEY (id);


--
-- Name: org_group_grant org_group_grant_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.org_group_grant
    ADD CONSTRAINT org_group_grant_unique UNIQUE (org_group_id, organization_id);


--
-- Name: org_group org_group_host_unique; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.org_group
    ADD CONSTRAINT org_group_host_unique UNIQUE (host_organization_id);


--
-- Name: org_group org_group_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.org_group
    ADD CONSTRAINT org_group_pkey PRIMARY KEY (id);


--
-- Name: organization_deletion_outbox organization_deletion_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_deletion_outbox
    ADD CONSTRAINT organization_deletion_outbox_pkey PRIMARY KEY (organization_id);


--
-- Name: organization_invitation_audit_outbox organization_invitation_audit_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_invitation_audit_outbox
    ADD CONSTRAINT organization_invitation_audit_outbox_pkey PRIMARY KEY (invitation_id);


--
-- Name: organization_membership_audit_outbox organization_membership_audit_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_membership_audit_outbox
    ADD CONSTRAINT organization_membership_audit_outbox_pkey PRIMARY KEY (organization_id, user_id, revision);


--
-- Name: organization_membership_mutation_operation organization_membership_mutation_operation_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_membership_mutation_operation
    ADD CONSTRAINT organization_membership_mutation_operation_pkey PRIMARY KEY (operation_id);


--
-- Name: organization_membership_outbox organization_membership_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_membership_outbox
    ADD CONSTRAINT organization_membership_outbox_pkey PRIMARY KEY (organization_id, user_id);


--
-- Name: organization organization_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization
    ADD CONSTRAINT organization_pkey PRIMARY KEY (id);


--
-- Name: organization_projection_outbox organization_projection_outbox_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_projection_outbox
    ADD CONSTRAINT organization_projection_outbox_pkey PRIMARY KEY (organization_id);


--
-- Name: organization organization_slug_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization
    ADD CONSTRAINT organization_slug_key UNIQUE (slug);


--
-- Name: owner_invariant_repair_audit owner_invariant_repair_audit_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.owner_invariant_repair_audit
    ADD CONSTRAINT owner_invariant_repair_audit_pkey PRIMARY KEY (audit_id);


--
-- Name: owner_invariant_reviewed_mapping owner_invariant_reviewed_mapping_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.owner_invariant_reviewed_mapping
    ADD CONSTRAINT owner_invariant_reviewed_mapping_pkey PRIMARY KEY (mapping_id);


--
-- Name: passkey passkey_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.passkey
    ADD CONSTRAINT passkey_pkey PRIMARY KEY (id);


--
-- Name: privacy_consent privacy_consent_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.privacy_consent
    ADD CONSTRAINT privacy_consent_pkey PRIMARY KEY (id);


--
-- Name: rate_limit rate_limit_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.rate_limit
    ADD CONSTRAINT rate_limit_pkey PRIMARY KEY (id);


--
-- Name: session session_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.session
    ADD CONSTRAINT session_pkey PRIMARY KEY (id);


--
-- Name: session session_token_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.session
    ADD CONSTRAINT session_token_key UNIQUE (token);


--
-- Name: sso_provider sso_provider_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sso_provider
    ADD CONSTRAINT sso_provider_pkey PRIMARY KEY (id);


--
-- Name: sso_provider sso_provider_provider_id_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sso_provider
    ADD CONSTRAINT sso_provider_provider_id_key UNIQUE (provider_id);


--
-- Name: team_member team_member_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.team_member
    ADD CONSTRAINT team_member_pkey PRIMARY KEY (id);


--
-- Name: team team_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.team
    ADD CONSTRAINT team_pkey PRIMARY KEY (id);


--
-- Name: two_factor two_factor_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.two_factor
    ADD CONSTRAINT two_factor_pkey PRIMARY KEY (id);


--
-- Name: user user_email_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."user"
    ADD CONSTRAINT user_email_key UNIQUE (email);


--
-- Name: user user_phone_number_key; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."user"
    ADD CONSTRAINT user_phone_number_key UNIQUE (phone_number);


--
-- Name: user user_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public."user"
    ADD CONSTRAINT user_pkey PRIMARY KEY (id);


--
-- Name: verification verification_pkey; Type: CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.verification
    ADD CONSTRAINT verification_pkey PRIMARY KEY (id);


--
-- Name: account_provider_account_unique; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX account_provider_account_unique ON public.account USING btree (provider_id, account_id);


--
-- Name: idx_account_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_account_user_id ON public.account USING btree (user_id);


--
-- Name: idx_apikey_config_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_apikey_config_id ON public.apikey USING btree (config_id);


--
-- Name: idx_apikey_config_reference; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_apikey_config_reference ON public.apikey USING btree (config_id, reference_id);


--
-- Name: idx_apikey_key_hash; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_apikey_key_hash ON public.apikey USING btree (key);


--
-- Name: idx_apikey_reference_enabled; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_apikey_reference_enabled ON public.apikey USING btree (reference_id, enabled) WHERE (enabled = true);


--
-- Name: idx_apikey_reference_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_apikey_reference_id ON public.apikey USING btree (reference_id);


--
-- Name: idx_apikey_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_apikey_user_id ON public.apikey USING btree (user_id);


--
-- Name: idx_auth_identity_event_outbox_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_auth_identity_event_outbox_pending ON public.auth_identity_event_outbox USING btree (created_at, event_id) WHERE ((published_at IS NULL) AND (dead_lettered_at IS NULL));


--
-- Name: idx_member_org_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_member_org_id ON public.member USING btree (organization_id);


--
-- Name: idx_member_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_member_user_id ON public.member USING btree (user_id);


--
-- Name: idx_privacy_consent_session_id; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_privacy_consent_session_id ON public.privacy_consent USING btree (session_id) WHERE (session_id IS NOT NULL);


--
-- Name: idx_privacy_consent_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX idx_privacy_consent_user_id ON public.privacy_consent USING btree (user_id) WHERE (user_id IS NOT NULL);


--
-- Name: idx_session_user_id; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_session_user_id ON public.session USING btree (user_id);


--
-- Name: idx_user_email_banned; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX idx_user_email_banned ON public."user" USING btree (email, banned);


--
-- Name: invitation_acceptance_repair_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX invitation_acceptance_repair_pending ON public.invitation_acceptance_repair USING btree (not_before, updated_at) WHERE (state = ANY (ARRAY['pending'::text, 'processing'::text]));


--
-- Name: member_organization_user_unique; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX member_organization_user_unique ON public.member USING btree (organization_id, user_id);


--
-- Name: org_group_grant_organization_id_idx; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX org_group_grant_organization_id_idx ON public.org_group_grant USING btree (organization_id) WHERE data_access;


--
-- Name: organization_deletion_outbox_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX organization_deletion_outbox_pending ON public.organization_deletion_outbox USING btree (created_at) WHERE (completed_at IS NULL);


--
-- Name: organization_invitation_audit_outbox_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX organization_invitation_audit_outbox_pending ON public.organization_invitation_audit_outbox USING btree (created_at, invitation_id) WHERE (published_at IS NULL);


--
-- Name: organization_membership_audit_outbox_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX organization_membership_audit_outbox_pending ON public.organization_membership_audit_outbox USING btree (created_at, organization_id, user_id, revision) WHERE (published_at IS NULL);


--
-- Name: organization_membership_mutation_operation_member; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX organization_membership_mutation_operation_member ON public.organization_membership_mutation_operation USING btree (organization_id, member_id, created_at);


--
-- Name: organization_membership_outbox_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX organization_membership_outbox_pending ON public.organization_membership_outbox USING btree (updated_at) WHERE (synced_at IS NULL);


--
-- Name: organization_projection_outbox_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX organization_projection_outbox_pending ON public.organization_projection_outbox USING btree (created_at) WHERE ((published_at IS NULL) AND (owner_user_id IS NOT NULL));


--
-- Name: owner_invariant_reviewed_mapping_pending; Type: INDEX; Schema: public; Owner: -
--

CREATE INDEX owner_invariant_reviewed_mapping_pending ON public.owner_invariant_reviewed_mapping USING btree (organization_id, reviewed_at) WHERE (applied_at IS NULL);


--
-- Name: user_email_normalized_unique; Type: INDEX; Schema: public; Owner: -
--

CREATE UNIQUE INDEX user_email_normalized_unique ON public."user" USING btree (lower(btrim(email)));


--
-- Name: invitation invitation_acceptance_repair_status_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER invitation_acceptance_repair_status_trigger AFTER DELETE OR UPDATE OF status ON public.invitation FOR EACH ROW EXECUTE FUNCTION public.track_invitation_acceptance_repair();


--
-- Name: member invitation_repair_member_delete_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER invitation_repair_member_delete_trigger AFTER DELETE ON public.member FOR EACH ROW EXECUTE FUNCTION public.supersede_invitation_repair_on_member_delete();


--
-- Name: member invitation_repair_member_insert_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER invitation_repair_member_insert_trigger AFTER INSERT ON public.member FOR EACH ROW EXECUTE FUNCTION public.bind_invitation_repair_on_member_insert();


--
-- Name: member member_projection_outbox_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER member_projection_outbox_trigger AFTER INSERT OR DELETE OR UPDATE OF organization_id, user_id, role ON public.member FOR EACH ROW EXECUTE FUNCTION public.enqueue_organization_membership_projection();


--
-- Name: organization organization_deletion_outbox_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER organization_deletion_outbox_trigger BEFORE DELETE ON public.organization FOR EACH ROW EXECUTE FUNCTION public.enqueue_organization_deletion();


--
-- Name: organization organization_id_reuse_guard; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER organization_id_reuse_guard BEFORE INSERT ON public.organization FOR EACH ROW EXECUTE FUNCTION public.reject_deleted_organization_id_reuse();


--
-- Name: organization_invitation_audit_outbox organization_invitation_audit_outbox_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER organization_invitation_audit_outbox_immutable BEFORE DELETE OR UPDATE ON public.organization_invitation_audit_outbox FOR EACH ROW EXECUTE FUNCTION public.protect_organization_invitation_audit_outbox();


--
-- Name: invitation organization_invitation_audit_outbox_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER organization_invitation_audit_outbox_trigger AFTER INSERT ON public.invitation FOR EACH ROW EXECUTE FUNCTION public.enqueue_organization_invitation_audit();


--
-- Name: organization_membership_audit_outbox organization_membership_audit_outbox_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER organization_membership_audit_outbox_immutable BEFORE DELETE OR UPDATE ON public.organization_membership_audit_outbox FOR EACH ROW EXECUTE FUNCTION public.protect_organization_membership_audit_outbox();


--
-- Name: organization_membership_mutation_operation organization_membership_mutation_operation_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER organization_membership_mutation_operation_immutable BEFORE DELETE OR UPDATE ON public.organization_membership_mutation_operation FOR EACH ROW EXECUTE FUNCTION public.protect_membership_mutation_operation();


--
-- Name: organization organization_projection_outbox_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER organization_projection_outbox_trigger AFTER INSERT OR UPDATE OF name, slug, metadata ON public.organization FOR EACH ROW EXECUTE FUNCTION public.enqueue_organization_projection();


--
-- Name: owner_invariant_repair_audit owner_invariant_repair_audit_immutable; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER owner_invariant_repair_audit_immutable BEFORE DELETE OR UPDATE ON public.owner_invariant_repair_audit FOR EACH ROW EXECUTE FUNCTION public.reject_owner_invariant_audit_mutation();


--
-- Name: owner_invariant_repair_audit owner_repair_membership_audit_actor; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER owner_repair_membership_audit_actor AFTER INSERT ON public.owner_invariant_repair_audit FOR EACH ROW EXECUTE FUNCTION public.classify_reviewed_owner_membership_audit();


--
-- Name: account trg_auth_provider_linked_outbox; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER trg_auth_provider_linked_outbox AFTER INSERT ON public.account FOR EACH ROW EXECUTE FUNCTION public.enqueue_auth_provider_linked_event();


--
-- Name: user trg_auth_user_registered_outbox; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER trg_auth_user_registered_outbox AFTER INSERT ON public."user" FOR EACH ROW EXECUTE FUNCTION public.enqueue_auth_user_registered_event();


--
-- Name: user trg_gdpr_clear_auth_outbox_claims; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER trg_gdpr_clear_auth_outbox_claims AFTER DELETE OR UPDATE OF name, email, banned, ban_reason ON public."user" FOR EACH ROW EXECUTE FUNCTION public.clear_auth_outbox_claims_after_erasure();


--
-- Name: user trg_gdpr_erase_auth_outboxes; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER trg_gdpr_erase_auth_outboxes AFTER DELETE OR UPDATE OF name, email, banned, ban_reason ON public."user" FOR EACH ROW EXECUTE FUNCTION public.erase_auth_outbox_pii_on_user_change();


--
-- Name: user trg_gdpr_fence_auth_outbox_publish; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER trg_gdpr_fence_auth_outbox_publish BEFORE DELETE OR UPDATE OF name, email, banned, ban_reason ON public."user" FOR EACH ROW EXECUTE FUNCTION public.fence_direct_auth_user_erasure();


--
-- Name: member zz_membership_mutation_operation_trigger; Type: TRIGGER; Schema: public; Owner: -
--

CREATE TRIGGER zz_membership_mutation_operation_trigger AFTER DELETE OR UPDATE OF role ON public.member FOR EACH ROW EXECUTE FUNCTION public.bind_membership_mutation_operation();


--
-- Name: account account_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.account
    ADD CONSTRAINT account_user_id_fkey FOREIGN KEY (user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: apikey apikey_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.apikey
    ADD CONSTRAINT apikey_user_id_fkey FOREIGN KEY (user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: invitation invitation_inviter_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invitation
    ADD CONSTRAINT invitation_inviter_id_fkey FOREIGN KEY (inviter_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: invitation invitation_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.invitation
    ADD CONSTRAINT invitation_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organization(id) ON DELETE CASCADE;


--
-- Name: member member_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.member
    ADD CONSTRAINT member_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organization(id) ON DELETE CASCADE;


--
-- Name: member member_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.member
    ADD CONSTRAINT member_user_id_fkey FOREIGN KEY (user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: org_group_grant org_group_grant_granted_by_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.org_group_grant
    ADD CONSTRAINT org_group_grant_granted_by_fkey FOREIGN KEY (granted_by) REFERENCES public."user"(id);


--
-- Name: org_group_grant org_group_grant_org_group_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.org_group_grant
    ADD CONSTRAINT org_group_grant_org_group_id_fkey FOREIGN KEY (org_group_id) REFERENCES public.org_group(id) ON DELETE CASCADE;


--
-- Name: org_group_grant org_group_grant_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.org_group_grant
    ADD CONSTRAINT org_group_grant_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organization(id) ON DELETE CASCADE;


--
-- Name: org_group org_group_host_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.org_group
    ADD CONSTRAINT org_group_host_organization_id_fkey FOREIGN KEY (host_organization_id) REFERENCES public.organization(id) ON DELETE CASCADE;


--
-- Name: organization_projection_outbox organization_projection_outbox_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.organization_projection_outbox
    ADD CONSTRAINT organization_projection_outbox_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organization(id) ON DELETE CASCADE;


--
-- Name: owner_invariant_reviewed_mapping owner_invariant_reviewed_mapping_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.owner_invariant_reviewed_mapping
    ADD CONSTRAINT owner_invariant_reviewed_mapping_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organization(id) ON DELETE CASCADE;


--
-- Name: owner_invariant_reviewed_mapping owner_invariant_reviewed_mapping_owner_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.owner_invariant_reviewed_mapping
    ADD CONSTRAINT owner_invariant_reviewed_mapping_owner_user_id_fkey FOREIGN KEY (owner_user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: passkey passkey_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.passkey
    ADD CONSTRAINT passkey_user_id_fkey FOREIGN KEY (user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: privacy_consent privacy_consent_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.privacy_consent
    ADD CONSTRAINT privacy_consent_user_id_fkey FOREIGN KEY (user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: session session_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.session
    ADD CONSTRAINT session_user_id_fkey FOREIGN KEY (user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: sso_provider sso_provider_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.sso_provider
    ADD CONSTRAINT sso_provider_user_id_fkey FOREIGN KEY (user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: team_member team_member_team_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.team_member
    ADD CONSTRAINT team_member_team_id_fkey FOREIGN KEY (team_id) REFERENCES public.team(id) ON DELETE CASCADE;


--
-- Name: team_member team_member_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.team_member
    ADD CONSTRAINT team_member_user_id_fkey FOREIGN KEY (user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- Name: team team_organization_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.team
    ADD CONSTRAINT team_organization_id_fkey FOREIGN KEY (organization_id) REFERENCES public.organization(id) ON DELETE CASCADE;


--
-- Name: two_factor two_factor_user_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: -
--

ALTER TABLE ONLY public.two_factor
    ADD CONSTRAINT two_factor_user_id_fkey FOREIGN KEY (user_id) REFERENCES public."user"(id) ON DELETE CASCADE;


--
-- PostgreSQL database dump complete
--

\unrestrict 9eVTSLgl1eKnAbERkJOVGbeCk1VbMqWLCGxeFXvBrtq5avLMNcNK0spyOaRPrwd

