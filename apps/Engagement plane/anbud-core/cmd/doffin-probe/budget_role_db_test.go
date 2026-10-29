package main

import (
	"bytes"
	"context"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/rawstore"
)

// The probe against real roles. Needs a throwaway server on ANBUD_TEST_DSN: run
// these with make test-db. The probe applies no migration and writes nothing, so
// the role database is not migrated: a pool, the role check and the advisory lock
// are all it needs.

func probeWarnings(t *testing.T) *bytes.Buffer {
	t.Helper()
	return stubRoleSteps(t, openRealPool, db.CheckRuntimeRole)
}

func openRealPool(ctx context.Context, dsn string) (*pgxpool.Pool, error) { return db.Open(ctx, dsn) }

// As the application role the probe takes the lock and says nothing about privileges.
func TestProbeAsTheApplicationRoleTakesTheLockWithoutAWarning(t *testing.T) {
	rs := dbtest.RoleDatabase(t)
	dbtest.MustBeThrowaway(t, rs.AppDSN)
	warnings := probeWarnings(t)
	d := newCountingDoffin(t)
	useLoopbackDoffin(t, d)
	t.Setenv(envDSN, rs.AppDSN)

	var out, stderr bytes.Buffer
	err := run([]string{"-discover=false"}, &out)
	if code := finish(err, &stderr); code != 0 {
		t.Fatalf("exit code = %d, want 0; error: %v", code, err)
	}
	if warnings.Len() != 0 {
		t.Errorf("warning = %q, want nothing for a least-privilege role", warnings.String())
	}
	if !strings.Contains(out.String(), lockHeldLine) {
		t.Errorf("the probe did not hold the budget lock:\n%s", out.String())
	}
	for _, part := range dbtest.DSNParts(rs.AppDSN) {
		if strings.Contains(out.String()+stderr.String()+warnings.String(), part) {
			t.Errorf("the output carries a part of the DSN")
		}
	}
}

// With a superuser on loopback the probe warns once and carries on, as it always ran.
func TestProbeWarnsOnceForASuperuserOnLoopback(t *testing.T) {
	dsn := dbtest.Database(t)
	dbtest.MustBeThrowaway(t, dsn)
	warnings := probeWarnings(t)
	d := newCountingDoffin(t)
	useLoopbackDoffin(t, d)
	t.Setenv(envDSN, dsn)

	var out, stderr bytes.Buffer
	err := run([]string{"-discover=false"}, &out)
	if code := finish(err, &stderr); code != 0 {
		t.Fatalf("exit code = %d, want 0; error: %v", code, err)
	}
	if want := "doffin-probe: " + db.RoleWarning + "\n"; warnings.String() != want {
		t.Errorf("warning = %q, want exactly one line, %q", warnings.String(), want)
	}
	if n := d.requests.Load(); n == 0 {
		t.Error("the probe should carry on and call Doffin after a warning")
	}
}

// Off loopback a superuser is refused: exit 1, the fixed message, not one Doffin
// call, and the budget lock was never taken. The throwaway server is on loopback,
// so the host is judged from a DSN that names a remote one, with the real role's facts.
func TestProbeRefusesARealSuperuserOffLoopbackBeforeAnyCall(t *testing.T) {
	dsn := dbtest.Database(t)
	dbtest.MustBeThrowaway(t, dsn)
	pool := dbtest.Pool(t, dsn)
	d := newCountingDoffin(t)
	useLoopbackDoffin(t, d)
	t.Setenv(envDSN, dsn)
	const remote = "postgres://remote_user:remote-marker-pw@db.example.com:5432/remote_db?sslmode=verify-full"
	warnings := stubRoleSteps(t, openRealPool, func(ctx context.Context, q db.RowQuerier, _ string) (db.RoleVerdict, error) {
		return db.CheckRuntimeRole(ctx, q, remote)
	})

	var out, stderr bytes.Buffer
	err := run([]string{"-discover=false"}, &out)
	code := finish(err, &stderr)

	if code != 1 {
		t.Fatalf("exit code = %d, want 1; error: %v", code, err)
	}
	if !strings.Contains(stderr.String(), db.ErrRoleTooPrivileged.Error()) {
		t.Errorf("stderr = %q, want the fixed refusal", stderr.String())
	}
	if n := d.requests.Load(); n != 0 {
		t.Errorf("Doffin saw %d requests, want none: a refused role stops the probe before any call", n)
	}
	if warnings.Len() != 0 {
		t.Errorf("a refusal also warned: %q", warnings.String())
	}
	// Nothing holds the lock: the pool the probe opened is closed.
	ctx := context.Background()
	lock, lockErr := rawstore.New(pool).TryLock(ctx)
	if lockErr != nil {
		t.Fatalf("the budget lock is held after a refusal: %s", dbtest.Describe(lockErr))
	}
	_ = lock.Release(ctx)
	for _, part := range append(dbtest.DSNParts(dsn), dbtest.DSNParts(remote)...) {
		if strings.Contains(out.String()+stderr.String(), part) {
			t.Errorf("the output carries a part of a DSN")
		}
	}
}
