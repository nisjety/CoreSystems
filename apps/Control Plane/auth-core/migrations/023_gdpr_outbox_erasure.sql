-- Preserve non-identifying membership and invitation audit evidence while
-- erasing durable Auth outbox PII in the same transaction as a user deletion
-- or the existing GDPR anonymization routine. This is an additive migration:
-- deployed GDPR and bootstrap migrations remain immutable.

ALTER TABLE organization_membership_audit_outbox
  ADD COLUMN IF NOT EXISTS subject_erased_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS actor_erased_at TIMESTAMPTZ;

ALTER TABLE organization_invitation_audit_outbox
  ADD COLUMN IF NOT EXISTS inviter_erased_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS invitee_erased_at TIMESTAMPTZ;

-- A hard-deleted user can still have operator-reviewed owner-repair evidence.
-- The staging/review row is not compliance evidence and must not block the
-- canonical user deletion. The append-only repair audit below is retained only
-- in pseudonymized form.
ALTER TABLE owner_invariant_reviewed_mapping
  DROP CONSTRAINT IF EXISTS owner_invariant_reviewed_mapping_owner_user_id_fkey;
ALTER TABLE owner_invariant_reviewed_mapping
  ADD CONSTRAINT owner_invariant_reviewed_mapping_owner_user_id_fkey
  FOREIGN KEY (owner_user_id) REFERENCES "user"(id) ON DELETE CASCADE;

ALTER TABLE owner_invariant_repair_audit
  ADD COLUMN IF NOT EXISTS owner_user_erased_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS reviewer_erased_at TIMESTAMPTZ;

ALTER TABLE organization_membership_outbox
  ADD COLUMN IF NOT EXISTS gdpr_erasure_requested_at TIMESTAMPTZ;
ALTER TABLE organization_membership_outbox
  ADD CONSTRAINT organization_membership_outbox_gdpr_removal_check CHECK (
    gdpr_erasure_requested_at IS NULL OR desired_action = 'remove'
  );

ALTER TABLE owner_invariant_repair_audit
  ADD CONSTRAINT owner_invariant_repair_audit_owner_erasure_check CHECK (
    owner_user_erased_at IS NULL OR owner_user_id LIKE 'erased:%'
  ),
  ADD CONSTRAINT owner_invariant_repair_audit_reviewer_erasure_check CHECK (
    reviewer_erased_at IS NULL OR reviewed_by LIKE 'erased:%'
  );

ALTER TABLE organization_membership_audit_outbox
  DROP CONSTRAINT IF EXISTS organization_membership_audit_actor_check;
ALTER TABLE organization_membership_audit_outbox
  ADD CONSTRAINT organization_membership_audit_actor_check CHECK (
    (actor_classification = 'pending' AND actor_user_id IS NULL) OR
    (actor_classification = 'verified_user' AND actor_user_id IS NOT NULL) OR
    (actor_classification = 'system_repair' AND actor_user_id IS NULL) OR
    (actor_classification = 'operator' AND actor_user_id IS NOT NULL) OR
    (actor_classification = 'unresolved' AND actor_user_id IS NULL) OR
    (actor_classification = 'erased_actor' AND actor_user_id IS NULL)
  );

ALTER TABLE organization_membership_audit_outbox
  ADD CONSTRAINT organization_membership_audit_subject_erasure_check CHECK (
    subject_erased_at IS NULL OR user_id LIKE 'erased:%'
  ),
  ADD CONSTRAINT organization_membership_audit_actor_erasure_check CHECK (
    (actor_classification = 'erased_actor' AND actor_erased_at IS NOT NULL) OR
    (actor_classification <> 'erased_actor' AND actor_erased_at IS NULL)
  );

ALTER TABLE organization_invitation_audit_outbox
  ADD CONSTRAINT organization_invitation_audit_inviter_erasure_check CHECK (
    inviter_erased_at IS NULL OR inviter_user_id LIKE 'erased:%'
  ),
  ADD CONSTRAINT organization_invitation_audit_invitee_erasure_check CHECK (
    invitee_erased_at IS NULL OR invitee_email LIKE 'erased+%@invalid.local'
  );

CREATE OR REPLACE FUNCTION protect_organization_membership_audit_outbox()
RETURNS TRIGGER
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

CREATE OR REPLACE FUNCTION protect_organization_invitation_audit_outbox()
RETURNS TRIGGER
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

CREATE OR REPLACE FUNCTION reject_owner_invariant_audit_mutation()
RETURNS TRIGGER
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

CREATE OR REPLACE FUNCTION purge_completed_gdpr_membership_outbox(
  p_limit INTEGER DEFAULT 200
)
RETURNS INTEGER
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

REVOKE ALL ON FUNCTION purge_completed_gdpr_membership_outbox(INTEGER)
  FROM PUBLIC;

-- The trigger is SECURITY DEFINER because the existing GDPR functions are
-- operator-owned. Pin the runtime search path to the triggering table's schema
-- before accessing any relation so an untrusted session search_path cannot
-- redirect the erasure writes.
CREATE OR REPLACE FUNCTION erase_auth_outbox_pii_on_user_change()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
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

REVOKE ALL ON FUNCTION erase_auth_outbox_pii_on_user_change() FROM PUBLIC;

DROP TRIGGER IF EXISTS trg_gdpr_erase_auth_outboxes ON "user";
CREATE TRIGGER trg_gdpr_erase_auth_outboxes
AFTER DELETE OR UPDATE OF name, email, banned, ban_reason ON "user"
FOR EACH ROW EXECUTE FUNCTION erase_auth_outbox_pii_on_user_change();

-- Legacy GDPR routines predate a dedicated operator role. They are
-- SECURITY DEFINER, so the PostgreSQL default PUBLIC EXECUTE grant and a
-- caller-controlled search_path would turn them into arbitrary-user deletion
-- primitives. Pin each routine to the exact schema where this migration is
-- applied and leave execution only to its owner until release tooling grants a
-- dedicated operator role explicitly.
DO $gdpr_function_hardening$
DECLARE
  auth_schema NAME := current_schema();
BEGIN
  EXECUTE format(
    'ALTER FUNCTION %I.gdpr_hard_delete_user(TEXT) SET search_path = pg_catalog, %I',
    auth_schema, auth_schema
  );
  EXECUTE format(
    'ALTER FUNCTION %I.gdpr_anonymize_user(TEXT) SET search_path = pg_catalog, %I',
    auth_schema, auth_schema
  );
  EXECUTE format(
    'REVOKE ALL ON FUNCTION %I.gdpr_hard_delete_user(TEXT) FROM PUBLIC',
    auth_schema
  );
  EXECUTE format(
    'REVOKE ALL ON FUNCTION %I.gdpr_anonymize_user(TEXT) FROM PUBLIC',
    auth_schema
  );
END;
$gdpr_function_hardening$;
