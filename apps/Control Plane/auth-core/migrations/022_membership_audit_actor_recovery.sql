-- A Better Auth after-hook enriches membership audit rows with a verified
-- session actor. If the process exits after the canonical transaction commits,
-- that hook cannot be replayed. Persist bounded recovery state so an unknown
-- actor never blocks this member's later audit revisions forever.
ALTER TABLE organization_membership_audit_outbox
  ADD COLUMN IF NOT EXISTS actor_resolution_not_before TIMESTAMPTZ
    NOT NULL DEFAULT (NOW() + INTERVAL '2 minutes'),
  ADD COLUMN IF NOT EXISTS actor_resolution_attempts INTEGER NOT NULL DEFAULT 0,
  ADD COLUMN IF NOT EXISTS actor_resolution_last_error TEXT,
  ADD COLUMN IF NOT EXISTS actor_resolution_dead_lettered_at TIMESTAMPTZ;

ALTER TABLE organization_membership_audit_outbox
  DROP CONSTRAINT IF EXISTS organization_membership_audit_actor_check;
ALTER TABLE organization_membership_audit_outbox
  ADD CONSTRAINT organization_membership_audit_actor_check CHECK (
    (actor_classification = 'pending' AND actor_user_id IS NULL) OR
    (actor_classification = 'verified_user' AND actor_user_id IS NOT NULL) OR
    (actor_classification = 'system_repair' AND actor_user_id IS NULL) OR
    (actor_classification = 'operator' AND actor_user_id IS NOT NULL) OR
    (actor_classification = 'unresolved' AND actor_user_id IS NULL)
  );

ALTER TABLE organization_membership_audit_outbox
  ADD CONSTRAINT organization_membership_audit_actor_resolution_attempts_check
    CHECK (actor_resolution_attempts >= 0),
  ADD CONSTRAINT organization_membership_audit_actor_resolution_state_check
    CHECK (
      (actor_classification = 'unresolved' AND
       actor_resolution_dead_lettered_at IS NOT NULL) OR
      (actor_classification <> 'unresolved' AND
       actor_resolution_dead_lettered_at IS NULL)
    );

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
    (
      (OLD.actor_classification = 'pending' AND
       NEW.actor_classification IN ('verified_user', 'operator')) OR
      (OLD.actor_classification = 'unresolved' AND
       NEW.actor_classification = 'verified_user')
    ) AND
    OLD.published_at IS NULL
  ) THEN
    RAISE EXCEPTION 'organization membership audit actor is immutable';
  END IF;
  IF OLD.actor_classification IS DISTINCT FROM NEW.actor_classification AND NOT (
    (
      (OLD.actor_classification = 'pending' AND
       NEW.actor_classification IN (
         'verified_user', 'system_repair', 'operator', 'unresolved'
       )) OR
      (OLD.actor_classification = 'unresolved' AND
       NEW.actor_classification = 'verified_user')
    ) AND
    OLD.published_at IS NULL
  ) THEN
    RAISE EXCEPTION 'organization membership audit actor classification is immutable';
  END IF;
  RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION recover_pending_membership_audit_actors(
  p_limit INTEGER DEFAULT 200,
  p_max_attempts INTEGER DEFAULT 3
)
RETURNS TABLE(retried INTEGER, dead_lettered INTEGER)
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

REVOKE ALL ON FUNCTION recover_pending_membership_audit_actors(INTEGER, INTEGER)
  FROM PUBLIC;
