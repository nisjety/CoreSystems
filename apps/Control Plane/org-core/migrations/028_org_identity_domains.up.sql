-- WS05-S6: DNS-proven domain ownership, the identity audit outbox, and the
-- per-organisation SSO policy row.
--
-- org-core owns organization_domains (011), so it owns the proof. A domain is
-- verified only by a DNS TXT record at _verevon-challenge.<domain> carrying the
-- challenge token below; a Brreg match, a website URL or an authenticated e-mail
-- address is never proof. The partial unique index organization_domains_one_verified_owner
-- (011) stays the arbiter of a contested claim: a second organisation that
-- publishes the same record still cannot become the verified owner.
--
-- Additive and idempotent. The columns are nullable: rows written before this
-- migration (the pending 'authenticated_email' claim recorded at provisioning)
-- have no challenge and are given one when an admin first asks for it.

ALTER TABLE organization_domains
  ADD COLUMN IF NOT EXISTS verification_token      TEXT,         -- public DNS challenge value, not a secret
  ADD COLUMN IF NOT EXISTS verification_expires_at TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS last_check_at           TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS last_check_reason       TEXT,
  ADD COLUMN IF NOT EXISTS revoked_at              TIMESTAMPTZ,
  ADD COLUMN IF NOT EXISTS revoked_by              TEXT;

-- 'authenticated_email' is the legacy marker the provisioning path writes on a
-- pending claim (repository.go); it is kept valid so existing rows satisfy the
-- constraint. It never leads to 'verified'.
ALTER TABLE organization_domains DROP CONSTRAINT IF EXISTS organization_domain_method_check;
ALTER TABLE organization_domains ADD CONSTRAINT organization_domain_method_check
  CHECK (verification_method IS NULL OR verification_method IN ('dns_txt', 'operator', 'authenticated_email'));

-- A verified row must say how it was proven; 'authenticated_email' never counts.
ALTER TABLE organization_domains DROP CONSTRAINT IF EXISTS organization_domain_verified_method_check;
ALTER TABLE organization_domains ADD CONSTRAINT organization_domain_verified_method_check
  CHECK (status <> 'verified' OR verification_method IN ('dns_txt', 'operator')) NOT VALID;

-- Durable audit intent for every identity change, in the shape of
-- organization_gdpr_audit_outbox (017) and organization_siem_audit_outbox (022).
-- The erasure outbox pins its subject to the erasure event and withholds INSERT
-- from org_core_app; this sibling accepts any org-core control subject and lets
-- the scoped transaction insert beside the change it records. Payloads are
-- content-free: ids, the domain name and kinds only, never a token or a record.
CREATE TABLE IF NOT EXISTS organization_identity_audit_outbox (
  event_id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL,
  subject TEXT NOT NULL,
  payload JSONB NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  processing_at TIMESTAMPTZ,
  next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  published_at TIMESTAMPTZ,
  dead_lettered_at TIMESTAMPTZ,
  last_error TEXT,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT organization_identity_audit_outbox_event_id_nonempty
    CHECK (length(event_id) BETWEEN 1 AND 128 AND event_id = btrim(event_id)),
  CONSTRAINT organization_identity_audit_outbox_org_id_nonempty
    CHECK (btrim(org_id) <> ''),
  CONSTRAINT organization_identity_audit_outbox_subject_authority
    CHECK (subject LIKE 'verevon.audit.v2.control.org-core.%'),
  CONSTRAINT organization_identity_audit_outbox_payload_object
    CHECK (jsonb_typeof(payload) = 'object'),
  CONSTRAINT organization_identity_audit_outbox_payload_identity
    CHECK (payload->>'event_id' = event_id AND payload->>'org_id' = org_id),
  CONSTRAINT organization_identity_audit_outbox_attempts_bounded
    CHECK (attempts >= 0 AND attempts <= 8),
  CONSTRAINT organization_identity_audit_outbox_terminal_state
    CHECK (published_at IS NULL OR dead_lettered_at IS NULL),
  CONSTRAINT organization_identity_audit_outbox_dead_letter_complete
    CHECK (
      dead_lettered_at IS NULL
      OR (attempts = 8 AND published_at IS NULL AND last_error IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_organization_identity_audit_outbox_pending
  ON organization_identity_audit_outbox (next_attempt_at, created_at, event_id)
  WHERE published_at IS NULL AND dead_lettered_at IS NULL;

-- Per-organisation SSO policy (read and written by WS05-S7; created here so the
-- identity tables ship in one migration). mode off|report|enforce; at most two
-- break-glass user ids; revision is the compare-and-set counter.
CREATE TABLE IF NOT EXISTS organization_sso_policy (
  org_id TEXT PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
  mode TEXT NOT NULL DEFAULT 'off' CHECK (mode IN ('off', 'report', 'enforce')),
  break_glass_user_ids TEXT[] NOT NULL DEFAULT '{}' CHECK (cardinality(break_glass_user_ids) <= 2),
  revision BIGINT NOT NULL DEFAULT 1 CHECK (revision > 0),
  updated_by TEXT,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Migration 009 default privileges grant all four verbs to org_core_app on every
-- later table, so state the effective set explicitly (as 024 does).
REVOKE ALL ON organization_identity_audit_outbox FROM org_core_app;
GRANT SELECT, INSERT ON organization_identity_audit_outbox TO org_core_app;

REVOKE ALL ON organization_sso_policy FROM org_core_app;
GRANT SELECT, INSERT, UPDATE ON organization_sso_policy TO org_core_app;

ALTER TABLE organization_identity_audit_outbox ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_identity_audit_outbox_rls_isolation ON organization_identity_audit_outbox;
CREATE POLICY organization_identity_audit_outbox_rls_isolation
  ON organization_identity_audit_outbox
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE organization_sso_policy ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_sso_policy_rls_isolation ON organization_sso_policy;
CREATE POLICY organization_sso_policy_rls_isolation
  ON organization_sso_policy
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
