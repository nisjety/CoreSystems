-- privileges.sql
-- The application role's privileges on the tables (docs/ROLES.md). ApplyMigrations runs
-- this after every migration run, in the same transaction, as the role that owns the
-- tables, so it is stated again at every start and never goes stale.
--
-- Evidence (raw_search_pages, raw_documents, download_attempts) is read and inserted,
-- never changed. The ledger (sync_runs, sync_days) is also updated, because a run
-- has to finish and a day has to move on; that is the only UPDATE there is. Nothing is
-- ever deleted or truncated, no table is referenced or given a trigger by this role,
-- and the migration bookkeeping is the owner's alone.
--
-- A table that a later migration adds keeps what scripts/roles.sql gives by default
-- (read and insert), except that the privileges that change data or triggers are taken
-- from the application on it as on every other relation (update, delete, truncate,
-- references, trigger, and maintain on PostgreSQL 17). A later table the application
-- must update is therefore named in the last GRANT below, not in the migration that
-- creates it: this script runs after every migration, so it would undo a grant made there.
--
-- A REVOKE on a table also takes the privilege from every column of it, so a grant on a
-- single column made by hand (GRANT UPDATE (content_type) ON raw_documents TO anbud_app)
-- goes with the table-level REVOKE below. TestColumnPrivilegesGrantedByHandAreTakenBack
-- reads it back with has_any_column_privilege.
--
-- Table names are not qualified: the migrations create them in the schema this session
-- resolves, public, and so does this.
-- Only relations whose privileges this session may change are touched. PostgreSQL
-- refuses a REVOKE on a relation the caller holds no privilege on, which would roll the
-- whole migration back at every start, so a view an extension installed in public, or a
-- table a superuser made by hand, is left alone.
-- Nothing happens unless the role exists, so a database without the role model
-- is migrated exactly as before.

DO $privileges$
DECLARE
    rel   record;
    taken text := 'UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER';
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'anbud_app') THEN
        IF current_setting('server_version_num')::int >= 170000 THEN
            taken := taken || ', MAINTAIN';
        END IF;
        FOR rel IN
            SELECT format('%I.%I', n.nspname, c.relname) AS name, c.relname AS relname
              FROM pg_class c
              JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = 'public'
               AND c.relkind IN ('r', 'p', 'v', 'm', 'f')
               AND pg_has_role(current_user, c.relowner, 'USAGE')
             ORDER BY c.relname
        LOOP
            EXECUTE format('REVOKE %s ON TABLE %s FROM anbud_app',
                           CASE WHEN rel.relname = 'schema_migrations' THEN 'ALL PRIVILEGES' ELSE taken END,
                           rel.name);
        END LOOP;
        GRANT SELECT, INSERT ON raw_search_pages, raw_documents, download_attempts, sync_runs, sync_days TO anbud_app;
        GRANT UPDATE ON sync_runs, sync_days TO anbud_app;
        GRANT SELECT, INSERT, UPDATE ON catalogue_projection TO anbud_app;
        GRANT SELECT ON catalogue_generation TO anbud_app;
        -- Migration 0006 (redaction): the application reads the tombstones and the view of
        -- the bodies that are still held, and never inserts into either; only
        -- anbud_redact_raw_document() writes a tombstone. Stated here, and not left to the
        -- defaults, so a database migrated before the roles existed ends the same.
        GRANT SELECT ON raw_redactions, raw_documents_live TO anbud_app;
        REVOKE INSERT ON raw_redactions, raw_documents_live FROM anbud_app;
    END IF;
END
$privileges$;
