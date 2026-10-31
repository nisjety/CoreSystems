package main

import (
	"bytes"
	"context"
	"net/url"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// Needs a throwaway server on ANBUD_TEST_DSN: run these with make test-db. The
// command runs as the roles scripts/roles.sql creates: migrations through
// ANBUD_MIGRATE_DSN as anbud_migrator, the engine through ANBUD_DSN as anbud_app,
// against the loopback Doffin of cli_db_test.go. Nothing the command prints may
// carry a part of either DSN: the role, the password, the host or the database.

// migrationCount is the number of migrations the module ships, one file each under
// internal/db/migrations. It is read from the directory, so a new migration does not
// stale the count the tests expect.
func migrationCount(t *testing.T) int64 {
	t.Helper()
	files, err := filepath.Glob("../../internal/db/migrations/*.sql")
	if err != nil || len(files) == 0 {
		t.Fatalf("the migrations directory holds no migrations: %v", err)
	}
	return int64(len(files))
}

// assertNoDSNParts fails when the output carries any part of any of the DSNs.
func assertNoDSNParts(t *testing.T, output string, dsns ...string) {
	t.Helper()
	for _, dsn := range dsns {
		for _, part := range dbtest.DSNParts(dsn) {
			if strings.Contains(output, part) {
				t.Errorf("the output carries a part of a DSN (%d bytes of it)", len(part))
			}
		}
	}
}

// zeroDayEnv points the command at a loopback Doffin whose one day holds no notice.
// The day is done after two searches and nothing is downloaded, so the run is quick
// and the test is about the database.
func zeroDayEnv(t *testing.T, extra map[string]string) (env map[string]string, day string, doffinCalls func() (int, int)) {
	t.Helper()
	fake, srv := newLoopbackDoffin(t, nil, "")
	env = map[string]string{
		"DOFFIN_API_KEY":    cliKey,
		"DOFFIN_BASE_URL":   srv.URL + "/public/v2",
		"DOFFIN_SEARCH_RPM": "30", // the documented limit, so the two searches are not paced to a crawl
	}
	for k, v := range extra {
		env[k] = v
	}
	dbtest.MustBeThrowaway(t, env["ANBUD_DSN"], env["ANBUD_MIGRATE_DSN"])
	return env, loopbackDay().Format(dateLayout), func() (int, int) {
		searches, downloads := fake.calls()
		return searches, len(downloads)
	}
}

func runZeroDay(env map[string]string, day string) (code int, stdout, stderr string) {
	var out, errOut bytes.Buffer
	code = cli([]string{"-from", day, "-to", day}, mapEnv(env), &out, &errOut)
	return code, out.String(), errOut.String()
}

// The whole round trip as the least-privileged role: the day is searched, held
// notices are not downloaded again, two are fetched and stored, the ledger is written and
// closed, all through anbud_app, while the migrations go through anbud_migrator.
// A role that could not write what the engine writes, or could not take the run
// guard's row lock, would fail here.
func TestCLIRoundTripAsTheApplicationRole(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	migrator := dbtest.Pool(t, rs.MigratorDSN)
	if err := db.ApplyMigrations(context.Background(), migrator); err != nil {
		t.Fatalf("migrate as the owner role: %s", dbtest.Describe(err))
	}
	app := dbtest.Pool(t, rs.AppDSN)

	d := loopbackDay()
	ids := loopbackIDs(d)
	seedHeld(t, app, d, ids[:cliHeld])
	fake, srv := newLoopbackDoffin(t, ids, ids[cliHeld+1])
	stored, notFound := ids[cliHeld], ids[cliHeld+1]

	env := map[string]string{
		"ANBUD_DSN":           rs.AppDSN,
		"ANBUD_MIGRATE_DSN":   rs.MigratorDSN,
		"DOFFIN_API_KEY":      cliKey,
		"DOFFIN_BASE_URL":     srv.URL + "/public/v2",
		"DOFFIN_DOWNLOAD_RPM": "10",
	}
	dbtest.MustBeThrowaway(t, env["ANBUD_DSN"], env["ANBUD_MIGRATE_DSN"])
	day := d.Format(dateLayout)
	var stdout, stderr bytes.Buffer
	code := cli([]string{"-from", day, "-to", day}, mapEnv(env), &stdout, &stderr)
	if code != exitDone {
		t.Fatalf("exit = %d, want %d (done); stderr %q", code, exitDone, stderr.String())
	}

	searches, downloads := fake.calls()
	if searches != 4 {
		t.Errorf("searches = %d, want 4: two passes over pages 1 and 2 (D-B3)", searches)
	}
	if len(downloads) != 2 || downloads[stored] != 1 || downloads[notFound] != 1 {
		t.Errorf("downloads = %v, want one call each for the two unheld ids and none else", downloads)
	}
	assertRows(t, app, `SELECT count(*) FROM raw_documents WHERE doffin_id = $1`, 1, stored)
	assertRows(t, app, `SELECT count(*) FROM download_attempts WHERE doffin_id = $1 AND outcome = 'not_found' AND http_status = 404`, 1, notFound)
	assertRows(t, app, `SELECT count(*) FROM sync_runs WHERE state = 'running'`, 0)
	assertRows(t, app, `SELECT count(*) FROM sync_days WHERE state = 'done'`, 1)

	objs := jsonLines(t, stdout.String())
	if len(objs) != 2 {
		t.Fatalf("stdout has %d objects, want one day line and one summary: %q", len(objs), stdout.String())
	}
	if day := objs[0]; day["state"] != "done" || day["held"] != 99.0 || day["stored"] != 1.0 || day["not_found"] != 1.0 {
		t.Errorf("day line = %v, want done with held 99, stored 1, not_found 1", day)
	}
	if summary := objs[1]; summary["reason"] != "done" || summary["run_id"] == nil {
		t.Errorf("summary = %v, want reason done and a run id", summary)
	}

	// A role that is not too privileged is not warned about: stderr stays empty.
	if stderr.Len() != 0 {
		t.Errorf("stderr = %q, want nothing for a least-privilege role", stderr.String())
	}
	assertNoDSNParts(t, stdout.String()+stderr.String(), rs.AppDSN, rs.MigratorDSN)
	assertNoLeak(t, stdout.String()+stderr.String(), ids)
}

// The command applies the migrations itself, as the owner role, on a database that has
// none: the tables, the functions and the sequences end up owned by anbud_migrator,
// which the application role could never have created, and the run is written
// through the application role.
func TestCLIMigratesThroughTheMigrateDSN(t *testing.T) {
	rs := dbtest.RoleDatabase(t) // no migration has run
	admin := dbtest.Pool(t, rs.AdminDSN)
	env, day, calls := zeroDayEnv(t, map[string]string{
		"ANBUD_DSN":         rs.AppDSN,
		"ANBUD_MIGRATE_DSN": rs.MigratorDSN,
	})

	code, stdout, stderr := runZeroDay(env, day)

	if code != exitDone {
		t.Fatalf("exit = %d, want %d (done); stderr %q", code, exitDone, stderr)
	}
	if stderr != "" {
		t.Errorf("stderr = %q, want nothing", stderr)
	}
	if searches, downloads := calls(); searches != 2 || downloads != 0 {
		t.Errorf("doffin saw %d searches and %d downloads, want 2 and 0", searches, downloads)
	}
	// One row per migration in the directory. Seven tables in public: the five of the raw store, the
	// tombstones of 0006 and the migration bookkeeping. The tenant schema holds the uploads of 0007 and
	// the workspace of 0009 and 0010; 0008 and 0011 add no table.
	assertRows(t, admin, `SELECT count(*) FROM schema_migrations`, migrationCount(t))
	assertRows(t, admin, `SELECT count(*) FROM pg_tables WHERE schemaname = 'public' AND tableowner <> $1`, 0, dbtest.RoleMigrator)
	assertRows(t, admin, `SELECT count(*) FROM pg_tables WHERE schemaname = 'public'`, 7)
	assertRows(t, admin, `SELECT count(*) FROM sync_runs WHERE state = 'done'`, 1)
	assertRows(t, admin, `SELECT count(*) FROM sync_days WHERE state = 'done'`, 1)
	assertNoDSNParts(t, stdout+stderr, rs.AppDSN, rs.MigratorDSN)
}

// With one DSN, as before, nothing changes but a warning: the server's own role in
// the throwaway cluster is a superuser, the host is loopback, so the command prints
// one line to stderr and runs.
func TestCLIWithOneDSNWarnsOnceForASuperuserOnLoopback(t *testing.T) {
	dsn := dbtest.Database(t)
	env, day, _ := zeroDayEnv(t, map[string]string{"ANBUD_DSN": dsn})

	code, stdout, stderr := runZeroDay(env, day)

	if code != exitDone {
		t.Fatalf("exit = %d, want %d (done); stderr %q", code, exitDone, stderr)
	}
	if want := "doffin-sync: " + db.RoleWarning + "\n"; stderr != want {
		t.Errorf("stderr = %q, want exactly one warning line, %q", stderr, want)
	}
	if len(jsonLines(t, stdout)) != 2 {
		t.Errorf("stdout = %q, want one day line and one summary", stdout)
	}
	assertNoDSNParts(t, stdout+stderr, dsn)
}

// The same one-line warning when the runtime role is not a superuser but owns the
// tables, which is the owner role used for both jobs.
func TestCLIWarnsOnceWhenTheRuntimeRoleOwnsTheTables(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	env, day, _ := zeroDayEnv(t, map[string]string{"ANBUD_DSN": rs.MigratorDSN})

	code, stdout, stderr := runZeroDay(env, day)

	if code != exitDone {
		t.Fatalf("exit = %d, want %d (done); stderr %q", code, exitDone, stderr)
	}
	if want := "doffin-sync: " + db.RoleWarning + "\n"; stderr != want {
		t.Errorf("stderr = %q, want exactly one warning line, %q", stderr, want)
	}
	assertNoDSNParts(t, stdout+stderr, rs.MigratorDSN)
}

// Off loopback a superuser may not run the engine. The throwaway server is on
// loopback, so the host is judged from a DSN that names a remote one, with the
// facts read from the real role; the command refuses with exit 1 and a fixed
// message before it calls Doffin or starts a run.
func TestCLIRefusesARealSuperuserOffLoopback(t *testing.T) {
	dsn := dbtest.Database(t) // migrated, so a run could start if the command did not refuse
	admin := dbtest.Pool(t, dsn)
	env, day, calls := zeroDayEnv(t, map[string]string{"ANBUD_DSN": dsn})

	steps := realStoreOps()
	steps.checkRole = func(ctx context.Context, q db.RowQuerier, _ string) (db.RoleVerdict, error) {
		return db.CheckRuntimeRole(ctx, q, "postgres://remote_user:remote-marker-pw@db.example.com:5432/remote_db?sslmode=verify-full")
	}
	withStoreSteps(t, steps)

	code, stdout, stderr := runZeroDay(env, day)

	if code != exitUsage {
		t.Fatalf("exit = %d, want %d (usage)", code, exitUsage)
	}
	if want := "doffin-sync: " + db.ErrRoleTooPrivileged.Error() + "\n"; stderr != want {
		t.Errorf("stderr = %q, want %q", stderr, want)
	}
	if stdout != "" {
		t.Errorf("stdout = %q, want nothing", stdout)
	}
	if searches, downloads := calls(); searches != 0 || downloads != 0 {
		t.Errorf("doffin saw %d searches and %d downloads, want none: the command must refuse before it calls Doffin", searches, downloads)
	}
	assertRows(t, admin, `SELECT count(*) FROM sync_runs`, 0)
	assertNoDSNParts(t, stdout+stderr, dsn, "postgres://remote_user:remote-marker-pw@db.example.com:5432/remote_db?sslmode=verify-full")
}

// The application role cannot run the migrations, so with ANBUD_DSN alone and no
// ANBUD_MIGRATE_DSN the command stops at start up with the migrations' own message.
func TestCLIWithTheApplicationRoleAloneStopsAtTheMigrations(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	migrator := dbtest.Pool(t, rs.MigratorDSN)
	if err := db.ApplyMigrations(context.Background(), migrator); err != nil {
		t.Fatalf("migrate as the owner role: %s", dbtest.Describe(err))
	}
	env, day, calls := zeroDayEnv(t, map[string]string{"ANBUD_DSN": rs.AppDSN})

	code, stdout, stderr := runZeroDay(env, day)

	if code != exitFailed {
		t.Fatalf("exit = %d, want %d (failed)", code, exitFailed)
	}
	if want := "doffin-sync: could not apply the database migrations\n"; stderr != want {
		t.Errorf("stderr = %q, want %q", stderr, want)
	}
	if searches, _ := calls(); searches != 0 {
		t.Errorf("doffin saw %d searches, want none", searches)
	}
	assertNoDSNParts(t, stdout+stderr, rs.AppDSN)
}

// An ANBUD_MIGRATE_DSN the command cannot open ends the start with a fixed message
// that names the variable and nothing of the DSN, and not one row is written.
func TestCLIReportsAMigrateDSNItCannotOpen(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	u, err := url.Parse(rs.MigratorDSN)
	if err != nil {
		t.Fatal("the owner DSN does not parse")
	}
	u.Path = "/anbud_test_database_that_does_not_exist"
	missing := u.String()
	env, day, calls := zeroDayEnv(t, map[string]string{"ANBUD_DSN": rs.AppDSN, "ANBUD_MIGRATE_DSN": missing})

	code, stdout, stderr := runZeroDay(env, day)

	if code != exitFailed {
		t.Fatalf("exit = %d, want %d (failed)", code, exitFailed)
	}
	if want := "doffin-sync: could not open the database for the migrations; check ANBUD_MIGRATE_DSN\n"; stderr != want {
		t.Errorf("stderr = %q, want %q", stderr, want)
	}
	if searches, _ := calls(); searches != 0 {
		t.Errorf("doffin saw %d searches, want none", searches)
	}
	assertNoDSNParts(t, stdout+stderr, rs.AppDSN, missing)
}

// The pool the engine writes through is the application role's: while the run is
// in flight no session of the owner role is open, because the migration pool is closed
// before the run starts.
func TestCLIClosesTheMigrationPoolBeforeTheRun(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	admin := dbtest.Pool(t, rs.AdminDSN)
	env, day, _ := zeroDayEnv(t, map[string]string{"ANBUD_DSN": rs.AppDSN, "ANBUD_MIGRATE_DSN": rs.MigratorDSN})

	var ownerSessions, appSessions int
	steps := realStoreOps()
	inner := steps.checkRole
	steps.checkRole = func(ctx context.Context, q db.RowQuerier, dsn string) (db.RoleVerdict, error) {
		count := func(role string, into *int) {
			if err := admin.QueryRow(ctx, `SELECT count(*) FROM pg_stat_activity WHERE usename = $1 AND datname = current_database()`, role).Scan(into); err != nil {
				t.Errorf("count sessions: %s", dbtest.Describe(err))
			}
		}
		// The server removes a closed session a moment after the client says goodbye.
		for range 40 {
			count(dbtest.RoleMigrator, &ownerSessions)
			if ownerSessions == 0 {
				break
			}
			time.Sleep(50 * time.Millisecond)
		}
		count(dbtest.RoleApp, &appSessions)
		return inner(ctx, q, dsn)
	}
	withStoreSteps(t, steps)

	code, _, stderr := runZeroDay(env, day)

	if code != exitDone {
		t.Fatalf("exit = %d, want %d; stderr %q", code, exitDone, stderr)
	}
	if ownerSessions != 0 {
		t.Errorf("%d sessions of the owner role were open when the engine's role was checked, want 0", ownerSessions)
	}
	if appSessions == 0 {
		t.Error("no session of the application role was open when its role was checked")
	}
}

// The role of ANBUD_MIGRATE_DSN is judged before it migrates anything. The owner owns every table by
// design, so ownership is no reason; a superuser is. The throwaway server is on loopback, so the
// refusal is judged from a DSN that names a remote host, with the facts read from the real role,
// as the refusal of the runtime role is; the command stops with exit 1 and a fixed message before
// a migration has run or Doffin has been called, and on loopback it is one warning line and the
// run goes on.
func TestCLIChecksTheRoleOfTheMigrateDSNBeforeItMigrates(t *testing.T) {
	rs := dbtest.RoleDatabase(t) // not migrated: nothing exists until a migration runs
	admin := dbtest.Pool(t, rs.AdminDSN)
	const remote = "postgres://remote_user:remote-marker-pw@db.example.com:5432/remote_db?sslmode=verify-full"

	t.Run("a superuser owner is refused off loopback, with no migration run", func(t *testing.T) {
		env, day, calls := zeroDayEnv(t, map[string]string{"ANBUD_DSN": rs.AppDSN, "ANBUD_MIGRATE_DSN": rs.AdminDSN})
		steps := realStoreOps()
		steps.checkOwner = func(ctx context.Context, q db.RowQuerier, _ string) (db.RoleVerdict, error) {
			return db.CheckOwnerRole(ctx, q, remote)
		}
		withStoreSteps(t, steps)

		code, stdout, stderr := runZeroDay(env, day)

		if code != exitUsage {
			t.Fatalf("exit = %d, want %d (usage)", code, exitUsage)
		}
		if want := "doffin-sync: " + db.ErrOwnerTooPrivileged.Error() + "\n"; stderr != want {
			t.Errorf("stderr = %q, want %q", stderr, want)
		}
		if stdout != "" {
			t.Errorf("stdout = %q, want nothing", stdout)
		}
		if searches, downloads := calls(); searches != 0 || downloads != 0 {
			t.Errorf("doffin saw %d searches and %d downloads, want none", searches, downloads)
		}
		assertRows(t, admin, `SELECT count(*) FROM pg_tables WHERE schemaname = 'public'`, 0)
		assertNoDSNParts(t, stdout+stderr, rs.AdminDSN, rs.AppDSN, remote)
	})

	t.Run("a superuser owner on loopback is one warning, and the migrations and the run go on", func(t *testing.T) {
		env, day, _ := zeroDayEnv(t, map[string]string{"ANBUD_DSN": rs.AppDSN, "ANBUD_MIGRATE_DSN": rs.AdminDSN})

		code, stdout, stderr := runZeroDay(env, day)

		if code != exitDone {
			t.Fatalf("exit = %d, want %d (done); stderr %q", code, exitDone, stderr)
		}
		if want := "doffin-sync: " + db.OwnerWarning + "\n"; stderr != want {
			t.Errorf("stderr = %q, want exactly the owner warning, %q", stderr, want)
		}
		assertRows(t, admin, `SELECT count(*) FROM schema_migrations`, migrationCount(t))
		assertRows(t, admin, `SELECT count(*) FROM sync_runs WHERE state = 'done'`, 1)
		assertNoDSNParts(t, stdout+stderr, rs.AdminDSN, rs.AppDSN)
	})
}
