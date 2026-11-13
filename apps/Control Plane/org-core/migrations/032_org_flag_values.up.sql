-- W5-079-b (F-t3-079): the organisation's own values for the org-overridable
-- presentation flags.
--
-- The registry (apps/Control Plane/flag-registry) says which flags an organisation may
-- set. org-core holds the values; the closed list of flag names lives in
-- internal/org/org_flags.go. Presentation only: no security gate reads these rows.
--
-- A value is "true" or "false" for a switch, or one of a choice flag's closed values,
-- never free text. A missing row is the flag's default at revision 0. Every write moves
-- the row's revision on by one. The runtime role never deletes a row, so a revision
-- cannot reset.
--
-- Everything here is a flag name, a value, a revision and an actor id. No content is
-- stored, so a Zero Data Retention organisation's records hold nothing to retain. The
-- table cascades from organizations(id), which is how gdpr_hard_delete_organization()
-- (003) removes it without being redefined.
--
-- Every statement is idempotent (IF NOT EXISTS / DROP ... IF EXISTS), so a re-run on a
-- database that already has the tables changes nothing.

CREATE TABLE IF NOT EXISTS org_flag_values (
  org_id     TEXT        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  name       TEXT        NOT NULL CHECK (name ~ '^[A-Z][A-Z0-9_]{0,127}$'),
  value      TEXT        NOT NULL CHECK (btrim(value) <> '' AND length(value) <= 128),
  revision   BIGINT      NOT NULL CHECK (revision > 0),
  updated_by TEXT        NOT NULL CHECK (btrim(updated_by) <> ''),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (org_id, name)
);

-- Least privilege for org_core_app (009's default privileges grant all four on every
-- new table; see 024). The runtime role reads and inserts rows, and updates only the
-- columns a write changes: the flag name and the organisation never change, and no row
-- is deleted.
REVOKE ALL ON org_flag_values FROM org_core_app;
GRANT SELECT, INSERT ON org_flag_values TO org_core_app;
GRANT UPDATE (value, revision, updated_by, updated_at) ON org_flag_values TO org_core_app;

ALTER TABLE org_flag_values ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_flag_values_rls_isolation ON org_flag_values;
CREATE POLICY org_flag_values_rls_isolation
  ON org_flag_values
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

-- Durable audit intent for every flag write, in the shape of
-- organization_web_policy_audit_outbox (027). The subject is pinned to the flag event
-- and the scoped write transaction inserts beside the change it records. The payload is
-- content-free: the flag name, its value, the revision and the actor.
CREATE TABLE IF NOT EXISTS org_flag_audit_outbox (
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
  CONSTRAINT org_flag_audit_outbox_event_id_nonempty
    CHECK (length(event_id) BETWEEN 1 AND 128 AND event_id = btrim(event_id)),
  CONSTRAINT org_flag_audit_outbox_org_id_nonempty
    CHECK (btrim(org_id) <> ''),
  CONSTRAINT org_flag_audit_outbox_subject_authority
    CHECK (subject = 'verevon.audit.v2.control.org-core.org_flag_changed'),
  CONSTRAINT org_flag_audit_outbox_payload_object
    CHECK (jsonb_typeof(payload) = 'object'),
  CONSTRAINT org_flag_audit_outbox_payload_identity
    CHECK (payload->>'event_id' = event_id AND payload->>'org_id' = org_id),
  CONSTRAINT org_flag_audit_outbox_attempts_bounded
    CHECK (attempts >= 0 AND attempts <= 8),
  CONSTRAINT org_flag_audit_outbox_terminal_state
    CHECK (published_at IS NULL OR dead_lettered_at IS NULL),
  CONSTRAINT org_flag_audit_outbox_dead_letter_complete
    CHECK (
      dead_lettered_at IS NULL
      OR (attempts = 8 AND published_at IS NULL AND last_error IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_org_flag_audit_outbox_pending
  ON org_flag_audit_outbox (next_attempt_at, created_at, event_id)
  WHERE published_at IS NULL AND dead_lettered_at IS NULL;

-- The publisher claims and marks rows on the owner pool, never as org_core_app.
REVOKE ALL ON org_flag_audit_outbox FROM org_core_app;
GRANT SELECT, INSERT ON org_flag_audit_outbox TO org_core_app;

ALTER TABLE org_flag_audit_outbox ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_flag_audit_outbox_rls_isolation ON org_flag_audit_outbox;
CREATE POLICY org_flag_audit_outbox_rls_isolation
  ON org_flag_audit_outbox
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
