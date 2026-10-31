-- WS06-S7: SIEM destination configuration per organisation.
--
-- org-core stores where an organisation wants its audit events copied to and
-- the secret that authenticates the copy; audit-core's exporter (S8) reads the
-- enabled rows through a narrow internal route and delivers. At most three
-- destinations per organisation.
--
-- The secret is sealed (AES-256-GCM, internal/org/siem_seal.go) before it is
-- written and is write-only through the API: this table holds ciphertext and the
-- id of the key that sealed it, never the secret. Nothing here is audit content;
-- it is organisation configuration and goes with the organisation, so the table
-- cascades from organizations(id) and gdpr_hard_delete_organization() (003)
-- removes it without being redefined.
CREATE TABLE IF NOT EXISTS siem_destinations (
  id                TEXT        PRIMARY KEY DEFAULT gen_random_uuid()::TEXT,
  org_id            TEXT        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  kind              TEXT        NOT NULL CHECK (kind IN ('ndjson_webhook', 'splunk_hec')),
  -- https only, no userinfo, no private-range literal: the service validates the
  -- same rules; the dial-time guard in the exporter (S8) is the real SSRF check.
  url               TEXT        NOT NULL CHECK (length(url) BETWEEN 1 AND 2048 AND url LIKE 'https://%'),
  secret_ciphertext BYTEA       NOT NULL CHECK (octet_length(secret_ciphertext) BETWEEN 29 AND 1024),
  secret_key_id     TEXT        NOT NULL CHECK (secret_key_id ~ '^[A-Za-z0-9._-]{1,32}$'),
  enabled           BOOLEAN     NOT NULL DEFAULT TRUE,
  event_filter      JSONB       NOT NULL DEFAULT '{}'::jsonb CHECK (jsonb_typeof(event_filter) = 'object'),
  -- Compare-and-set counter for PATCH: a stale editor gets 409, never a silent
  -- overwrite. Starts at 1 and moves only through the service.
  revision          BIGINT      NOT NULL DEFAULT 1 CHECK (revision > 0),
  created_by        TEXT        NOT NULL CHECK (btrim(created_by) <> ''),
  created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_siem_destinations_org ON siem_destinations (org_id, created_at, id);
CREATE INDEX IF NOT EXISTS idx_siem_destinations_enabled_updated
  ON siem_destinations (updated_at, id) WHERE enabled;

-- The at-most-three rule. The service counts under the organisation lifecycle
-- advisory lock; this trigger takes the same lock, so it is a backstop that
-- serialises a writer that is not the service rather than a second opinion.
CREATE OR REPLACE FUNCTION siem_destinations_enforce_limit() RETURNS trigger AS $$
BEGIN
  PERFORM pg_advisory_xact_lock(hashtextextended(NEW.org_id, 0));
  IF (SELECT COUNT(*) FROM siem_destinations WHERE org_id = NEW.org_id) >= 3 THEN
    RAISE EXCEPTION 'siem destination limit reached for organization' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS siem_destinations_limit ON siem_destinations;
CREATE TRIGGER siem_destinations_limit
  BEFORE INSERT ON siem_destinations
  FOR EACH ROW EXECUTE FUNCTION siem_destinations_enforce_limit();

-- Durable audit intent for every destination change, in the shape of
-- organization_gdpr_audit_outbox (017). That table pins its subject to the
-- erasure event and withholds INSERT from org_core_app, so it is not reusable
-- for a change written inside a scoped transaction; this sibling pins the SIEM
-- subject and lets the scoped transaction insert beside the change it records.
-- The payload is content-free: host only (no URL query), never the secret.
CREATE TABLE IF NOT EXISTS organization_siem_audit_outbox (
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
  CONSTRAINT organization_siem_audit_outbox_event_id_nonempty
    CHECK (length(event_id) BETWEEN 1 AND 128 AND event_id = btrim(event_id)),
  CONSTRAINT organization_siem_audit_outbox_org_id_nonempty
    CHECK (btrim(org_id) <> ''),
  CONSTRAINT organization_siem_audit_outbox_subject_authority
    CHECK (subject = 'verevon.audit.v2.control.org-core.org_siem_destination_changed'),
  CONSTRAINT organization_siem_audit_outbox_payload_object
    CHECK (jsonb_typeof(payload) = 'object'),
  CONSTRAINT organization_siem_audit_outbox_payload_identity
    CHECK (payload->>'event_id' = event_id AND payload->>'org_id' = org_id),
  CONSTRAINT organization_siem_audit_outbox_attempts_bounded
    CHECK (attempts >= 0 AND attempts <= 8),
  CONSTRAINT organization_siem_audit_outbox_terminal_state
    CHECK (published_at IS NULL OR dead_lettered_at IS NULL),
  CONSTRAINT organization_siem_audit_outbox_dead_letter_complete
    CHECK (
      dead_lettered_at IS NULL
      OR (attempts = 8 AND published_at IS NULL AND last_error IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_organization_siem_audit_outbox_pending
  ON organization_siem_audit_outbox (next_attempt_at, created_at, event_id)
  WHERE published_at IS NULL AND dead_lettered_at IS NULL;

GRANT SELECT, INSERT, UPDATE, DELETE ON siem_destinations TO org_core_app;
GRANT SELECT, INSERT ON organization_siem_audit_outbox TO org_core_app;

ALTER TABLE siem_destinations ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS siem_destinations_rls_isolation ON siem_destinations;
CREATE POLICY siem_destinations_rls_isolation
  ON siem_destinations
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE organization_siem_audit_outbox ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS organization_siem_audit_outbox_rls_isolation ON organization_siem_audit_outbox;
CREATE POLICY organization_siem_audit_outbox_rls_isolation
  ON organization_siem_audit_outbox
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
