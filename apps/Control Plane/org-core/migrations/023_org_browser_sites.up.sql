-- WS21-S3 (DEC-19): the agent-browser site policy of record.
--
-- org-core stores which hosts an organisation's agent browser may reach and at
-- what level, and each member's own remembered choice for a host. The broker
-- pulls the resolved answer by revision and enforces it at grant mint; Quarry
-- stays the final enforcer. Nothing here enforces anything by itself.
--
-- Everything here is a hostname, a level, a revision and an actor id. No URL,
-- path, query, page content or credential is stored, so the tables carry no
-- retention class of their own. They are organisation configuration and go with
-- the organisation: every table cascades from organizations(id), which is how
-- gdpr_hard_delete_organization() (003) removes them without being redefined.

-- One row per organisation: the mode, and the revision that every admin write
-- moves. A missing row is revision 0 and open mode, so an organisation that never
-- wrote a policy gets the default (open web minus the block list).
CREATE TABLE IF NOT EXISTS org_browser_site_policy (
  org_id     TEXT        PRIMARY KEY REFERENCES organizations(id) ON DELETE CASCADE,
  mode       TEXT        NOT NULL CHECK (mode IN ('open', 'allowlist')),
  revision   BIGINT      NOT NULL CHECK (revision > 0),
  updated_by TEXT        NOT NULL CHECK (btrim(updated_by) <> ''),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- The admin's per-host rules. block always wins; read and act are the ceiling a
-- user's remembered choice cannot exceed. Hosts are the canonical form the
-- service writes (lower-case ASCII labels, no port, no dot at either end); the
-- check is a backstop for a writer that is not the service.
CREATE TABLE IF NOT EXISTS org_browser_site_rules (
  org_id TEXT NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  host   TEXT NOT NULL CHECK (
    length(host) BETWEEN 3 AND 253
    AND host ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+$'
  ),
  rule   TEXT NOT NULL CHECK (rule IN ('block', 'read', 'act')),
  note   TEXT NOT NULL DEFAULT '' CHECK (length(note) <= 200),
  PRIMARY KEY (org_id, host)
);

-- A member's own choice for one host, at one level, until expires_at. The
-- default lifetime is 90 days; the service refuses a longer one. At most 200 rows
-- per member, counted by the service under the member's advisory lock.
CREATE TABLE IF NOT EXISTS user_browser_site_prefs (
  org_id     TEXT        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  user_id    TEXT        NOT NULL CHECK (btrim(user_id) <> ''),
  host       TEXT        NOT NULL CHECK (
    length(host) BETWEEN 3 AND 253
    AND host ~ '^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?(\.[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?)+$'
  ),
  level      TEXT        NOT NULL CHECK (level IN ('none', 'read', 'act')),
  expires_at TIMESTAMPTZ NOT NULL DEFAULT (NOW() + INTERVAL '90 days'),
  created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (org_id, user_id, host)
);

CREATE INDEX IF NOT EXISTS idx_user_browser_site_prefs_expiry
  ON user_browser_site_prefs (expires_at);

-- Append-only record of every policy write and every member pref write or delete:
-- who, at which revision, and the change before and after. A pref change carries
-- the host and level only. org_core_app can insert and read it and nothing else.
CREATE TABLE IF NOT EXISTS org_browser_site_history (
  id              BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  org_id          TEXT        NOT NULL REFERENCES organizations(id) ON DELETE CASCADE,
  change_kind     TEXT        NOT NULL CHECK (change_kind IN ('policy', 'user_pref')),
  revision        BIGINT      NOT NULL CHECK (revision >= 0),
  actor_id        TEXT        NOT NULL CHECK (btrim(actor_id) <> ''),
  subject_user_id TEXT,
  before_set      JSONB       NOT NULL,
  after_set       JSONB       NOT NULL,
  created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  CONSTRAINT org_browser_site_history_subject CHECK (
    (change_kind = 'policy' AND subject_user_id IS NULL)
    OR (change_kind = 'user_pref' AND subject_user_id IS NOT NULL)
  )
);

CREATE INDEX IF NOT EXISTS idx_org_browser_site_history_org
  ON org_browser_site_history (org_id, id);

GRANT SELECT, INSERT, UPDATE ON org_browser_site_policy TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON org_browser_site_rules TO org_core_app;
GRANT SELECT, INSERT, UPDATE, DELETE ON user_browser_site_prefs TO org_core_app;
GRANT SELECT, INSERT ON org_browser_site_history TO org_core_app;
GRANT USAGE, SELECT ON SEQUENCE org_browser_site_history_id_seq TO org_core_app;

ALTER TABLE org_browser_site_policy ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_browser_site_policy_rls_isolation ON org_browser_site_policy;
CREATE POLICY org_browser_site_policy_rls_isolation
  ON org_browser_site_policy
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE org_browser_site_rules ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_browser_site_rules_rls_isolation ON org_browser_site_rules;
CREATE POLICY org_browser_site_rules_rls_isolation
  ON org_browser_site_rules
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE user_browser_site_prefs ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS user_browser_site_prefs_rls_isolation ON user_browser_site_prefs;
CREATE POLICY user_browser_site_prefs_rls_isolation
  ON user_browser_site_prefs
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));

ALTER TABLE org_browser_site_history ENABLE ROW LEVEL SECURITY;
DROP POLICY IF EXISTS org_browser_site_history_rls_isolation ON org_browser_site_history;
CREATE POLICY org_browser_site_history_rls_isolation
  ON org_browser_site_history
  USING (org_id = current_setting('app.current_org', true))
  WITH CHECK (org_id = current_setting('app.current_org', true));
