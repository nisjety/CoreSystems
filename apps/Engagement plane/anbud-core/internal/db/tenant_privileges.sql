-- tenant_privileges.sql
-- The application role's privileges on the schema tenant (migration 0007, docs/ROLES.md), stated
-- again after every migration run, in the same transaction, by the role that owns the schema, like
-- privileges.sql and for the same reason. It is a file of its own because the tenant schema is not
-- evidence: privileges.sql holds what the role model gives on the evidence tables, and a test guards it
-- as exactly that. This file holds what a tenant's uploads need, and a test guards it too.
--
-- The application selects and deletes the uploads of one organisation at a time (row level security,
-- in the migration, keeps it to one), inserts them, and may run the one function that removes what has
-- expired. It inserts every column but id and created_at: the identity and the clock are the
-- database's, so a session cannot take a future id or date a row into the future (which would also
-- carry its expiry past the bound the table checks). It may not update a row, truncate the table,
-- change it or grant on it. Nobody else is granted anything on the schema: not PUBLIC, and not the DBA
-- group, which has no business with a tenant's data. That is stated here and not only left unsaid, so
-- that a grant made by hand to either is taken back at the next start.
--
-- The schema is not in the loop of privileges.sql, which walks schema public alone. Nothing happens
-- unless the role and the schema exist and the role that runs this owns the schema, which is how a
-- database without the role model migrates as it always did, and how one whose schema another role made
-- (a service run with a superuser DSN, before scripts/roles.sql took the schema over) is left for
-- roles.sql to take over, instead of stopping every migration run with a permission error. Within the
-- schema only the relations and functions this role owns are touched, for the same reason: PostgreSQL
-- refuses a REVOKE on an object the caller holds nothing on, which would roll the whole migration back.

DO $tenant$
DECLARE
    obj record;
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'anbud_app')
       AND EXISTS (SELECT 1 FROM pg_namespace n WHERE n.nspname = 'tenant' AND pg_has_role(current_user, n.nspowner, 'USAGE')) THEN
        -- Nobody but the application, the owner and the function's definer has anything here.
        REVOKE ALL ON SCHEMA tenant FROM PUBLIC;
        IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'anbud_dba') THEN
            REVOKE ALL ON SCHEMA tenant FROM anbud_dba;
        END IF;
        FOR obj IN
            SELECT format('%I.%I', n.nspname, c.relname) AS name
              FROM pg_class c
              JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'tenant'
               AND c.relkind IN ('r', 'p', 'v', 'm', 'f')
               AND pg_has_role(current_user, c.relowner, 'USAGE')
             ORDER BY c.relname
        LOOP
            EXECUTE format('REVOKE ALL ON TABLE %s FROM PUBLIC, anbud_app', obj.name);
            IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'anbud_dba') THEN
                EXECUTE format('REVOKE ALL ON TABLE %s FROM anbud_dba', obj.name);
            END IF;
        END LOOP;
        FOR obj IN
            SELECT format('%I.%I(%s)', n.nspname, p.proname, pg_get_function_identity_arguments(p.oid)) AS name
              FROM pg_proc p
              JOIN pg_namespace n ON n.oid = p.pronamespace
             WHERE n.nspname = 'tenant'
               AND p.prokind IN ('f', 'p')
               AND pg_has_role(current_user, p.proowner, 'USAGE')
             ORDER BY p.proname
        LOOP
            EXECUTE format('REVOKE ALL ON ROUTINE %s FROM PUBLIC, anbud_app', obj.name);
            IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'anbud_dba') THEN
                EXECUTE format('REVOKE ALL ON ROUTINE %s FROM anbud_dba', obj.name);
            END IF;
        END LOOP;

        GRANT USAGE ON SCHEMA tenant TO anbud_app;
        GRANT SELECT, DELETE ON tenant.tender_uploads TO anbud_app;
        GRANT INSERT (org_id, notice_id, uploaded_by, archive_name, archive_sha256, archive_bytes, documents, findings, read_count, unread_count, findings_dropped, expires_at) ON tenant.tender_uploads TO anbud_app;
        GRANT EXECUTE ON FUNCTION tenant.purge_expired_uploads() TO anbud_app;
        GRANT SELECT, INSERT, UPDATE ON tenant.workspace_profiles TO anbud_app;
        GRANT SELECT, INSERT, UPDATE ON tenant.workspace_seen TO anbud_app;
        GRANT SELECT, INSERT, UPDATE, DELETE ON tenant.pipeline_items TO anbud_app;
        GRANT SELECT, INSERT, DELETE ON tenant.saved_searches TO anbud_app;
        GRANT USAGE ON SEQUENCE tenant.saved_searches_id_seq TO anbud_app;
    END IF;
END
$tenant$;
