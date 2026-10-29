package main

import (
	"bytes"
	"context"
	"errors"
	"os"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/config"
	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/secret"
)

// openStore is the part of cli between the configuration and runSync: it opens the
// pool the engine writes through, applies the migrations (through ANBUD_MIGRATE_DSN
// when that is set, through ANBUD_DSN otherwise), and checks the runtime role
// (O2). These tests stand in for each step, so the order, the DSN each step gets,
// the exit codes and the messages are checked without a database. The same code
// against real roles is in migrate_roles_db_test.go.

const (
	storeRuntimeDSN = "postgres://rt_user:rt-marker-pw@127.0.0.1:5432/rt_db?sslmode=disable"
	storeOwnerDSN   = "postgres://own_user:own-marker-pw@127.0.0.1:5432/own_db?sslmode=disable"
)

// storeLeaks are the parts of the two DSNs and of the errors the fakes return. No
// message may carry any of them.
var storeLeaks = []string{
	"rt_user", "rt-marker-pw", "rt_db", "own_user", "own-marker-pw", "own_db",
	"127.0.0.1", "5432", "postgres://", "sslmode", "driver-text-marker",
}

// fakeSteps stands in for the steps of openStore and records what they were given.
type fakeSteps struct {
	t   *testing.T
	log []string

	pools map[string]*pgxpool.Pool // the pool each open returned, by DSN

	openErr      map[string]error // by DSN
	migrateErr   error
	verdict      db.RoleVerdict
	roleErr      error
	ownerVerdict db.RoleVerdict
	ownerErr     error

	migrated        *pgxpool.Pool
	checked         *pgxpool.Pool
	checkedDSN      string
	ownerChecked    *pgxpool.Pool
	ownerCheckedDSN string
	ownerClosedAtCk bool
}

func newFakeSteps(t *testing.T) *fakeSteps {
	t.Helper()
	return &fakeSteps{t: t, pools: map[string]*pgxpool.Pool{}, openErr: map[string]error{}}
}

func (f *fakeSteps) steps() storeOps {
	return storeOps{
		open: func(_ context.Context, dsn string) (*pgxpool.Pool, error) {
			f.log = append(f.log, "open "+f.name(dsn))
			if err := f.openErr[dsn]; err != nil {
				return nil, err
			}
			// A pool that has not connected: pgxpool dials only when it is used, and
			// nothing here uses it. It is built on its own context, so a test that
			// cancels the step's context still gets a pool.
			pool, err := pgxpool.New(context.Background(), "postgres://unused:unused@127.0.0.1:1/unused?sslmode=disable")
			if err != nil {
				f.t.Fatalf("pgxpool.New: %v", err)
			}
			f.t.Cleanup(pool.Close)
			f.pools[dsn] = pool
			return pool, nil
		},
		checkOwner: func(_ context.Context, q db.RowQuerier, dsn string) (db.RoleVerdict, error) {
			f.log = append(f.log, "check owner")
			f.ownerChecked, _ = q.(*pgxpool.Pool)
			f.ownerCheckedDSN = dsn
			return f.ownerVerdict, f.ownerErr
		},
		migrate: func(_ context.Context, pool *pgxpool.Pool) error {
			f.log = append(f.log, "migrate")
			f.migrated = pool
			return f.migrateErr
		},
		checkRole: func(ctx context.Context, q db.RowQuerier, dsn string) (db.RoleVerdict, error) {
			f.log = append(f.log, "check")
			f.checked, _ = q.(*pgxpool.Pool)
			f.checkedDSN = dsn
			if owner := f.pools[storeOwnerDSN]; owner != nil {
				err := owner.Ping(ctx)
				f.ownerClosedAtCk = err != nil && strings.Contains(err.Error(), "closed pool")
			}
			return f.verdict, f.roleErr
		},
	}
}

func (f *fakeSteps) name(dsn string) string {
	switch dsn {
	case storeRuntimeDSN:
		return "ANBUD_DSN"
	case storeOwnerDSN:
		return "ANBUD_MIGRATE_DSN"
	}
	return "an unknown DSN"
}

func syncConfig(migrateDSN string) config.Sync {
	return config.Sync{
		DatabaseURL:        secret.Value(storeRuntimeDSN),
		MigrateDatabaseURL: secret.Value(migrateDSN),
	}
}

func openWith(ctx context.Context, cfg config.Sync, f *fakeSteps) (*pgxpool.Pool, int, string) {
	var stderr bytes.Buffer
	pool, code := openStoreWith(ctx, cfg, &stderr, f.steps())
	return pool, code, stderr.String()
}

// With one DSN, as before: the pool that migrates is the pool that runs, and the role
// check follows the migrations.
func TestOpenStoreWithOneDSNMigratesThroughIt(t *testing.T) {
	f := newFakeSteps(t)

	pool, code, stderr := openWith(context.Background(), syncConfig(""), f)

	if pool == nil || code != exitDone {
		t.Fatalf("got (%v, %d), want a pool and exit 0; stderr %q", pool, code, stderr)
	}
	if want := []string{"open ANBUD_DSN", "migrate", "check"}; !slices.Equal(f.log, want) {
		t.Errorf("steps = %v, want %v", f.log, want)
	}
	if f.migrated != pool || f.checked != pool || pool != f.pools[storeRuntimeDSN] {
		t.Error("the one pool must migrate, be checked and be returned")
	}
	if f.checkedDSN != storeRuntimeDSN {
		t.Error("the role check judged a DSN other than ANBUD_DSN")
	}
	if stderr != "" {
		t.Errorf("stderr = %q, want nothing", stderr)
	}
}

// With ANBUD_MIGRATE_DSN set, the migrations run through a pool of their own, that
// pool is closed before the engine starts, and the pool the engine writes through,
// which the role check judges, is the one ANBUD_DSN opened.
func TestOpenStoreWithTwoDSNsMigratesThroughTheOwnerAndRunsThroughTheApp(t *testing.T) {
	f := newFakeSteps(t)

	pool, code, stderr := openWith(context.Background(), syncConfig(storeOwnerDSN), f)

	if pool == nil || code != exitDone {
		t.Fatalf("got (%v, %d), want a pool and exit 0; stderr %q", pool, code, stderr)
	}
	if want := []string{"open ANBUD_DSN", "open ANBUD_MIGRATE_DSN", "check owner", "migrate", "check"}; !slices.Equal(f.log, want) {
		t.Errorf("steps = %v, want %v", f.log, want)
	}
	if f.migrated != f.pools[storeOwnerDSN] {
		t.Error("the migrations did not run through the pool ANBUD_MIGRATE_DSN opened")
	}
	if pool != f.pools[storeRuntimeDSN] || f.checked != pool || f.checkedDSN != storeRuntimeDSN {
		t.Error("the engine's pool and the role check must both be ANBUD_DSN's")
	}
	if !f.ownerClosedAtCk {
		t.Error("the migration pool was still open when the engine's role was checked")
	}
	if stderr != "" {
		t.Errorf("stderr = %q, want nothing", stderr)
	}
}

// A role that is too privileged for a host that is not loopback stops the start: exit 1,
// the usage and configuration code, the fixed message, no pool handed on.
func TestOpenStoreRefusesATooPrivilegedRole(t *testing.T) {
	for name, cfg := range map[string]config.Sync{"one DSN": syncConfig(""), "two DSNs": syncConfig(storeOwnerDSN)} {
		t.Run(name, func(t *testing.T) {
			f := newFakeSteps(t)
			f.roleErr = db.ErrRoleTooPrivileged

			pool, code, stderr := openWith(context.Background(), cfg, f)

			if pool != nil {
				t.Error("a refused start handed on a pool")
			}
			if code != exitUsage {
				t.Errorf("exit = %d, want %d", code, exitUsage)
			}
			if want := "doffin-sync: " + db.ErrRoleTooPrivileged.Error() + "\n"; stderr != want {
				t.Errorf("stderr = %q, want %q", stderr, want)
			}
		})
	}
}

// On a loopback host a role that is too privileged is one warning line, and the start goes on.
func TestOpenStoreWarnsOnceAndContinues(t *testing.T) {
	f := newFakeSteps(t)
	f.verdict = db.RoleWarn

	pool, code, stderr := openWith(context.Background(), syncConfig(""), f)

	if pool == nil || code != exitDone {
		t.Fatalf("got (%v, %d), want a pool and exit 0", pool, code)
	}
	if want := "doffin-sync: " + db.RoleWarning + "\n"; stderr != want {
		t.Errorf("stderr = %q, want exactly one line, %q", stderr, want)
	}
	if strings.Count(stderr, "\n") != 1 {
		t.Errorf("stderr has %d lines, want 1", strings.Count(stderr, "\n"))
	}
}

// If the role cannot be read, the start does not go on: the check fails closed.
func TestOpenStoreStopsWhenTheRoleCannotBeChecked(t *testing.T) {
	f := newFakeSteps(t)
	f.roleErr = db.ErrRoleCheckFailed

	pool, code, stderr := openWith(context.Background(), syncConfig(""), f)

	if pool != nil || code != exitFailed {
		t.Fatalf("got (%v, %d), want no pool and exit %d", pool, code, exitFailed)
	}
	if want := "doffin-sync: " + db.ErrRoleCheckFailed.Error() + "\n"; stderr != want {
		t.Errorf("stderr = %q, want %q", stderr, want)
	}
}

// Every failure has a fixed message that names the variable to check and none of the
// DSNs or the driver's text: pgx errors can quote the connection details.
func TestOpenStoreFailureMessagesAreFixedAndNameTheVariable(t *testing.T) {
	leaky := errors.New("failed to connect to `host=127.0.0.1 user=rt_user database=rt_db`: driver-text-marker own-marker-pw")
	cases := []struct {
		name    string
		cfg     config.Sync
		arrange func(*fakeSteps)
		want    string
		log     []string
	}{
		{"the runtime database cannot be opened", syncConfig(""), func(f *fakeSteps) { f.openErr[storeRuntimeDSN] = leaky },
			"doffin-sync: could not open the database; check ANBUD_DSN\n", []string{"open ANBUD_DSN"}},
		{"the runtime database cannot be opened, two DSNs", syncConfig(storeOwnerDSN), func(f *fakeSteps) { f.openErr[storeRuntimeDSN] = leaky },
			"doffin-sync: could not open the database; check ANBUD_DSN\n", []string{"open ANBUD_DSN"}},
		{"the migration database cannot be opened", syncConfig(storeOwnerDSN), func(f *fakeSteps) { f.openErr[storeOwnerDSN] = leaky },
			"doffin-sync: could not open the database for the migrations; check ANBUD_MIGRATE_DSN\n", []string{"open ANBUD_DSN", "open ANBUD_MIGRATE_DSN"}},
		{"the migrations fail", syncConfig(""), func(f *fakeSteps) { f.migrateErr = leaky },
			"doffin-sync: could not apply the database migrations\n", []string{"open ANBUD_DSN", "migrate"}},
		{"the migrations fail, two DSNs", syncConfig(storeOwnerDSN), func(f *fakeSteps) { f.migrateErr = leaky },
			"doffin-sync: could not apply the database migrations\n", []string{"open ANBUD_DSN", "open ANBUD_MIGRATE_DSN", "check owner", "migrate"}},
		{"the owner's role cannot be read", syncConfig(storeOwnerDSN), func(f *fakeSteps) { f.ownerErr = leaky },
			"doffin-sync: " + db.ErrOwnerCheckFailed.Error() + "\n", []string{"open ANBUD_DSN", "open ANBUD_MIGRATE_DSN", "check owner"}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			f := newFakeSteps(t)
			c.arrange(f)

			pool, code, stderr := openWith(context.Background(), c.cfg, f)

			if pool != nil || code != exitFailed {
				t.Errorf("got (%v, %d), want no pool and exit %d", pool, code, exitFailed)
			}
			if stderr != c.want {
				t.Errorf("stderr = %q, want %q", stderr, c.want)
			}
			for _, leak := range storeLeaks {
				if strings.Contains(stderr, leak) {
					t.Errorf("stderr carries %q", leak)
				}
			}
			if !slices.Equal(f.log, c.log) {
				t.Errorf("steps = %v, want %v: a failed step must stop the later ones", f.log, c.log)
			}
		})
	}
}

// A signal that ends the start is exit 5, as it is for every other step.
func TestOpenStoreReportsCancelledWhenASignalEndedIt(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	f := newFakeSteps(t)
	f.migrateErr = errors.New("context canceled")
	cancel()

	_, code, _ := openWith(ctx, syncConfig(""), f)

	if code != exitCancelled {
		t.Errorf("exit = %d, want %d", code, exitCancelled)
	}
}

// The role is judged after the migrations: before them, a role that is about to
// own the tables owns none.
func TestOpenStoreChecksTheRoleAfterTheMigrations(t *testing.T) {
	f := newFakeSteps(t)
	openWith(context.Background(), syncConfig(storeOwnerDSN), f)

	migrate, check := slices.Index(f.log, "migrate"), slices.Index(f.log, "check")
	if migrate < 0 || check < 0 || migrate > check {
		t.Errorf("steps = %v, want the migrations before the role check", f.log)
	}
}

// The default steps are the real ones: the pool opener, the two role checks and the migration
// runner, each the very function the rest of the program uses, and the steps cli takes are those.
// Comparing the code pointers of the functions says so; a nil check would pass for any function.
func TestStoreStepsAreTheSharedImplementations(t *testing.T) {
	pointer := func(fn any) uintptr { return reflect.ValueOf(fn).Pointer() }
	for name, steps := range map[string]storeOps{"realStoreOps": realStoreOps(), "storeSteps": storeSteps} {
		for step, c := range map[string]struct{ got, want any }{
			"open":       {steps.open, db.Open},
			"migrate":    {steps.migrate, db.ApplyMigrations},
			"checkRole":  {steps.checkRole, db.CheckRuntimeRole},
			"checkOwner": {steps.checkOwner, db.CheckOwnerRole},
		} {
			if c.got == nil || pointer(c.got) != pointer(c.want) {
				t.Errorf("%s.%s is not the shared implementation", name, step)
			}
		}
	}
}

// The variable's name is declared once, in internal/config, and this package takes it from there.
func TestMigrateDSNNameComesFromConfig(t *testing.T) {
	if config.EnvMigrateDSN != "ANBUD_MIGRATE_DSN" {
		t.Fatalf("config.EnvMigrateDSN = %q", config.EnvMigrateDSN)
	}
	entries, err := os.ReadDir(".")
	if err != nil {
		t.Fatalf("read the package directory: %v", err)
	}
	for _, e := range entries {
		name := e.Name()
		if !strings.HasSuffix(name, ".go") || strings.HasSuffix(name, "_test.go") {
			continue
		}
		src, err := os.ReadFile(name)
		if err != nil {
			t.Fatalf("read %s: %v", name, err)
		}
		if strings.Contains(string(src), `"ANBUD_MIGRATE_DSN"`) {
			t.Errorf("%s spells the variable's name out; it must use config.EnvMigrateDSN", name)
		}
	}
}

// The role of ANBUD_MIGRATE_DSN is judged before it does anything: the check follows opening its
// pool and precedes the migrations, it judges that pool and that DSN, and a start with one DSN has
// no such check, because the runtime check of that DSN is the stricter one.
func TestOpenStoreChecksTheOwnerOnlyWithTwoDSNsAndBeforeTheMigrations(t *testing.T) {
	f := newFakeSteps(t)
	pool, code, stderr := openWith(context.Background(), syncConfig(storeOwnerDSN), f)
	if pool == nil || code != exitDone || stderr != "" {
		t.Fatalf("got (%v, %d, %q), want a pool, exit 0 and no output", pool, code, stderr)
	}
	owner, migrate := slices.Index(f.log, "check owner"), slices.Index(f.log, "migrate")
	if owner < 0 || migrate < 0 || owner > migrate {
		t.Errorf("steps = %v, want the owner check before the migrations", f.log)
	}
	if f.ownerChecked != f.pools[storeOwnerDSN] || f.ownerCheckedDSN != storeOwnerDSN {
		t.Error("the owner check did not judge the pool and the DSN of ANBUD_MIGRATE_DSN")
	}

	one := newFakeSteps(t)
	openWith(context.Background(), syncConfig(""), one)
	if slices.Contains(one.log, "check owner") {
		t.Errorf("steps with one DSN = %v: there is no owner check without ANBUD_MIGRATE_DSN", one.log)
	}
}

// An owner that is too privileged for a host that is not loopback stops the start before a single
// migration runs: exit 1, the fixed message that names ANBUD_MIGRATE_DSN, nothing from either DSN,
// no pool handed on, and both pools closed.
func TestOpenStoreRefusesATooPrivilegedOwnerBeforeAnyMigration(t *testing.T) {
	f := newFakeSteps(t)
	f.ownerErr = db.ErrOwnerTooPrivileged

	pool, code, stderr := openWith(context.Background(), syncConfig(storeOwnerDSN), f)

	if pool != nil || code != exitUsage {
		t.Fatalf("got (%v, %d), want no pool and exit %d", pool, code, exitUsage)
	}
	if want := "doffin-sync: " + db.ErrOwnerTooPrivileged.Error() + "\n"; stderr != want {
		t.Errorf("stderr = %q, want %q", stderr, want)
	}
	if want := []string{"open ANBUD_DSN", "open ANBUD_MIGRATE_DSN", "check owner"}; !slices.Equal(f.log, want) {
		t.Errorf("steps = %v, want %v: the migrations and the runtime check must not run", f.log, want)
	}
	for _, leak := range storeLeaks {
		if strings.Contains(stderr, leak) {
			t.Errorf("stderr carries %q", leak)
		}
	}
	for name, p := range map[string]*pgxpool.Pool{"ANBUD_DSN": f.pools[storeRuntimeDSN], "ANBUD_MIGRATE_DSN": f.pools[storeOwnerDSN]} {
		if err := p.Ping(context.Background()); err == nil || !strings.Contains(err.Error(), "closed pool") {
			t.Errorf("the pool of %s is still open after the refusal", name)
		}
	}
}

// On a loopback host an owner that is too privileged is one warning line, and the migrations go on.
func TestOpenStoreWarnsOnceForATooPrivilegedOwnerAndMigrates(t *testing.T) {
	f := newFakeSteps(t)
	f.ownerVerdict = db.RoleWarn

	pool, code, stderr := openWith(context.Background(), syncConfig(storeOwnerDSN), f)

	if pool == nil || code != exitDone {
		t.Fatalf("got (%v, %d), want a pool and exit 0", pool, code)
	}
	if want := "doffin-sync: " + db.OwnerWarning + "\n"; stderr != want {
		t.Errorf("stderr = %q, want exactly one line, %q", stderr, want)
	}
	if want := []string{"open ANBUD_DSN", "open ANBUD_MIGRATE_DSN", "check owner", "migrate", "check"}; !slices.Equal(f.log, want) {
		t.Errorf("steps = %v, want %v", f.log, want)
	}
}

// An owner that cannot be read ends the start, and a signal during the check is exit 5, like
// every other step.
func TestOpenStoreStopsWhenTheOwnerCannotBeCheckedAndSaysCancelledForASignal(t *testing.T) {
	f := newFakeSteps(t)
	f.ownerErr = db.ErrOwnerCheckFailed
	pool, code, stderr := openWith(context.Background(), syncConfig(storeOwnerDSN), f)
	if pool != nil || code != exitFailed {
		t.Fatalf("got (%v, %d), want no pool and exit %d", pool, code, exitFailed)
	}
	if want := "doffin-sync: " + db.ErrOwnerCheckFailed.Error() + "\n"; stderr != want {
		t.Errorf("stderr = %q, want %q", stderr, want)
	}
	if slices.Contains(f.log, "migrate") {
		t.Errorf("steps = %v: the migrations ran although the owner could not be read", f.log)
	}

	ctx, cancel := context.WithCancel(context.Background())
	g := newFakeSteps(t)
	g.ownerErr = db.ErrOwnerCheckFailed
	cancel()
	if _, code, _ := openWith(ctx, syncConfig(storeOwnerDSN), g); code != exitCancelled {
		t.Errorf("exit = %d, want %d when a signal ended the start", code, exitCancelled)
	}
}
