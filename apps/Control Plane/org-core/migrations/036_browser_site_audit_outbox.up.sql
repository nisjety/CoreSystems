-- F-f06-11 (WS21-S6, audit K3-11): durable audit intent for every agent-browser site policy
-- write, and for every member choice write or delete, in the shape of org_flag_audit_outbox (032).
--
-- The write transaction inserts the intent beside the change it records (enqueueAudit in
-- internal/browsersites/audit.go), so an intent exists if and only if the change committed.
-- The publisher claims and marks rows on the owner pool, never as org_core_app. Only two
-- subjects are allowed: the policy event and the choice event.
--
-- The payload is content-free: the event, the organisation, the actor and its role, the
-- revision, and details that carry a host and a level (a choice) or a mode and a rule count
-- (a policy). No note, rule host or other free text reaches the audit stream.
--
-- org_id cascades from organizations(id), as every other browser site table does (023). So the
-- organisation purge, gdpr_hard_delete_organization() (003), removes an undelivered intent with
-- the organisation, without being redefined.
--
-- Every statement is idempotent (IF NOT EXISTS / DROP ... IF EXISTS), so a re-run changes nothing.

CREATE TABLE IF NOT EXISTS browser_site_audit_outbox (
  event_id TEXT PRIMARY KEY,
  org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
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
  CONSTRAINT browser_site_audit_outbox_event_id_nonempty
    CHECK (length(event_id) BETWEEN 1 AND 128 AND event_id = btrim(event_id)),
  CONSTRAINT browser_site_audit_outbox_org_id_nonempty
    CHECK (btrim(org_id) <> ''),
  CONSTRAINT browser_site_audit_outbox_subject_authority
    CHECK (subject IN (
      'verevon.audit.v2.control.org-core.browser_site_policy_changed',
      'verevon.audit.v2.control.org-core.browser_site_pref_changed'
    )),
  CONSTRAINT browser_site_audit_outbox_payload_object
    CHECK (jsonb_typeof(payload) = 'object'),
  CONSTRAINT browser_site_audit_outbox_payload_identity
    CHECK (payload->>'event_id' = event_id AND payload->>'org_id' = org_id),
  CONSTRAINT browser_site_audit_outbox_attempts_bounded
    CHECK (attempts >= 0 AND attempts <= 8),
  CONSTRAINT browser_site_audit_outbox_terminal_state
    CHECK (published_at IS NULL OR dead_lettered_at IS NULL),
  CONSTRAINT browser_site_audit_outbox_dead_letter_complete
    CHECK (
      dead_lettered_at IS NULL
      OR (attempts = 8 AND published_at IS NULL AND last_error IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_browser_site_audit_outbox_pending
  ON browser_site_audit_outbox (next_attempt_at, created_at, event_id)
  WHERE published_at IS NULL AND dead_lettered_at IS NULL;

-- The runtime role inserts intents and reads them back. The publisher claims and marks rows on
-- the owner pool, and no row is deleted by the runtime role.
REVOKE ALL ON browser_site_audit_outbox FROM org_core_app;
GRANT SELECT, INSERT ON browser_site_audit_outbox TO org_core_app;

ALTER TABLE browser_site_audit_outbox ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS browser_site_audit_outbox_rls_isolation ON browser_site_audit_outbox;
CREATE POLICY browser_site_audit_outbox_rls_isolation
  ON browser_site_audit_outbox
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
