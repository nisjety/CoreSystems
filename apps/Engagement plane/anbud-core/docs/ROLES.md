# Database roles (O2, least privilege)

The evidence tables (`raw_search_pages`, `raw_documents`, `download_attempts`) are
append-only: `BEFORE UPDATE OR DELETE` and `BEFORE TRUNCATE` triggers refuse every change
(migration 0002), and `evidence_run_guard()` refuses a row whose run is not `running`
(migration 0005). The one change `raw_documents` admits is the redaction of a body by
`anbud_redact_raw_document()` (migration 0006, `docs/PERSONAL-DATA.md`). Those triggers stop a
mistake. They stop an attacker, or a bug with
the wrong credential, only when the role the sync runs as cannot take them away. A role
that owns a table, or is a superuser, can `DROP TRIGGER`, `ALTER TABLE ... DISABLE TRIGGER`,
replace the guard function or `SET session_replication_role = replica`. With one DSN
whose role owns every table, which is how the service ran until now, the triggers are a
safeguard and not a boundary. Three roles make them a boundary.

## The roles

`scripts/roles.sql` creates them. No credential is in the repository; the DBA gives the
two login roles theirs (see Credentials).

| Role | Attributes | What it is for |
|---|---|---|
| `anbud_migrator` | `LOGIN`, `NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION`, `search_path = public` | Owns the database and every object in it. Migrations run as this role (`ANBUD_MIGRATE_DSN`). A deploy-time credential: it can drop the triggers. |
| `anbud_app` | same, `search_path = public` | What `doffin-sync` and `doffin-probe` run as (`ANBUD_DSN`). Owns nothing, holds no `TEMPORARY` privilege. |
| `anbud_dba` | `NOLOGIN`, the same `NO*` attributes | A group role. Human DBAs get their own login roles granted into it. It reads every table, runs the redaction function, and may create temporary tables. |

The three roles are members of no role. A membership is a way into the privileges of another
role: the application in `anbud_migrator` would be the owner, in `anbud_dba` it could run the
redaction function, in `pg_execute_server_program` it could run programs. The script takes back
every membership that any of the three holds, whoever granted it. The roles that join
`anbud_dba` are the human DBAs, and they stay.

The migrator owns the database, and a database's owner owns the schema `public`
(owned by `pg_database_owner` from PostgreSQL 15), which is how it may create tables
there while `PUBLIC` may not. The script also grants it `USAGE, CREATE` on the schema
by name, so it works on a cluster that left the schema owned by someone else. It needs
nothing else: it is not a superuser and holds no cluster-wide attribute.

The three names are fixed and belong to the cluster, not to a database. **One anbud database per
cluster**: a second one would share the roles, their passwords and their memberships. The script
enforces the half of this that it can: the database name must start with `anbud` (`anbud_core`
for the plane, `anbud_test_*` for the tests), or it stops before it changes anything.

## Privilege matrix

As built, and read back from a real server by `TestRolePrivilegeMatrix` (`make test-db`).
An empty cell is no privilege. The migrator owns every table and holds all of them.

| Relation | `anbud_app` | `anbud_dba` |
|---|---|---|
| `raw_search_pages` | SELECT, INSERT | SELECT |
| `raw_documents` | SELECT, INSERT | SELECT |
| `download_attempts` | SELECT, INSERT | SELECT |
| `sync_runs` | SELECT, INSERT, UPDATE | SELECT |
| `sync_days` | SELECT, INSERT, UPDATE | SELECT |
| `schema_migrations` | | SELECT |
| `raw_redactions` (tombstones, 0006) | SELECT | SELECT |
| `raw_documents_live` (view, 0006) | SELECT | SELECT |

| Function | `anbud_app` | `anbud_dba` |
|---|---|---|
| `anbud_redact_raw_document(bigint, text, text)` (0006) | | EXECUTE |

The owner holds `EXECUTE` too. Nobody else holds it, `PUBLIC` included, and the group does not
hold the grant option.

* No role but the owner has `DELETE`, `TRUNCATE`, `REFERENCES` or `TRIGGER` on any table.
* No role holds a privilege on a column that it does not hold on the table. The matrix is read with
  `has_table_privilege`, which cannot see a column grant, so it is read with
  `has_any_column_privilege` as well (`TestRolePrivilegeMatrix`).
* `UPDATE` is only on the two ledger tables, where a run has to finish and a day has to
  move on. It is also what `evidence_run_guard()` needs: its `SELECT ... FOR SHARE` on
  `sync_runs` is a row lock, and a row lock needs `UPDATE`. Without it every evidence
  insert would fail with `42501` (`TestRunGuardWorksForTheAppRoleBecauseTheLedgerGrantsUpdate`).
* A row lock (`SELECT ... FOR UPDATE` and its variants), a `MERGE` that updates or deletes, an
  `INSERT ... ON CONFLICT DO UPDATE` and a `LOCK TABLE` in a mode that excludes the writers need
  `UPDATE`, `DELETE` or `TRUNCATE`, which the application lacks on the evidence tables, so the server
  refuses each with `42501` (`TestAppRoleCannotChangeEvidence`).
* Schema `public`: `anbud_app` and `anbud_dba` have `USAGE` and no `CREATE`. `PUBLIC` has no `CREATE`.
* Database: all three roles `CONNECT`. `PUBLIC` holds nothing on it (no `CONNECT`, `TEMPORARY` or
  `CREATE`). `TEMPORARY`: the owner and `anbud_dba`, not `anbud_app`. `CREATE`: the owner only.
* Sequences: no role but the owner has any privilege. The identity columns need none: an
  `INSERT` into a table with a `GENERATED ALWAYS AS IDENTITY` column works with `INSERT` on
  the table alone. A `serial` column does need a grant on its sequence, so a migration
  that adds a table uses an identity column.
* Functions: a function the migrator creates later is executable by its owner alone until
  a migration grants `EXECUTE`, so a function meant for `anbud_dba` is not callable by the
  application. The existing trigger functions fire for any role that writes the table; firing a
  trigger needs no `EXECUTE`.

### Tenant data: schema `tenant` (migration 0007)

What a company uploaded as tender documents for a notice ([ARCHITECTURE.md, Tender documents](ARCHITECTURE.md#tender-documents)) is tenant data, not evidence, and lives in a schema of its own so that the append-only model of `public` stays true of everything in it. The privileges are not in the matrix above:

| Object | `anbud_app` | `anbud_dba` | `PUBLIC` |
|---|---|---|---|
| schema `tenant` | USAGE | | |
| `tenant.tender_uploads` | SELECT, DELETE, and INSERT on every column but `id` and `created_at` | | |
| `tenant.purge_expired_uploads()` | EXECUTE | | |

* **Row level security** is enabled on the table, with one policy for every command: a row is visible only when its `org_id` equals `current_setting('anbud.org_id', true)`, which the store sets with `set_config(..., true)` at the start of each transaction. With none set no row is visible, so a query that forgets its `WHERE org_id = ...` sees nothing and an insert for another organisation is refused. The application cannot switch it off: `SET row_security = off` makes the server refuse the query (`42501`), not skip the policy. The setting is one the application makes itself, so the policy guards against a query that forgets its `WHERE`, not against a connection that is stolen or SQL that is injected: whoever can run SQL as `anbud_app` can name any organisation. That is the same limit as for every privilege of the application, and the remedy is the same: keep the connection the application's.
* **It binds a role that owns nothing.** The owner and a superuser bypass row level security, which is what a database run with one DSN is (the single-DSN development setup, and every database before the role model). There the isolation is the store's own `WHERE` clauses and the table's checks. This is the same limit as the append-only triggers have, and the same remedy: the role model.
* **The application cannot choose a row's identity or its clock.** It inserts every column but `id` and `created_at`, so the identity is the database's and the time is the server's (`TestTheAppRoleCannotChooseTheIdentityOrTheClock`). A session cannot date a row into the future, which would carry its expiry past the bound of the table: nothing is held for longer than two years and two days, counted in hours, so that a retention the service may ask for (730 days) passes in a leap year and across a change of clock (`TestTheTwoYearBoundHoldsWhateverTheTimeZone`).
* Storing an upload with limits takes a transaction advisory lock of the organisation (`pg_advisory_xact_lock(int, int)`, a class of its own and the organisation as the keys), which any role may take and which is gone when the transaction ends. It is a lock with two keys, so it is never the same lock as the ones with one key that the sync and the migrations take, and it names no row and no table: the application still cannot take a row lock (below). The transaction asks for `READ COMMITTED`, because the count after the wait has to see what the upload before it committed (`TestTheLimitHoldsWhateverIsolationTheRoleDefaultsTo`).
* The DBA group and `PUBLIC` have **nothing** on the schema: a tenant's documents are not a DBA's to read in the course of looking after evidence. A DBA who needs a tenant's rows (an erasure request, [PERSONAL-DATA.md](PERSONAL-DATA.md) section 9) acts as the owner, on purpose.
* The application cannot `UPDATE`, `TRUNCATE`, `REFERENCES` or `TRIGGER` the table, nor alter it, nor take a row lock (`FOR UPDATE` is refused with `42501`). `DELETE` does let it take a table lock (`LOCK TABLE`, any mode), as it can on any table it deletes from: that costs availability and not integrity, and the table holds nothing the gate cannot do without.
* `tenant.purge_expired_uploads()` is `SECURITY DEFINER`, owned by the migrator, so that it is not held to the row policy: it deletes what has expired for every organisation, and does nothing else. Its search path is pinned to `pg_catalog, pg_temp`, so a schema or a temporary table in the caller's path cannot stand in for anything it uses, and `EXECUTE` is revoked from `PUBLIC` and granted to the application alone.

Why the application can delete here and not in `public`: an upload is the company's, it is kept for a time and then expires, and the company may ask for it to be removed. None of that is true of evidence.

Why the application has no `TEMPORARY`: a temporary table is searched before the schemas of a
search path unless the path names `pg_temp` later, so a role that may create one named `sync_runs`
makes every function that reads `sync_runs` without a schema read its table instead. The run guard
of migration 0005 is such a function (it is applied and cannot be edited), and with a temporary
`sync_runs` that says a finished run is running it accepts evidence under that run
(`TestATemporaryTableCannotMakeTheRunGuardAcceptAFinishedRun` shows it work while the privilege
is held and fail once it is not). The redaction function pins its search path with `pg_temp`
last and names every object with its schema, so the DBA group, which may create temporary tables,
cannot do this to it (`TestRedactionCannotBeHijackedThroughTemporaryTables`).

## Where each privilege comes from

* `scripts/roles.sql`, run by a superuser: the roles and their attributes, the memberships
  (none, for the three roles), the database owner and the database privileges, the schema grants,
  the hand-over of existing objects to the migrator, the default privileges for tables a later
  migration adds, and who may execute the redaction function. `anbud_app` reads and inserts new
  tables, `anbud_dba` reads them, `UPDATE` is a default for nobody. It names no table.
  For the redaction function it takes back every direct grant that a role other than the owner
  and `anbud_dba` holds (a grant to `anbud_app`, to `PUBLIC` or to any other role, with what that
  role passed on), and the group's grant option, then grants `EXECUTE` to `anbud_dba`.
* `internal/db/privileges.sql`, run by the migration runner after every run, in the same
  transaction, as the owner: the application's table privileges by name (the matrix above).
  It has to live there because `UPDATE` on the ledger tables can only be granted once the
  tables exist, and on a new database they do not until the first migration has run. It is
  stated again at every start, so a grant added by hand is removed and a lost one restored
  (`TestApplyMigrationsReassertsTheAppPrivileges`). It does nothing when `anbud_app` does
  not exist, so a database without the role model migrates as it always did.
  On every relation of `public` that the running role may change (it owns it, directly
  or through a role it holds) it takes `UPDATE`, `DELETE`, `TRUNCATE`, `REFERENCES` and `TRIGGER`
  from the application, and `MAINTAIN` on PostgreSQL 17, and on `schema_migrations` everything.
  A `REVOKE` on a table takes the privilege from every column of it as well, so a column grant made
  by hand goes with it (`TestColumnPrivilegesGrantedByHandAreTakenBack`).
  Then it names the grants, among them `SELECT` on `raw_redactions` and `raw_documents_live`
  and no `INSERT` on either: only the redaction function writes a tombstone, and a database that
  was migrated to 0006 before the roles existed ends the same as one that was not.
  A relation that another role owns, such as a view an extension installed in `public`
  or a table a superuser made by hand, is left alone: PostgreSQL refuses a `REVOKE` on a relation
  the caller holds nothing on, which would roll the whole migration back at every start
  (`TestApplyMigrationsIgnoresRelationsOfOtherOwners`).
* `internal/db/tenant_privileges.sql`, run by the migration runner after `privileges.sql`, in the same
  transaction, as the owner: the application's privileges on schema `tenant` (the table above). It is a
  file of its own because `privileges.sql` is guarded as exactly the evidence matrix. It first takes the
  schema from `PUBLIC` and from the DBA group, and every table and routine of the schema that the running
  role owns from `PUBLIC`, the application and the DBA group, and then grants what the table above lists:
  a grant made by hand to any of them is taken back at the next start. It does nothing unless the role and
  the schema exist and the running role owns the schema, so a database without the role model migrates as
  it always did, and one whose schema another role made (a service run with a superuser DSN, before
  `roles.sql` took the schema over) is left for `roles.sql` to take over and is not stopped by a
  permission error (`TestAMigrationRunLeavesATenantSchemaAnotherRoleOwnsAlone`). `roles.sql` hands the
  schema, its tables and its functions to the migrator like everything else in `public`, which is why the
  migrator may state grants on it; the default privileges of `roles.sql` are for `public` alone, so nothing
  in `tenant` is granted by default. A shape test keeps the grants of the file to exactly four statements
  and checks that each revoke is still there (`TestTenantPrivilegesManifestStaysWhereItIs`, with a test of
  the checker that feeds it each form it must refuse and each revoke removed), a second guards that
  migration 0007 itself grants nothing (`TestMigration0007GrantsNothing`), and a third that the privileges
  are stated again at every start (`TestThePrivilegesOnTheTenantSchemaAreStatedAgainAtEveryStart`).
* A table that a later migration adds keeps the defaults `roles.sql` gives (`SELECT` and
  `INSERT` to the application), minus the privileges above, which `privileges.sql` takes
  at the end of every run. A table that the application must not read needs an explicit
  `REVOKE` in its migration, because the default gives it `SELECT` and `INSERT`;
  `privileges.sql` grants nothing on a table it does not name, so the `REVOKE` stands.
* A migration that adds a table which the application must also update changes
  `privileges.sql`, in the same commit: a `GRANT UPDATE` in the migration itself would be
  taken back by the runner's last step of the same run. A shape test keeps that file inside
  the matrix: every `GRANT` has the one form `GRANT <privileges> ON <table names> TO anbud_app`,
  with no grant option, no column list and no `ALL TABLES IN SCHEMA`, and only `SELECT, INSERT` on
  the evidence and ledger tables, `SELECT` on the two relations of 0006 and `UPDATE` on
  `sync_runs` and `sync_days`. The test of that check feeds it each form it must refuse, and it
  checks that every table the file names exists.

## Setting it up

New database, in order:

1. As a superuser: `CREATE DATABASE anbud_core;` (the owner is changed in step 2).
2. `psql -X -v ON_ERROR_STOP=1 -v dbname=anbud_core -d anbud_core -f scripts/roles.sql`.
   It runs in one transaction and stops if `-v dbname` is not the database it is
   connected to, or if the name does not start with `anbud`. Run it as often as you like:
   nothing that is right changes, and no credential is touched.
3. Give the two login roles credentials (below).
4. Start `doffin-sync` with both DSNs. It opens `ANBUD_DSN`, opens `ANBUD_MIGRATE_DSN` and checks its
   role, applies the migrations through it, which creates the tables as the migrator and states the
   application's privileges on them, closes that pool, checks the role of `ANBUD_DSN`, and
   runs as `anbud_app`.

A database that a single role has migrated already, as today: the same steps. Step 2 moves
every table, view, function and procedure of the schema `public` to `anbud_migrator`
(a sequence goes with its table), keeps the data and the triggers, and gives `anbud_dba`
`SELECT` at once. `anbud_app` has no table privilege until the first start of step 4,
which is the first run of the migrator. Objects that an extension owns are left alone.
Foreign tables, sequences that no column owns, types, aggregates and every other kind of
object are not handed over; the schema has none, and a database that has would
need them moved by hand.
`TestRolesScriptTakesOverADatabaseMigratedWithOneRole` runs this path.

Taking `CONNECT` and `TEMPORARY` from `PUBLIC` changes who can connect: only the three roles, a role
that has been granted `CONNECT` on the database by name, and the members of `anbud_dba`, which hold it
through the group. A monitoring role, a backup role or a DBA's own login that is in none of these
loses access to this database until it is given `CONNECT`.

`scripts/test-db.sh` runs the script twice under the real `psql` before the Go tests, and
the Go tests run it again through `pgx`, which is why it uses plain SQL and one variable.

The setup above and the tests run the script as a superuser. A non-superuser admin, on a
managed service, would need `CREATEROLE` and `CREATEDB` and must be able to `SET ROLE
anbud_migrator` for the owner change and the hand-over. The script grants no such membership,
and that path is not tested. On PostgreSQL 16 and later it revokes each membership by the role
that granted it, which names a grantor other than the one issuing the command; that is a
superuser's to do, and a non-superuser admin may be refused. Before PostgreSQL 16 it revokes
without a grantor; that branch is not tested either, because the test server is 16.

## Configuration

| Variable | Meaning |
|---|---|
| `ANBUD_DSN` | The pool the engine writes through. Required, as before. With the role model this is `anbud_app`. |
| `ANBUD_MIGRATE_DSN` | Optional. When set, migrations run through it and the runtime pool uses `ANBUD_DSN`. Its role is checked before it migrates anything (see The owner check below). When unset, one DSN does both and the migrations run as before; the runtime-role check below still applies to it, so a superuser or owner DSN warns on loopback and is refused elsewhere. |

Both go through the same TLS rule (`db.CheckTLS`): a host that is not loopback needs
`sslmode=verify-ca` or `verify-full`, with one host. A refusal names the variable and
never the DSN. Nothing prints a DSN or any part of it: errors are fixed text, and the
migration pool is closed before the engine starts. The variable's name is declared once, as
`config.EnvMigrateDSN`.

With the role model, `ANBUD_DSN` alone, as `anbud_app`, stops at start up with
`could not apply the database migrations`: the application cannot create
`schema_migrations`, and that is intended.

## The runtime-role check

### Unified Studio Compose deployment

The Studio root `docker-compose.yml` has an `anbud` profile with two services:
`anbud-migrate` runs `anbud-api --migrate-only` as `anbud_migrator`, exits, and
`anbud-core` then runs with `ANBUD_SKIP_MIGRATIONS=true` as `anbud_app`.
The API receives no migrator DSN. Both modes reject an unsafe role even on
loopback; migration-only also refuses a missing dedicated migration DSN.
The default API mode retains its existing single-DSN development migrations.

From the Studio directory, `node scripts/provision-anbud-database.mjs` prints
the local plan without contacting Docker. `--apply` provisions only the fixed
`anbud_core` database inside `studio-postgres`, using protected deployment files
`anbud_db_password` and `anbud_migrator_db_password` (owned 0400/0600, 64 hex
characters, under the owned 0700 secrets directory). Passwords travel only over
stdin during login checks; creation SQL contains salted SCRAM verifiers, not
cleartext passwords. Existing configured databases verify logins without
rotating credentials. An unknown database owner or partial existing roles stops
before any writes and requires review. No migration is run by this helper; the
one-shot service runs it through the owner role.

The local Compose DSNs disable TLS only over the private Studio Docker network;
this is not a production DSN policy. The API publishes only on loopback by
default and `/ready` checks database connectivity. The API does not import the
whole shared-template `.env`: only explicitly projected service settings enter
it, so unrelated provider credentials do not leak into Anbud.

After the migrations, `doffin-sync` checks the role of the pool the engine writes through, and
`doffin-probe` checks the role of the pool it opens to hold the budget lock
(`db.CheckRuntimeRole`). A role is judged by what it can use or become: the check reads every role
it is a member of, itself included, and a membership counts whether or not it inherits, because a
member can `SET ROLE` to the group. The role is too privileged if **any** of these holds:

* it, or a role it is a member of, is a superuser, or has `CREATEROLE`, `CREATEDB` or `BYPASSRLS`;
* it is a member of a predefined role that reaches outside the database: `pg_execute_server_program`,
  `pg_write_server_files`, `pg_read_server_files`, `pg_write_all_data`, `pg_signal_backend`,
  `pg_maintain` and `pg_checkpoint` (those the server has);
* it is `anbud_migrator` or `anbud_dba`, or a member of either: the application is neither;
* it owns, itself or through a role it is a member of, a table, partitioned table, view,
  materialized view, foreign table or sequence in any schema (not a temporary table, which belongs to
  a session), a function or procedure, a schema on its search path, or the database.

For a superuser every one of these is true, which is right: it can do all of it. The owner of the
database owns the schema `public` too, through `pg_database_owner`.

| Role | Host is loopback (or a Unix socket) | Any other host |
|---|---|---|
| nothing above | runs | runs |
| too privileged | one warning line on stderr, then runs | refuses: exit 1, fixed message, no run starts |
| cannot be read | refuses: exit 6 (probe: exit 1) | refuses |

"Loopback" is judged by the rule `db.CheckTLS` uses: `localhost`, `127.0.0.1`, `::1` or a Unix
socket directory, and a remote host anywhere in the DSN, a fallback included, makes it remote.
The messages name `ANBUD_DSN` and `docs/ROLES.md` and no role, host, database or credential.
So with one DSN, whose role owns the tables, the sync warns on loopback and refuses
elsewhere. That is the push towards the two-role setup.

The check is a start-up check: it reads the catalog once. A membership someone grants while a sync
runs is seen at the next start, and `scripts/roles.sql` takes it back when it is run again. What the
check sees and the script undoes were tested together: a membership of `anbud_app` in
`anbud_migrator`, in `anbud_dba` and in `pg_execute_server_program` is seen, taken back by the
script, and afterwards the application cannot run the redaction function
(`TestRuntimeRoleCheckSeesAMembershipGrantedToTheApplicationRoleByHand`,
`TestRolesScriptTakesBackEveryMembershipOfTheThreeRoles`).

## The owner check

When `ANBUD_MIGRATE_DSN` is set, `doffin-sync` checks its role (`db.CheckOwnerRole`) right after it
opens that pool, before the migrations run. The owner owns every object, the schema and the database,
and is `anbud_migrator`, so ownership and the model's own roles are no reason here. What is: it, or a
role it is a member of, is a superuser or has `CREATEROLE`, `CREATEDB` or `BYPASSRLS`, or it is a member
of one of the predefined roles above. The outcome is the same table as above, with its own fixed
messages that name `ANBUD_MIGRATE_DSN`: refused on a host that is not loopback with exit 1 before any
migration has run, one warning line on loopback, exit 6 when the role cannot be read. A start with
one DSN has no owner check, because the runtime check of that DSN is the stricter one.

## Credentials

The repository has none. Give each login role its credential out of band, in an interactive
`psql` with `\password anbud_app` (it hashes on the client and keeps the value out of the
server log), or from the secret store that provisions the database. The script never
sets or changes one, so running it again keeps what is there. Use `scram-sha-256` for these
roles in `pg_hba.conf`; a role with no credential cannot log in with a password at all.
The migrator's credential belongs to the deploy step.

A human DBA: `CREATE ROLE alice LOGIN ...; GRANT anbud_dba TO alice;`. `anbud_dba` reads
every table and writes nothing, except that its members may run the redaction function, and it may
create temporary tables. A member connects through the group's `CONNECT`. Nothing else is
granted to it.

## What this does not do

* The ledger is the application's to update, by design, so the application role can put a
  finished run back to `running` and the run guard of migration 0005 would then admit
  evidence under it. That guard stops a stale process, not the application role. The
  triggers on the evidence tables are the part that the roles make a boundary.
* The checks read the catalog at the start of a process. A role that holds a privilege it should
  not, given while the process runs, is found by the next start, or by running `scripts/roles.sql`.
* A process started with both DSNs has the owner's credential in its environment until it
  exits, although the engine uses only the application pool. The boundary protects the
  connection the engine writes through, and a compromise of the whole process is outside
  it. Keeping the owner's credential out of the long-running process needs a migrate-only
  invocation, which does not exist yet.
* `PUBLIC` keeps `USAGE` on `public`, the PostgreSQL default. It has no `CONNECT`, `TEMPORARY` or
  `CREATE` on the database, and the script takes them away on every run, which affects other
  roles that may have relied on them (see Setting it up).
* `anbud_app` can read all evidence, which may hold the contact details of persons named in
  a notice. That is the engine's job. `anbud_dba` reads it too. Its members can erase a body, and
  only through `anbud_redact_raw_document()`: the function runs with the owner's rights and
  records a tombstone under the caller's own login, refuses an unknown or already redacted
  document, and logs ids only. They cannot change or delete anything else in evidence, write a
  tombstone, change a trigger or the function, or set `session_replication_role`
  (`TestDBARoleReadsEverythingAndWritesNothing`,
  `TestDBAGroupCannotSwitchOffTheTriggersWithSessionReplicationRole`). The application cannot
  run the function: nothing grants it, a grant to any role but the DBA group is taken back by
  the script, and a membership of the application in the group is taken back by the script and
  refused by the check. Who may be a member of the group is the DBA's to look after: the script takes
  back the memberships of the three roles, not those of the people in the group.
* Row level security on `tenant.tender_uploads` does not bind the owner or a superuser (see above), and
  the isolation of tenants on a database run with one DSN is the store's own queries. It is checked
  against the roles (`TestRowLevelSecurityKeepsOrganisationsApartWhateverTheQueryForgets`) and the store
  against the owner (`internal/docstore`); the plane's compose setup still runs one superuser DSN.
* One anbud database per cluster. The three roles are the cluster's, so a second database prepared
  with the script shares their passwords and memberships, and a role test run against a cluster that
  already holds them is refused by the test helpers, which exist to run on a throwaway server.
* `cmd/api` still opens one pool from `ANBUD_DSN` and migrates through it. `internal/config`
  already carries `ANBUD_MIGRATE_DSN` for it.

## Tests

Pure: the decision table of the role check (every combination of the eleven facts and both
kinds of host, and the owner's smaller table), the check against a fake query, the shape of both
scripts (no credential, only the `dbname` variable, every attribute off, one transaction, nothing
dropped or granted to `PUBLIC`, the database privileges by name, the memberships revoked, the name
checks before any change), the privileges check and the test of that check, the wiring of both
commands, and that no output carries a DSN or a part of one.
Against Postgres (`make test-db`): the matrix at table level and at column level, ownership, role
attributes, idempotency, the hand-over, the default privileges, drift correction (by the script and
by the runner: attributes, schema and database grants, memberships by any grantor, grants on the
redaction function and what was passed on through them, column grants), every reason of the runtime
check (membership of a privileged role, of the owner, of the DBA group, of a predefined role, a
member that does not inherit, ownership of each kind of relation, of a function, a procedure, the
schema and the database), the owner check, the application's whole write path, a full `doffin-sync`
round trip as `anbud_app` with `ANBUD_MIGRATE_DSN` set, a role that no grant lets connect, the
application's `42501` on a temporary table and the shadowing of the run guard, a refused attempt, as
`anbud_app`, at every way to take the triggers away or to change or lock evidence (each fails with
`42501`, the privilege layer's own code, and never with the triggers' `23001` or `55000`), and the
refusal of a database whose name does not start with `anbud`. For the tenant schema (`internal/docstore`):
the application's use of the store, row level security keeping two organisations apart whatever the
query forgets (a `SELECT` with no `WHERE`, an insert for another organisation, a `DELETE` with no
condition), the refusal of `UPDATE`, `TRUNCATE`, `ALTER` and `DROP POLICY` with `42501`, the server
refusing a query once row security is switched off, that only the owner bypasses the policy, that the application
cannot choose a row's id or its clock and that the two-year bound holds in every time zone, that the DBA
group and a stranger have nothing on the schema (a grant made by hand to either is taken back), that the purge removes expired rows for every
organisation and nothing else, and that its definition is pinned and owned by the migrator. The roles are cluster-wide, so the tests
create them only on the throwaway server, under a lock, with random credentials, and drop them when
the test ends; a server that already has one of them is refused before anything is changed, with a
failure when `ANBUD_REQUIRE_DB_TESTS` is 1 and a skip otherwise.
