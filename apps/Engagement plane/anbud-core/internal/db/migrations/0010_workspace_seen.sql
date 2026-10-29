-- 0010_workspace_seen.sql
-- When an organisation last marked its tenders as seen. The tender pages count the tenders published since, so
-- the organisation sees what is new. Tenant data, like the profile of 0009: one row per organisation, and row
-- level security keeps every query to the organisation the session names (set_config('anbud.org_id', ...)).
--
-- Privileges are stated in tenant_privileges.sql. Forward-only; runs inside the caller's transaction.

CREATE TABLE tenant.workspace_seen (
    org_id      TEXT        PRIMARY KEY CHECK (org_id ~ '^[A-Za-z0-9][A-Za-z0-9_.:-]{0,127}$'),
    seen_at     TIMESTAMPTZ NOT NULL,
    updated_by  TEXT        NOT NULL CHECK (char_length(updated_by) BETWEEN 1 AND 128)
);

ALTER TABLE tenant.workspace_seen ENABLE ROW LEVEL SECURITY;

CREATE POLICY workspace_seen_own_organisation ON tenant.workspace_seen
    USING (org_id = (SELECT current_setting('anbud.org_id', true)))
    WITH CHECK (org_id = (SELECT current_setting('anbud.org_id', true)));

COMMENT ON TABLE tenant.workspace_seen IS 'When one organisation last marked its tenders as seen. Row level security: the session''s organisation only.';
