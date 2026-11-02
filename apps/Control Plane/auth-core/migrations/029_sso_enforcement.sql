-- WS05-S7: SSO enforcement support.
--
-- sso_signin_evidence is the proof that an owner/admin has really signed in
-- through an organisation's identity provider. org-core asks for it
-- (GET /api/v1/internal/sso/evidence) before it lets an organisation switch its
-- SSO policy to enforce, so an admin cannot lock the organisation out behind a
-- provider that has never worked. It holds ids and a time only: no profile
-- attribute, no token, no e-mail address.
--
-- Additive and idempotent.

CREATE TABLE IF NOT EXISTS sso_signin_evidence (
  id BIGSERIAL PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES "user"(id) ON DELETE CASCADE,
  organization_id TEXT NOT NULL,
  provider_id TEXT NOT NULL,
  signed_in_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS sso_signin_evidence_org_user_idx
  ON sso_signin_evidence (organization_id, user_id, signed_in_at DESC);

-- The membership mutation machinery gains a kind for SCIM deprovisioning
-- (WS05-S8 uses it). Only the table constraint is widened here; the SQL
-- function that validates p_kind is changed by the slice that first calls it.
ALTER TABLE organization_membership_mutation_operation
  DROP CONSTRAINT IF EXISTS organization_membership_mutation_operation_kind_check;
ALTER TABLE organization_membership_mutation_operation
  ADD CONSTRAINT organization_membership_mutation_operation_kind_check
  CHECK (kind IN ('role_change', 'admin_remove', 'self_leave', 'scim_deprovision'));
