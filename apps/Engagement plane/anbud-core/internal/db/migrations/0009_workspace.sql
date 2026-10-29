-- 0009_workspace.sql
-- What an organisation keeps while it works a tender list: its company profile, where each tender stands
-- in its pipeline, and the searches it saves. Tenant data, like the uploads of 0007 and not evidence: a row
-- is one organisation's, row level security keeps every query to the organisation the session names
-- (set_config('anbud.org_id', ...), in each transaction), and the application may change these rows,
-- which the evidence tables never admit.
--
-- The organisation is an id the gateway authenticated (X-Org-ID), not a name. The profile is what the user
-- typed: company numbers and codes, never a person's contact, which the parser does not read and the
-- profile does not ask for. A tender is named by its public id (internal/noticeid).
--
-- Privileges are stated in tenant_privileges.sql, the file that holds what the application may do on the
-- tenant schema. Forward-only; runs inside the caller's transaction (migrate.go, applyOne).

CREATE TABLE tenant.workspace_profiles (
    org_id      TEXT        PRIMARY KEY CHECK (org_id ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$'),
    profile     JSONB       NOT NULL CHECK (jsonb_typeof(profile) = 'object' AND octet_length(profile::text) <= 16384),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by  TEXT        NOT NULL CHECK (char_length(updated_by) BETWEEN 1 AND 128)
);

CREATE TABLE tenant.pipeline_items (
    org_id      TEXT        NOT NULL CHECK (org_id ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$'),
    notice_id   TEXT        NOT NULL CHECK (char_length(notice_id) BETWEEN 1 AND 64),
    stage       TEXT        NOT NULL CHECK (stage IN ('inbox', 'review', 'working', 'sent', 'hidden')),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_by  TEXT        NOT NULL CHECK (char_length(updated_by) BETWEEN 1 AND 128),
    PRIMARY KEY (org_id, notice_id)
);

CREATE TABLE tenant.saved_searches (
    id          BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    org_id      TEXT        NOT NULL CHECK (org_id ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$'),
    name        TEXT        NOT NULL CHECK (char_length(name) BETWEEN 1 AND 120),
    query       JSONB       NOT NULL CHECK (jsonb_typeof(query) = 'object' AND octet_length(query::text) <= 8192),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by  TEXT        NOT NULL CHECK (char_length(created_by) BETWEEN 1 AND 128)
);
CREATE INDEX saved_searches_by_org ON tenant.saved_searches (org_id, id);

ALTER TABLE tenant.workspace_profiles ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant.pipeline_items ENABLE ROW LEVEL SECURITY;
ALTER TABLE tenant.saved_searches ENABLE ROW LEVEL SECURITY;

-- One policy for every command, as tender_uploads has: a row is the session's organisation's, or nobody's.
CREATE POLICY workspace_profiles_own_organisation ON tenant.workspace_profiles
    USING (org_id = (SELECT current_setting('anbud.org_id', true)))
    WITH CHECK (org_id = (SELECT current_setting('anbud.org_id', true)));
CREATE POLICY pipeline_items_own_organisation ON tenant.pipeline_items
    USING (org_id = (SELECT current_setting('anbud.org_id', true)))
    WITH CHECK (org_id = (SELECT current_setting('anbud.org_id', true)));
CREATE POLICY saved_searches_own_organisation ON tenant.saved_searches
    USING (org_id = (SELECT current_setting('anbud.org_id', true)))
    WITH CHECK (org_id = (SELECT current_setting('anbud.org_id', true)));

COMMENT ON TABLE tenant.workspace_profiles IS 'One organisation''s company profile as the tender matcher reads it (codes, regions, keywords, value range). Row level security: the session''s organisation only.';
COMMENT ON TABLE tenant.pipeline_items IS 'Where each tender stands in one organisation''s pipeline. The notice is named by its public id. Row level security: the session''s organisation only.';
COMMENT ON TABLE tenant.saved_searches IS 'Searches an organisation saved from the tender list: a name and the filters. Row level security: the session''s organisation only.';
