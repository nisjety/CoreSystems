-- WS17.E2 (X-8): per-organisation key-encryption keys for data other planes
-- encrypt (Quarry browser profiles first). The plan called this 024; 024-029 were
-- taken by the time it landed, so it is 030.
--
-- Hierarchy: a master key (ORG_CORE_KEY_MASTER_KEYS, env now, a KMS later) wraps
-- each KEK; each KEK wraps data keys that the caller stores wrapped beside its
-- ciphertext. org-core never sees the protected data, and the caller never holds
-- a KEK.
--
-- Crypto-shred: DELETE /internal/orgs/:orgId/keys sets every wrapped_kek to NULL
-- and the state to destroyed; the org hard delete (003,
-- gdpr_hard_delete_organization) removes the rows through the FK cascade without
-- being redefined. A soft delete does NOT shred. Shredding is effective once
-- backups age out.
--
-- Every statement is idempotent (IF NOT EXISTS / DROP ... IF EXISTS), so a
-- re-run on a database that already has the tables changes nothing.

CREATE TABLE IF NOT EXISTS org_key_encryption_keys (
  org_id        TEXT        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  -- 1, 2, 3 ... per org; 0 is the tombstone a destroy leaves when the org never
  -- had a KEK, so a later data-key call cannot mint one after erasure.
  kek_version   INTEGER     NOT NULL CHECK (kek_version >= 0 AND kek_version <= 1000000),
  master_key_id TEXT        CHECK (master_key_id IS NULL OR master_key_id ~ '^[a-z0-9][a-z0-9._-]{0,31}$'),
  -- nonce(12) || AES-256-GCM(master, KEK, aad = "org-core.kek|org=<org>|v=<n>|master=<id>")
  wrapped_kek   BYTEA,
  state         TEXT        NOT NULL CHECK (state IN ('active', 'retired', 'destroyed')),
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  retired_at    TIMESTAMPTZ,
  destroyed_at  TIMESTAMPTZ,
  PRIMARY KEY (org_id, kek_version),
  CONSTRAINT org_key_encryption_keys_material
    CHECK (
      (state = 'destroyed' AND wrapped_kek IS NULL AND master_key_id IS NULL AND destroyed_at IS NOT NULL)
      OR (state <> 'destroyed' AND kek_version >= 1 AND master_key_id IS NOT NULL
          AND wrapped_kek IS NOT NULL AND octet_length(wrapped_kek) = 60 AND destroyed_at IS NULL)
    ),
  CONSTRAINT org_key_encryption_keys_retired
    CHECK ((state = 'retired') = (retired_at IS NOT NULL) OR state = 'destroyed')
);

-- At most one active KEK per org.
CREATE UNIQUE INDEX IF NOT EXISTS idx_org_key_encryption_keys_one_active
  ON org_key_encryption_keys (org_id) WHERE state = 'active';

-- One known-plaintext canary per master key id, sealed under that key. org-core
-- opens every canary before the key routes serve, so a missing or wrong master key
-- answers 503 keys_unavailable instead of failing a caller's decrypt. Not org
-- data: no org_id, no RLS, read and written on the owner pool only.
CREATE TABLE IF NOT EXISTS org_key_master_canaries (
  master_key_id TEXT        PRIMARY KEY CHECK (master_key_id ~ '^[a-z0-9][a-z0-9._-]{0,31}$'),
  sealed        BYTEA       NOT NULL CHECK (octet_length(sealed) BETWEEN 28 AND 256),
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Durable audit intent for KEK creation, rotation, destruction and denied
-- unwraps, in the shape of organization_web_policy_audit_outbox (027). The payload
-- is content free: org, KEK version, purpose, the calling principal and a reason
-- code; never a key, a wrapped key or a context value. Successful unwraps are
-- counted, not audited.
CREATE TABLE IF NOT EXISTS org_key_audit_outbox (
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
  CONSTRAINT org_key_audit_outbox_event_id_nonempty
    CHECK (length(event_id) BETWEEN 1 AND 128 AND event_id = btrim(event_id)),
  CONSTRAINT org_key_audit_outbox_org_id_nonempty
    CHECK (btrim(org_id) <> ''),
  CONSTRAINT org_key_audit_outbox_subject_authority
    CHECK (subject = 'verevon.audit.v2.control.org-core.org_key_custody'),
  CONSTRAINT org_key_audit_outbox_payload_object
    CHECK (jsonb_typeof(payload) = 'object'),
  CONSTRAINT org_key_audit_outbox_payload_identity
    CHECK (payload->>'event_id' = event_id AND payload->>'org_id' = org_id),
  CONSTRAINT org_key_audit_outbox_payload_no_key_material
    CHECK (NOT (payload ?| ARRAY['dek', 'dek_b64', 'wrapped_dek', 'wrapped_dek_b64', 'kek', 'wrapped_kek'])
           AND NOT ((payload->'details') ?| ARRAY['dek', 'dek_b64', 'wrapped_dek', 'wrapped_dek_b64', 'kek', 'wrapped_kek', 'context'])),
  CONSTRAINT org_key_audit_outbox_attempts_bounded
    CHECK (attempts >= 0 AND attempts <= 8),
  CONSTRAINT org_key_audit_outbox_terminal_state
    CHECK (published_at IS NULL OR dead_lettered_at IS NULL),
  CONSTRAINT org_key_audit_outbox_dead_letter_complete
    CHECK (
      dead_lettered_at IS NULL
      OR (attempts = 8 AND published_at IS NULL AND last_error IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_org_key_audit_outbox_pending
  ON org_key_audit_outbox (next_attempt_at, created_at, event_id)
  WHERE published_at IS NULL AND dead_lettered_at IS NULL;

-- Least privilege for org_core_app (009's default privileges grant all four; see
-- 024). The scoped transactions read, create, retire and shred KEK rows, never
-- delete them (the cascade runs as the owner). The canaries are owner-only. The
-- outbox is inserted beside the change and claimed on the owner pool.
REVOKE ALL ON org_key_encryption_keys FROM org_core_app;
GRANT SELECT, INSERT, UPDATE ON org_key_encryption_keys TO org_core_app;
REVOKE ALL ON org_key_master_canaries FROM org_core_app;
REVOKE ALL ON org_key_audit_outbox FROM org_core_app;
GRANT SELECT, INSERT ON org_key_audit_outbox TO org_core_app;

ALTER TABLE org_key_encryption_keys ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_key_encryption_keys_rls_isolation ON org_key_encryption_keys;
CREATE POLICY org_key_encryption_keys_rls_isolation
  ON org_key_encryption_keys
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE org_key_audit_outbox ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_key_audit_outbox_rls_isolation ON org_key_audit_outbox;
CREATE POLICY org_key_audit_outbox_rls_isolation
  ON org_key_audit_outbox
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
