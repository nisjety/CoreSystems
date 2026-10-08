-- Z-12: zero-data-retention (ZDR) capability of a user-owned model connection, and the
-- ledger of acknowledgements users give when a connection cannot do ZDR.
--
-- Forward-only (there are no down migrations in this service). Two tables, deliberately
-- NOT columns of integration_connections:
--
--   * integration_connection_zdr holds the CURRENT assessment of a connection (one row per
--     connection: capability supported | not_supported | cannot_verify, what it rests on,
--     and the digest an acknowledgement is bound to). It is written only by
--     integration-corev2's assessor, never by the generic connection upsert or patch paths,
--     so a read-modify-write that holds a stale connection cannot write a stale digest back
--     and no client-controlled field can set a capability. Derived state: it follows the
--     connection row if that is ever hard-deleted.
--
--   * integration_connection_zdr_acknowledgements is the LEDGER of what a user was shown and
--     accepted, bound to the digest of the assessment. A row is inserted once and updated at
--     most once afterwards, to record its revocation. It has NO foreign key to
--     integration_connections: connections are soft-deleted (deleted_at) and a compliance
--     record kept for a year must not depend on, or cascade away with, any row it names (the
--     ON DELETE CASCADE of 0004_platform_primitives.sql is exactly what must not happen here).
--     The BEFORE UPDATE guard below makes "append-only apart from revocation" a database fact.
--
-- Nothing reads these tables yet; the acknowledgement API, the decision endpoint and the
-- enforcers that consume them ship later. A connection without a row is unassessed and every
-- reader must treat it as cannot_verify. Existing connections are assessed by the service's
-- idempotent start-up reconcile (internal/api/zdr_assessment.go), which computes the digest
-- with the same code that computes every later one; this migration deliberately holds no copy
-- of the capability rules.

CREATE TABLE IF NOT EXISTS integration_connection_zdr (
  connection_id TEXT PRIMARY KEY REFERENCES integration_connections(id) ON DELETE CASCADE,
  provider_key TEXT NOT NULL,
  auth_mode TEXT NOT NULL,
  capability TEXT NOT NULL CHECK (capability IN ('supported', 'not_supported', 'cannot_verify')),
  basis TEXT NOT NULL CHECK (basis IN (
    'verevon_attested_deployment',
    'provider_terms_known',
    'provider_by_contract',
    'customer_asserted',
    'not_assessed'
  )),
  basis_ref TEXT NOT NULL,
  catalog_version INTEGER NOT NULL CHECK (catalog_version >= 1),
  digest TEXT NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
  checked_at TIMESTAMPTZ NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS integration_connection_zdr_capability_idx
  ON integration_connection_zdr (capability, catalog_version);

CREATE TABLE IF NOT EXISTS integration_connection_zdr_acknowledgements (
  id TEXT PRIMARY KEY,
  organization_id TEXT NOT NULL,
  -- Deliberately not a foreign key (see above).
  connection_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  provider_key TEXT NOT NULL,
  capability TEXT NOT NULL CHECK (capability IN ('not_supported', 'cannot_verify')),
  basis TEXT NOT NULL CHECK (basis IN (
    'verevon_attested_deployment',
    'provider_terms_known',
    'provider_by_contract',
    'customer_asserted',
    'not_assessed'
  )),
  catalog_version INTEGER NOT NULL CHECK (catalog_version >= 1),
  digest TEXT NOT NULL CHECK (digest ~ '^[0-9a-f]{64}$'),
  text_version TEXT NOT NULL CHECK (text_version <> ''),
  locale TEXT NOT NULL CHECK (locale IN ('no', 'en')),
  text_sha256 TEXT NOT NULL CHECK (text_sha256 ~ '^[0-9a-f]{64}$'),
  text_snapshot TEXT NOT NULL CHECK (text_snapshot <> ''),
  org_zdr_at_ack BOOLEAN NOT NULL,
  acknowledged_via TEXT NOT NULL CHECK (acknowledged_via IN ('connect', 'first_use', 'settings')),
  request_id TEXT NOT NULL DEFAULT '',
  acknowledged_at TIMESTAMPTZ NOT NULL DEFAULT now(),
  revoked_at TIMESTAMPTZ,
  revoke_reason TEXT NOT NULL DEFAULT '' CHECK (revoke_reason IN (
    '',
    'capability_changed',
    'connection_removed',
    'owner_revoked',
    'text_version_superseded'
  )),
  CHECK ((revoked_at IS NULL) = (revoke_reason = ''))
);

-- At most one ACTIVE acknowledgement per connection and digest: this is what makes an
-- acknowledgement replay idempotent. A revoked row leaves the index, so the user can
-- acknowledge the same assessment again after revoking.
CREATE UNIQUE INDEX IF NOT EXISTS integration_connection_zdr_ack_active_idx
  ON integration_connection_zdr_acknowledgements (connection_id, digest)
  WHERE revoked_at IS NULL;

CREATE INDEX IF NOT EXISTS integration_connection_zdr_ack_connection_idx
  ON integration_connection_zdr_acknowledgements (connection_id, acknowledged_at DESC);

CREATE INDEX IF NOT EXISTS integration_connection_zdr_ack_org_idx
  ON integration_connection_zdr_acknowledgements (organization_id, acknowledged_at DESC);

-- Append-only apart from revocation: a row may be updated only to set revoked_at and
-- revoke_reason, once. Deleting rows (retention expiry, an organisation purge) is a separate,
-- explicit operation and is not blocked here.
CREATE OR REPLACE FUNCTION integration_connection_zdr_ack_guard()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF OLD.revoked_at IS NOT NULL THEN
    RAISE EXCEPTION 'a revoked zdr acknowledgement is final'
      USING ERRCODE = 'check_violation';
  END IF;
  IF (to_jsonb(NEW) - 'revoked_at' - 'revoke_reason') IS DISTINCT FROM (to_jsonb(OLD) - 'revoked_at' - 'revoke_reason') THEN
    RAISE EXCEPTION 'only the revocation columns of a zdr acknowledgement may change'
      USING ERRCODE = 'check_violation';
  END IF;
  RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS integration_connection_zdr_ack_guard ON integration_connection_zdr_acknowledgements;
CREATE TRIGGER integration_connection_zdr_ack_guard
  BEFORE UPDATE ON integration_connection_zdr_acknowledgements
  FOR EACH ROW EXECUTE FUNCTION integration_connection_zdr_ack_guard();
