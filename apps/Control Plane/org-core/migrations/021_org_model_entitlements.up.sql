-- WS10-S2 (ADR-0005): the chat model policy per organisation and per role.
--
-- org-core stores and resolves which chat models an organisation, and each role
-- inside it, may use; inference-core enforces the answer. A policy row is a
-- COMPLETE policy for one scope (a role override replaces the organisation
-- default rather than merging with it), so the table has no inheritance columns.
--
-- Everything here is model ids and actor ids. No prompt, message or any other
-- content is stored, so the tables carry no retention class of their own; they
-- are organisation configuration and go with the organisation: every table
-- cascades from organizations(id), which is how gdpr_hard_delete_organization()
-- (003) removes them without being redefined.
CREATE TABLE IF NOT EXISTS org_model_policies (
  org_id           TEXT  NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  scope_kind       TEXT  NOT NULL CHECK (scope_kind IN ('org', 'role')),
  -- '' for the organisation default, the lower-case role name for an override.
  scope_value      TEXT  NOT NULL DEFAULT '',
  allow_mode       TEXT  NOT NULL CHECK (allow_mode IN ('all', 'listed')),
  -- Exact model ids or 'provider:<id>' selectors, lower-cased. Never patterns:
  -- an allow glob would widen the policy silently when a model is added.
  allow            TEXT[] NOT NULL DEFAULT '{}',
  deny             TEXT[] NOT NULL DEFAULT '{}',
  max_effort       TEXT,
  min_privacy_tier TEXT,
  defaults         JSONB NOT NULL DEFAULT '{}'::jsonb,
  PRIMARY KEY (org_id, scope_kind, scope_value),
  CONSTRAINT org_model_policies_scope_shape CHECK (
    (scope_kind = 'org' AND scope_value = '')
    OR (scope_kind = 'role' AND btrim(scope_value) <> '')
  ),
  -- The service validates the same limit; this is the backstop for a writer
  -- that is not the service.
  CONSTRAINT org_model_policies_list_sizes CHECK (
    cardinality(allow) <= 500 AND cardinality(deny) <= 500
  )
);

-- One revision counter per organisation. PUT carries expected_revision and the
-- row is the compare-and-set that turns a stale writer into a 409.
CREATE TABLE IF NOT EXISTS org_model_policy_revisions (
  org_id     TEXT        PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
  revision   BIGINT      NOT NULL CHECK (revision > 0),
  updated_by TEXT        NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Append-only record of every write: who, which revision, the whole policy set
-- before and after. org_core_app can insert and read it and nothing else.
CREATE TABLE IF NOT EXISTS org_model_policy_history (
  id         BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id     TEXT        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  revision   BIGINT      NOT NULL CHECK (revision > 0),
  actor_id   TEXT        NOT NULL,
  before_set JSONB       NOT NULL,
  after_set  JSONB       NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  UNIQUE (org_id, revision)
);

GRANT SELECT, INSERT, UPDATE, DELETE ON org_model_policies TO org_core_app;
GRANT SELECT, INSERT, UPDATE ON org_model_policy_revisions TO org_core_app;
GRANT SELECT, INSERT ON org_model_policy_history TO org_core_app;
GRANT USAGE, SELECT ON SEQUENCE org_model_policy_history_id_seq TO org_core_app;

ALTER TABLE org_model_policies ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_model_policies_rls_isolation ON org_model_policies;
CREATE POLICY org_model_policies_rls_isolation
  ON org_model_policies
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE org_model_policy_revisions ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_model_policy_revisions_rls_isolation ON org_model_policy_revisions;
CREATE POLICY org_model_policy_revisions_rls_isolation
  ON org_model_policy_revisions
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE org_model_policy_history ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_model_policy_history_rls_isolation ON org_model_policy_history;
CREATE POLICY org_model_policy_history_rls_isolation
  ON org_model_policy_history
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
