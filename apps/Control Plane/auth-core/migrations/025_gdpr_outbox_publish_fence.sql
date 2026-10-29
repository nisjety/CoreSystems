-- Fence PII-bearing outbox delivery against concurrent GDPR erasure. Claims
-- carry an unguessable lease token, and workers hold the same ordered identity
-- advisory locks as deletion/anonymization while re-reading, publishing, and
-- acknowledging an exact row. Therefore no stale in-memory PII can publish
-- after an erasure transaction commits.

ALTER TABLE organization_projection_outbox
  ADD COLUMN IF NOT EXISTS claim_token TEXT;
ALTER TABLE organization_membership_outbox
  ADD COLUMN IF NOT EXISTS claim_token TEXT;
ALTER TABLE organization_membership_audit_outbox
  ADD COLUMN IF NOT EXISTS claim_token TEXT;
ALTER TABLE organization_invitation_audit_outbox
  ADD COLUMN IF NOT EXISTS claim_token TEXT;
ALTER TABLE auth_identity_event_outbox
  ADD COLUMN IF NOT EXISTS claim_token TEXT;

CREATE OR REPLACE FUNCTION acquire_auth_gdpr_identity_locks(
  target_user_id TEXT,
  target_email TEXT
)
RETURNS VOID
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

REVOKE ALL ON FUNCTION acquire_auth_gdpr_identity_locks(TEXT, TEXT)
  FROM PUBLIC;

-- Keep the checksum-ledgered GDPR routines immutable. Rename each deployed
-- implementation and expose a fenced wrapper under the established name.
ALTER FUNCTION gdpr_hard_delete_user(TEXT)
  RENAME TO gdpr_hard_delete_user_unfenced;
ALTER FUNCTION gdpr_anonymize_user(TEXT)
  RENAME TO gdpr_anonymize_user_unfenced;

CREATE FUNCTION gdpr_hard_delete_user(user_id_param TEXT)
RETURNS JSONB
LANGUAGE plpgsql
SECURITY DEFINER
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

CREATE FUNCTION gdpr_anonymize_user(user_id_param TEXT)
RETURNS JSONB
LANGUAGE plpgsql
SECURITY DEFINER
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

CREATE OR REPLACE FUNCTION fence_direct_auth_user_erasure()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
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

REVOKE ALL ON FUNCTION fence_direct_auth_user_erasure() FROM PUBLIC;

DROP TRIGGER IF EXISTS trg_gdpr_fence_auth_outbox_publish ON "user";
CREATE TRIGGER trg_gdpr_fence_auth_outbox_publish
BEFORE DELETE OR UPDATE OF name, email, banned, ban_reason ON "user"
FOR EACH ROW EXECUTE FUNCTION fence_direct_auth_user_erasure();

CREATE OR REPLACE FUNCTION clear_auth_outbox_claims_after_erasure()
RETURNS TRIGGER
LANGUAGE plpgsql
SECURITY DEFINER
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

REVOKE ALL ON FUNCTION clear_auth_outbox_claims_after_erasure() FROM PUBLIC;

DROP TRIGGER IF EXISTS trg_gdpr_clear_auth_outbox_claims ON "user";
CREATE TRIGGER trg_gdpr_clear_auth_outbox_claims
AFTER DELETE OR UPDATE OF name, email, banned, ban_reason ON "user"
FOR EACH ROW EXECUTE FUNCTION clear_auth_outbox_claims_after_erasure();

DO $gdpr_fence_hardening$
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
$gdpr_fence_hardening$;
