-- Better Auth 1.6.x commits invitation pending->accepted before its separate
-- member/session transaction. Persist an idempotent repair intent so a failed
-- compensation cannot leave an accepted invitation without membership.
CREATE TABLE IF NOT EXISTS invitation_acceptance_repair (
  invitation_id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL,
  normalized_email TEXT NOT NULL,
  invited_role TEXT NOT NULL DEFAULT 'member',
  state TEXT NOT NULL DEFAULT 'pending',
  not_before TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '30 seconds'),
  processing_at TIMESTAMPTZ,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  repaired_member_id TEXT,
  inserted_member BOOLEAN NOT NULL DEFAULT FALSE,
  completed_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT invitation_acceptance_repair_state_check
    CHECK (state IN ('pending', 'processing', 'completed', 'superseded', 'dead_letter')),
  CONSTRAINT invitation_acceptance_repair_attempts_check CHECK (attempts >= 0)
);

CREATE INDEX IF NOT EXISTS invitation_acceptance_repair_pending
  ON invitation_acceptance_repair (not_before, updated_at)
  WHERE state IN ('pending', 'processing');

CREATE OR REPLACE FUNCTION track_invitation_acceptance_repair()
RETURNS TRIGGER
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

DROP TRIGGER IF EXISTS invitation_acceptance_repair_status_trigger ON invitation;
CREATE TRIGGER invitation_acceptance_repair_status_trigger
AFTER UPDATE OF status OR DELETE ON invitation
FOR EACH ROW EXECUTE FUNCTION track_invitation_acceptance_repair();

-- Better Auth normally creates the canonical member during the repair delay.
-- Bind only an exact organization + normalized-email intent to that row so a
-- subsequent explicit member deletion tombstones the pending repair.
CREATE OR REPLACE FUNCTION bind_invitation_repair_on_member_insert()
RETURNS TRIGGER
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

DROP TRIGGER IF EXISTS invitation_repair_member_insert_trigger ON member;
CREATE TRIGGER invitation_repair_member_insert_trigger
AFTER INSERT ON member
FOR EACH ROW EXECUTE FUNCTION bind_invitation_repair_on_member_insert();

-- Deleting the exact canonical member recorded by a repair intent is an
-- explicit revocation signal, including during the delay window. Tombstone
-- only that intent; never infer a new member from the old invitation.
CREATE OR REPLACE FUNCTION supersede_invitation_repair_on_member_delete()
RETURNS TRIGGER
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

DROP TRIGGER IF EXISTS invitation_repair_member_delete_trigger ON member;
CREATE TRIGGER invitation_repair_member_delete_trigger
AFTER DELETE ON member
FOR EACH ROW EXECUTE FUNCTION supersede_invitation_repair_on_member_delete();

-- Historical accepted invitations are not mutated automatically because a
-- status alone is insufficient evidence to choose a user or recreate access.
-- They are, however, release-blocking operator evidence rather than an
-- invisible gap. The startup preflight reports and stops until the canonical
-- membership is reviewed/reconciled through Auth.
CREATE OR REPLACE VIEW accepted_invitation_membership_gap_report AS
SELECT
  invitation.id AS invitation_id,
  invitation.organization_id,
  LOWER(BTRIM(invitation.email)) AS normalized_email,
  invited_user.id AS matched_user_id,
  CASE
    WHEN invited_user.id IS NULL THEN 'accepted_invitee_missing'
    ELSE 'accepted_membership_missing'
  END AS issue
FROM invitation
LEFT JOIN "user" invited_user
  ON LOWER(BTRIM(invited_user.email)) = LOWER(BTRIM(invitation.email))
WHERE invitation.status = 'accepted'
  AND (
    invited_user.id IS NULL OR NOT EXISTS (
      SELECT 1
      FROM member canonical_member
      WHERE canonical_member.organization_id = invitation.organization_id
        AND canonical_member.user_id = invited_user.id
    )
  );
