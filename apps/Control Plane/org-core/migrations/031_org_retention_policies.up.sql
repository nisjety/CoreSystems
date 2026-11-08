-- EP-9 C1: the organisation's retention policy and legal holds of record.
--
-- The design reserved 030; 030 is org_key_encryption_keys (WS17.E2), so this is
-- 031.
--
-- org-core stores how long an organisation keeps personal threads and which org
-- or thread holds are active. It deletes nothing itself: session-core reads the
-- answer through POST /internal/orgs/:orgId/retention/check and sweeps (EP-9 C2,
-- C3), and a hold is never released by anything but an owner or compliance
-- member. organizations.compliance.data_retention_days (002) is not read: it has
-- no revision, no audit and an unknown writer (D-C4); this record replaces it.
--
-- Everything here is an id, a number, a hold reference and an actor id. No thread
-- content, title or message is stored, so a Zero Data Retention organisation's
-- records hold nothing to retain. Every table cascades from organizations(id),
-- which is how gdpr_hard_delete_organization() (003) removes them without being
-- redefined.
--
-- Every statement is idempotent (IF NOT EXISTS / DROP ... IF EXISTS), so a re-run
-- on a database that already has the tables changes nothing.

-- One row per organisation: the thread window and the revision every write moves.
-- A missing row is revision 0 and no window, which means keep (today's behaviour).
-- A NULL window after a write is an explicit "keep". The clock is the thread's last
-- activity (D-C2); the bounds are the same 30..3650 days the service checks.
CREATE TABLE IF NOT EXISTS org_retention_policies (
  org_id      TEXT        PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
  thread_days INTEGER     CHECK (thread_days IS NULL OR thread_days BETWEEN 30 AND 3650),
  revision    BIGINT      NOT NULL CHECK (revision > 0),
  updated_by  TEXT        NOT NULL CHECK (btrim(updated_by) <> ''),
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- A hold on the whole organisation (target_ref is the org id) or on one thread
-- (target_ref is the thread id). hold_ref is the matter or case reference the
-- placer gives, never content. A hold is released by setting released_by and
-- released_at together; it is never deleted, so the record of what was held
-- survives its release.
CREATE TABLE IF NOT EXISTS org_legal_holds (
  id          TEXT        PRIMARY KEY CHECK (id ~ '^lh_[0-9a-f]{32}$'),
  org_id      TEXT        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  scope       TEXT        NOT NULL CHECK (scope IN ('org', 'thread')),
  target_ref  TEXT        NOT NULL CHECK (length(target_ref) BETWEEN 1 AND 128 AND target_ref = btrim(target_ref)),
  hold_ref    TEXT        NOT NULL CHECK (btrim(hold_ref) <> '' AND length(hold_ref) <= 200),
  created_by  TEXT        NOT NULL CHECK (btrim(created_by) <> ''),
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  released_by TEXT,
  released_at TIMESTAMPTZ,
  CONSTRAINT org_legal_holds_org_target
    CHECK (scope <> 'org' OR target_ref = org_id),
  CONSTRAINT org_legal_holds_release_complete
    CHECK ((released_by IS NULL) = (released_at IS NULL)
           AND (released_by IS NULL OR btrim(released_by) <> ''))
);

-- At most one active hold per target: a second hold on a held thread is the same
-- hold, and the service answers it with the existing one.
CREATE UNIQUE INDEX IF NOT EXISTS idx_org_legal_holds_one_active
  ON org_legal_holds (org_id, scope, target_ref) WHERE released_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_org_legal_holds_org_created
  ON org_legal_holds (org_id, created_at DESC, id);

-- Append-only record of every policy write, hold placement and hold release: who,
-- at which revision or on which hold, and the state before and after.
-- org_core_app can insert and read it and nothing else.
CREATE TABLE IF NOT EXISTS org_retention_history (
  id           BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id       TEXT        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  change_kind  TEXT        NOT NULL CHECK (change_kind IN ('policy', 'hold_placed', 'hold_released')),
  revision     BIGINT      CHECK (revision IS NULL OR revision >= 0),
  hold_id      TEXT,
  actor_id     TEXT        NOT NULL CHECK (btrim(actor_id) <> ''),
  before_state JSONB       NOT NULL CHECK (jsonb_typeof(before_state) = 'object'),
  after_state  JSONB       NOT NULL CHECK (jsonb_typeof(after_state) = 'object'),
  created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT org_retention_history_subject CHECK (
    (change_kind = 'policy' AND revision IS NOT NULL AND hold_id IS NULL)
    OR (change_kind <> 'policy' AND hold_id IS NOT NULL)
  )
);

CREATE INDEX IF NOT EXISTS idx_org_retention_history_org
  ON org_retention_history (org_id, id);

-- Durable audit intent for every write above, in the shape of
-- organization_siem_audit_outbox (022). The subject is pinned to the retention
-- event and the scoped write transaction inserts beside the change it records. The
-- payload is content-free: window, revision, hold id, scope and target id; the
-- hold reference is not copied into it.
CREATE TABLE IF NOT EXISTS org_retention_audit_outbox (
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
  CONSTRAINT org_retention_audit_outbox_event_id_nonempty
    CHECK (length(event_id) BETWEEN 1 AND 128 AND event_id = btrim(event_id)),
  CONSTRAINT org_retention_audit_outbox_org_id_nonempty
    CHECK (btrim(org_id) <> ''),
  CONSTRAINT org_retention_audit_outbox_subject_authority
    CHECK (subject = 'verevon.audit.v2.control.org-core.org_retention_changed'),
  CONSTRAINT org_retention_audit_outbox_payload_object
    CHECK (jsonb_typeof(payload) = 'object'),
  CONSTRAINT org_retention_audit_outbox_payload_identity
    CHECK (payload->>'event_id' = event_id AND payload->>'org_id' = org_id),
  CONSTRAINT org_retention_audit_outbox_payload_no_reference
    CHECK (NOT (payload ? 'hold_ref') AND NOT ((payload->'details') ? 'hold_ref')),
  CONSTRAINT org_retention_audit_outbox_attempts_bounded
    CHECK (attempts >= 0 AND attempts <= 8),
  CONSTRAINT org_retention_audit_outbox_terminal_state
    CHECK (published_at IS NULL OR dead_lettered_at IS NULL),
  CONSTRAINT org_retention_audit_outbox_dead_letter_complete
    CHECK (
      dead_lettered_at IS NULL
      OR (attempts = 8 AND published_at IS NULL AND last_error IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_org_retention_audit_outbox_pending
  ON org_retention_audit_outbox (next_attempt_at, created_at, event_id)
  WHERE published_at IS NULL AND dead_lettered_at IS NULL;

-- Least privilege for org_core_app (009's default privileges grant all four on
-- every new table; see 024). Each table is revoked in full and granted exactly
-- what the scoped transactions run:
--
--   org_retention_policies     SELECT, INSERT, UPDATE of the written columns
--                              (never deleted: the revision must not reset)
--   org_legal_holds            SELECT, INSERT, UPDATE of the release columns only
--                              (a hold's scope, target and reference never change,
--                              and a hold is never deleted)
--   org_retention_history      SELECT, INSERT (append-only)
--   org_retention_audit_outbox SELECT, INSERT (claimed and marked by the publisher
--                              on the owner pool, never here)
REVOKE ALL ON org_retention_policies FROM org_core_app;
GRANT SELECT, INSERT ON org_retention_policies TO org_core_app;
GRANT UPDATE (thread_days, revision, updated_by, updated_at) ON org_retention_policies TO org_core_app;

REVOKE ALL ON org_legal_holds FROM org_core_app;
GRANT SELECT, INSERT ON org_legal_holds TO org_core_app;
GRANT UPDATE (released_by, released_at) ON org_legal_holds TO org_core_app;

REVOKE ALL ON org_retention_history FROM org_core_app;
GRANT SELECT, INSERT ON org_retention_history TO org_core_app;
GRANT USAGE, SELECT ON SEQUENCE org_retention_history_id_seq TO org_core_app;

REVOKE ALL ON org_retention_audit_outbox FROM org_core_app;
GRANT SELECT, INSERT ON org_retention_audit_outbox TO org_core_app;

ALTER TABLE org_retention_policies ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_retention_policies_rls_isolation ON org_retention_policies;
CREATE POLICY org_retention_policies_rls_isolation
  ON org_retention_policies
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE org_legal_holds ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_legal_holds_rls_isolation ON org_legal_holds;
CREATE POLICY org_legal_holds_rls_isolation
  ON org_legal_holds
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE org_retention_history ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_retention_history_rls_isolation ON org_retention_history;
CREATE POLICY org_retention_history_rls_isolation
  ON org_retention_history
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE org_retention_audit_outbox ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_retention_audit_outbox_rls_isolation ON org_retention_audit_outbox;
CREATE POLICY org_retention_audit_outbox_rls_isolation
  ON org_retention_audit_outbox
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
