-- WS7.H: durable audit intent for every web policy write, in the shape of
-- organization_siem_audit_outbox (022). The subject is pinned to the web policy
-- event and the scoped write transaction may insert beside the change it records.
-- The payload is content-free: capability KEYS, revision, digest and the tightened
-- flag, never a host name, so a Zero Data Retention organisation's rules do not
-- reach the audit stream.
CREATE TABLE IF NOT EXISTS organization_web_policy_audit_outbox (
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
  CONSTRAINT organization_web_policy_audit_outbox_event_id_nonempty
    CHECK (length(event_id) BETWEEN 1 AND 128 AND event_id = btrim(event_id)),
  CONSTRAINT organization_web_policy_audit_outbox_org_id_nonempty
    CHECK (btrim(org_id) <> ''),
  CONSTRAINT organization_web_policy_audit_outbox_subject_authority
    CHECK (subject = 'verevon.audit.v2.control.org-core.org_web_policy_changed'),
  CONSTRAINT organization_web_policy_audit_outbox_payload_object
    CHECK (jsonb_typeof(payload) = 'object'),
  CONSTRAINT organization_web_policy_audit_outbox_payload_identity
    CHECK (payload->>'event_id' = event_id AND payload->>'org_id' = org_id),
  CONSTRAINT organization_web_policy_audit_outbox_attempts_bounded
    CHECK (attempts >= 0 AND attempts <= 8),
  CONSTRAINT organization_web_policy_audit_outbox_terminal_state
    CHECK (published_at IS NULL OR dead_lettered_at IS NULL),
  CONSTRAINT organization_web_policy_audit_outbox_dead_letter_complete
    CHECK (
      dead_lettered_at IS NULL
      OR (attempts = 8 AND published_at IS NULL AND last_error IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_organization_web_policy_audit_outbox_pending
  ON organization_web_policy_audit_outbox (next_attempt_at, created_at, event_id)
  WHERE published_at IS NULL AND dead_lettered_at IS NULL;

-- The publisher claims and marks rows on the owner pool, never as org_core_app.
REVOKE ALL ON organization_web_policy_audit_outbox FROM org_core_app;
GRANT SELECT, INSERT ON organization_web_policy_audit_outbox TO org_core_app;

ALTER TABLE organization_web_policy_audit_outbox ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_web_policy_audit_outbox_rls_isolation ON organization_web_policy_audit_outbox;
CREATE POLICY organization_web_policy_audit_outbox_rls_isolation
  ON organization_web_policy_audit_outbox
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
