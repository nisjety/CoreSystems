-- roles.sql: the role model of anbud-core (docs/ROLES.md).
--
-- It makes the append-only triggers on the evidence tables a boundary and not only a
-- safeguard against mistakes, because the role the sync runs as can no longer drop
-- them or switch them off:
--
--   anbud_migrator  LOGIN. Owns the database and every object in it. Migrations run as
--                   this role (ANBUD_MIGRATE_DSN). Not a superuser.
--   anbud_app       LOGIN. What doffin-sync and doffin-probe run as (ANBUD_DSN). Reads
--                   and inserts evidence, updates the run ledger, nothing else. Owns
--                   nothing, so it can drop and disable nothing. No TEMPORARY privilege.
--   anbud_dba       NOLOGIN group role. SELECT on every table, EXECUTE on the redaction
--                   function of migration 0006 (docs/PERSONAL-DATA.md), and TEMPORARY on
--                   the database. A human DBA gets a login role of their own, granted
--                   into this one.
--
-- The three roles are members of no role: a membership is a way in to the privileges of
-- the other role (anbud_app in anbud_migrator would be the owner, in anbud_dba it could
-- erase a notice body, in pg_execute_server_program it could run programs), so the script
-- takes back every membership that any of the three holds, whoever granted it. Roles join
-- anbud_dba, and those memberships stay.
--
-- Run it as a superuser, connected to the database it prepares. It is idempotent:
-- run it again after any change to it, and nothing that is already right changes.
--
--   psql -X -v ON_ERROR_STOP=1 -v dbname=anbud_core -d anbud_core -f scripts/roles.sql
--
-- -v dbname must be the database of the connection, and the script stops if it is not.
-- It must also start with anbud (anbud_core, anbud_test_*): the three roles have fixed
-- names and belong to the whole cluster, not to a database, so the script prepares an
-- anbud database and no other, and one anbud database per cluster is what the names allow.
-- The database must exist already (CREATE DATABASE cannot run inside a transaction); it
-- may be empty or may already be migrated by a single role, as today: the objects
-- in it are handed over to anbud_migrator.
--
-- The script sets no credential. The DBA gives the two login roles theirs out of band
-- (docs/ROLES.md), and a second run leaves them as they are.
--
-- The table privileges of anbud_app are not here. They name tables, and on a new
-- database the tables do not exist until the first migration has run, so the migration
-- runner states them after every run (internal/db/privileges.sql). What this script
-- gives anbud_app is the default for tables a later migration adds: read and insert.
--
-- Plain SQL and one psql variable, in the two forms :'dbname' (a literal) and
-- :"dbname" (an identifier). The Go tests expand exactly those two and run what is
-- left through pgx, and scripts/test-db.sh runs the file under the real psql.

BEGIN;

-- 1. The target. The variable names the database this run is meant for, and the
-- connection must be that database: a run against the wrong one changes nothing. The
-- name must also start with anbud, so a mistyped or copied command line that names
-- some other database of the cluster cannot create these cluster-wide roles there.
SELECT set_config('anbud_roles.dbname', :'dbname', true);

DO $roles$
BEGIN
    IF current_database() IS DISTINCT FROM current_setting('anbud_roles.dbname') THEN
        RAISE EXCEPTION 'roles.sql: the connection is not the database that -v dbname names';
    END IF;
    IF left(current_database(), 5) IS DISTINCT FROM 'anbud' THEN
        RAISE EXCEPTION 'roles.sql: the database name must start with anbud, because the roles have fixed names and belong to the whole cluster';
    END IF;
END
$roles$;

-- 2. The roles. A role that exists is kept, with its credential, and has its
-- attributes set again: every attribute that could cross the boundary is off by name.
DO $roles$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'anbud_migrator') THEN
        CREATE ROLE anbud_migrator LOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'anbud_app') THEN
        CREATE ROLE anbud_app LOGIN;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'anbud_dba') THEN
        CREATE ROLE anbud_dba NOLOGIN;
    END IF;
END
$roles$;

ALTER ROLE anbud_migrator LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION;
ALTER ROLE anbud_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION;
ALTER ROLE anbud_dba NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION;

-- The two login roles name the schema public and no other, so a schema named after the
-- role, or one earlier on a path, cannot stand in for it. The path cannot shut out
-- pg_catalog, which is always searched first, or pg_temp, which is searched first for
-- tables and views unless the path names it later; the application may create nothing
-- there, because it holds no TEMPORARY privilege (step 3), so a temporary table cannot
-- shadow the name of a table in a function that does not qualify it.
ALTER ROLE anbud_migrator SET search_path = public;
ALTER ROLE anbud_app SET search_path = public;

-- 2b. The three roles are members of nothing. Every membership any of them holds in any role
-- is revoked, the memberships that other roles hold in anbud_dba excepted: those are the
-- human DBAs, and they stay. On PostgreSQL 16 and later a membership records the role that
-- granted it, and a REVOKE removes only the grants made by the role that issues it (a
-- superuser counts as the bootstrap superuser), so each is revoked by its own grantor; before
-- 16 a membership is removed whoever made it.
DO $roles$
DECLARE
    m record;
BEGIN
    FOR m IN
        SELECT pg_get_userbyid(am.roleid) AS grp, pg_get_userbyid(am.member) AS member, pg_get_userbyid(am.grantor) AS grantor
          FROM pg_auth_members am
         WHERE pg_get_userbyid(am.member) IN ('anbud_migrator', 'anbud_app', 'anbud_dba')
         ORDER BY 1, 2, 3
    LOOP
        IF current_setting('server_version_num')::int >= 160000 THEN
            EXECUTE format('REVOKE %I FROM %I GRANTED BY %I CASCADE', m.grp, m.member, m.grantor);
        ELSE
            EXECUTE format('REVOKE %I FROM %I CASCADE', m.grp, m.member);
        END IF;
    END LOOP;
END
$roles$;

-- 3. The database. Its owner owns the schema public (owned by pg_database_owner from
-- PostgreSQL 15), which is how the migrator may create tables there, and PUBLIC may not.
--
-- PUBLIC holds CONNECT and TEMPORARY on every database unless it is told not to, and then
-- any role of the cluster could connect and take the budget lock of the sync (every sync
-- and probe would then exit 7), and could create a temporary table named like a table, which
-- shadows the name wherever a function does not qualify it, as the run guard of migration 0005
-- does with sync_runs. So both go. The three roles get CONNECT by name. TEMPORARY goes to the DBA
-- group only, for the session-local tables of a DBA's own work; the application has none, and
-- neither has anyone else, apart from the owner, who holds every privilege of the database.
ALTER DATABASE :"dbname" OWNER TO anbud_migrator;
REVOKE CONNECT, TEMPORARY, CREATE ON DATABASE :"dbname" FROM PUBLIC;
REVOKE CREATE ON DATABASE :"dbname" FROM anbud_dba;
REVOKE CREATE, TEMPORARY ON DATABASE :"dbname" FROM anbud_app;
GRANT CONNECT ON DATABASE :"dbname" TO anbud_migrator, anbud_app, anbud_dba;
GRANT TEMPORARY ON DATABASE :"dbname" TO anbud_dba;

-- 4. The schema. Only the migrator creates in it. The grants to the migrator are
-- explicit as well, so the schema works the same when an older cluster left it
-- owned by someone else.
REVOKE CREATE ON SCHEMA public FROM PUBLIC;
REVOKE CREATE ON SCHEMA public FROM anbud_app, anbud_dba;
GRANT USAGE, CREATE ON SCHEMA public TO anbud_migrator;
GRANT USAGE ON SCHEMA public TO anbud_app, anbud_dba;

-- 5. What already exists belongs to the migrator: the tables, views, functions and
-- procedures of the schema that are not owned by it already, and the sequences of
-- identity columns with their tables. Objects an extension owns are left alone. The schema
-- tenant of migration 0007 is taken over the same way, when it exists.
DO $roles$
DECLARE
    obj record;
BEGIN
    FOR obj IN
        SELECT CASE c.relkind WHEN 'v' THEN 'VIEW' WHEN 'm' THEN 'MATERIALIZED VIEW' ELSE 'TABLE' END AS kind,
               format('%I.%I', n.nspname, c.relname) AS name
          FROM pg_class c
          JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname IN ('public', 'tenant')
           AND c.relkind IN ('r', 'p', 'v', 'm')
           AND pg_get_userbyid(c.relowner) <> 'anbud_migrator'
           AND NOT EXISTS (SELECT 1 FROM pg_depend d
                            WHERE d.classid = 'pg_class'::regclass AND d.objid = c.oid AND d.deptype = 'e')
         ORDER BY c.relname
    LOOP
        EXECUTE format('ALTER %s %s OWNER TO anbud_migrator', obj.kind, obj.name);
    END LOOP;

    FOR obj IN
        SELECT CASE p.prokind WHEN 'p' THEN 'PROCEDURE' ELSE 'FUNCTION' END AS kind,
               format('%I.%I(%s)', n.nspname, p.proname, pg_get_function_identity_arguments(p.oid)) AS name
          FROM pg_proc p
          JOIN pg_namespace n ON n.oid = p.pronamespace
         WHERE n.nspname IN ('public', 'tenant')
           AND p.prokind IN ('f', 'p')
           AND pg_get_userbyid(p.proowner) <> 'anbud_migrator'
           AND NOT EXISTS (SELECT 1 FROM pg_depend d
                            WHERE d.classid = 'pg_proc'::regclass AND d.objid = p.oid AND d.deptype = 'e')
         ORDER BY p.proname
    LOOP
        EXECUTE format('ALTER %s %s OWNER TO anbud_migrator', obj.kind, obj.name);
    END LOOP;

    -- The schema of migration 0007 (a tenant's uploaded tender documents), when it exists: its tables
    -- and functions are in the loops above, and the schema itself is the migrator's too, or the
    -- migrator could not state the application's privileges on it.
    IF EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = 'tenant' AND pg_get_userbyid(nspowner) <> 'anbud_migrator') THEN
        ALTER SCHEMA tenant OWNER TO anbud_migrator;
    END IF;
END
$roles$;

-- 6. What a later migration adds. Tables the migrator creates from now on are read
-- and inserted by anbud_app and read by anbud_dba; nobody else gets anything by default.
-- A function it creates is executable by its owner alone until a migration grants
-- EXECUTE, so a function meant for one role (a redaction function for anbud_dba) is
-- not callable by the application. UPDATE is not a default for anyone: the two
-- ledger tables are the only tables the application updates, and the migration runner
-- grants it on them by name.
ALTER DEFAULT PRIVILEGES FOR ROLE anbud_migrator IN SCHEMA public GRANT SELECT, INSERT ON TABLES TO anbud_app;
ALTER DEFAULT PRIVILEGES FOR ROLE anbud_migrator IN SCHEMA public GRANT SELECT ON TABLES TO anbud_dba;
ALTER DEFAULT PRIVILEGES FOR ROLE anbud_migrator REVOKE EXECUTE ON FUNCTIONS FROM PUBLIC;

-- 7. The DBA group reads every table that exists and changes none, however it
-- came to hold more.
GRANT SELECT ON ALL TABLES IN SCHEMA public TO anbud_dba;
REVOKE INSERT, UPDATE, DELETE, TRUNCATE, REFERENCES, TRIGGER ON ALL TABLES IN SCHEMA public FROM anbud_dba;

-- 8. The redaction function (migration 0006). Nobody but the owner and the DBA group may run
-- it, and the DBA group may not pass that on. The migration grants EXECUTE to the group when the
-- group exists before it runs, and this block does when the script runs after the migration,
-- so either order ends the same. It does nothing while the function does not exist, which on a
-- new database is until the first migration has run.
--
-- A run takes back every direct grant that anybody else holds, a role of the cluster or PUBLIC
-- (that includes anbud_app), together with whatever was granted onward through it (CASCADE),
-- and the grant option of the group, which would let its members hand the function on. A
-- member of the group holds EXECUTE through the group, which is not a direct grant and is not
-- touched; a role that someone made a member of the group again is what step 2b undoes for the
-- three roles, and what the DBA has to look after for the others.
DO $redaction$
DECLARE
    grantee record;
BEGIN
    IF to_regprocedure('public.anbud_redact_raw_document(bigint, text, text)') IS NOT NULL THEN
        FOR grantee IN
            SELECT DISTINCT CASE WHEN a.grantee = 0 THEN 'PUBLIC' ELSE quote_ident(pg_get_userbyid(a.grantee)) END AS who
              FROM pg_proc p, aclexplode(p.proacl) a
             WHERE p.oid = to_regprocedure('public.anbud_redact_raw_document(bigint, text, text)')
               AND a.grantee <> p.proowner
               AND a.grantee <> 'anbud_dba'::regrole
             ORDER BY 1
        LOOP
            EXECUTE format('REVOKE ALL ON FUNCTION public.anbud_redact_raw_document(bigint, text, text) FROM %s CASCADE', grantee.who);
        END LOOP;
        REVOKE ALL ON FUNCTION public.anbud_redact_raw_document(bigint, text, text) FROM PUBLIC, anbud_app;
        REVOKE GRANT OPTION FOR EXECUTE ON FUNCTION public.anbud_redact_raw_document(bigint, text, text) FROM anbud_dba CASCADE;
        GRANT EXECUTE ON FUNCTION public.anbud_redact_raw_document(bigint, text, text) TO anbud_dba;
    END IF;
END
$redaction$;

COMMIT;
