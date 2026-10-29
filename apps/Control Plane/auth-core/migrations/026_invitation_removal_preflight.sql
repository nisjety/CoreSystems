-- An accepted invitation proves how a membership was established, but it is
-- not a permanent grant. Once Auth's canonical membership authority records an
-- exact member removal, startup must not misclassify that intentional removal
-- as a partially committed invitation acceptance and recreate access.
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
  )
  AND NOT EXISTS (
    SELECT 1
    FROM invitation_acceptance_repair repair
    JOIN organization_membership_audit_outbox removal
      ON removal.organization_id = repair.organization_id
     AND removal.user_id = invited_user.id
     AND removal.member_id = repair.repaired_member_id
     AND removal.action = 'member_removed'
    WHERE repair.invitation_id = invitation.id
      AND repair.organization_id = invitation.organization_id
      AND repair.normalized_email = LOWER(BTRIM(invitation.email))
      AND repair.state = 'superseded'
      AND repair.repaired_member_id IS NOT NULL
  );
