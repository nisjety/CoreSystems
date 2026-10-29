package main

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
)

// The probe opens a pool on ANBUD_DSN to hold the budget lock, so it applies the
// runtime-role rule wherever doffin-sync does (O2): a role that can drop the
// append-only triggers is refused off loopback and is one warning line on loopback.
// These tests stand in for the open and the check, so they need no database. The
// same code against real roles is in budget_role_db_test.go.

const (
	probeRoleDSN = "postgres://probe_role:probe-role-marker-pw@127.0.0.1:5432/probe_role_db?sslmode=disable"
)

var probeRoleLeaks = []string{"probe_role", "probe-role-marker-pw", "probe_role_db", "127.0.0.1", "5432", "postgres://", "driver-text-marker"}

// stubRoleSteps replaces the open and the check for one test and returns what the
// warning writer collected. The tests that call it do not run in parallel.
func stubRoleSteps(t *testing.T, open func(context.Context, string) (*pgxpool.Pool, error), check func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error)) *bytes.Buffer {
	t.Helper()
	oldOpen, oldCheck, oldWarn := openPool, checkRuntimeRole, roleWarnings
	var warnings bytes.Buffer
	openPool, checkRuntimeRole, roleWarnings = open, check, &warnings
	t.Cleanup(func() { openPool, checkRuntimeRole, roleWarnings = oldOpen, oldCheck, oldWarn })
	return &warnings
}

// lazyPool is a pool that has not connected: pgxpool dials only when it is used.
func lazyPool(t *testing.T) *pgxpool.Pool {
	t.Helper()
	pool, err := pgxpool.New(context.Background(), "postgres://unused:unused@127.0.0.1:1/unused?sslmode=disable")
	if err != nil {
		t.Fatalf("pgxpool.New: %v", err)
	}
	t.Cleanup(pool.Close)
	return pool
}

func isClosed(pool *pgxpool.Pool) bool {
	err := pool.Ping(context.Background())
	return err != nil && strings.Contains(err.Error(), "closed pool")
}

func lockerWith(t *testing.T, verdict db.RoleVerdict, checkErr error) (*dbLocker, *bytes.Buffer, **pgxpool.Pool) {
	t.Helper()
	var opened *pgxpool.Pool
	warnings := stubRoleSteps(t,
		func(context.Context, string) (*pgxpool.Pool, error) {
			opened = lazyPool(t)
			return opened, nil
		},
		func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error) { return verdict, checkErr },
	)
	return &dbLocker{dsn: probeRoleDSN}, warnings, &opened
}

// A role the check refuses stops the probe before it takes the lock: the error is the
// fixed refusal, and the pool it opened is closed.
func TestProbeRefusesATooPrivilegedRoleWithAFixedMessage(t *testing.T) {
	l, warnings, opened := lockerWith(t, db.RoleOK, db.ErrRoleTooPrivileged)

	pool, err := l.openChecked(context.Background())

	if pool != nil {
		t.Error("a refused role was handed a pool")
	}
	if !errors.Is(err, db.ErrRoleTooPrivileged) {
		t.Fatalf("error = %v, want the refusal", err)
	}
	if !isClosed(*opened) {
		t.Error("the pool the probe opened is still open after the refusal")
	}
	if warnings.Len() != 0 {
		t.Errorf("a refusal also warned: %q", warnings.String())
	}
}

// Through finish the refusal is exit 1 with its fixed text, and nothing from the DSN.
func TestProbeRefusalIsExitOneAndLeaksNothing(t *testing.T) {
	l, _, _ := lockerWith(t, db.RoleOK, db.ErrRoleTooPrivileged)
	_, err := l.openChecked(context.Background())

	var stderr bytes.Buffer
	if code := finish(lockFailure(err), &stderr); code != 1 {
		t.Errorf("exit code = %d, want 1", code)
	}
	if !strings.HasPrefix(stderr.String(), "doffin-probe: ") || !strings.Contains(stderr.String(), db.ErrRoleTooPrivileged.Error()) {
		t.Errorf("stderr = %q, want the command name and the fixed refusal", stderr.String())
	}
	for _, leak := range probeRoleLeaks {
		if strings.Contains(stderr.String(), leak) {
			t.Errorf("stderr carries %q", leak)
		}
	}
}

// On loopback the probe says so once, in one line on stderr, and goes on to take
// the lock with the pool it opened.
func TestProbeWarnsOnceAndKeepsThePool(t *testing.T) {
	l, warnings, opened := lockerWith(t, db.RoleWarn, nil)

	pool, err := l.openChecked(context.Background())

	if err != nil || pool == nil || pool != *opened {
		t.Fatalf("got (%v, %v), want the opened pool and no error", pool, err)
	}
	if isClosed(pool) {
		t.Error("the pool was closed although the probe goes on")
	}
	if want := "doffin-probe: " + db.RoleWarning + "\n"; warnings.String() != want {
		t.Errorf("warning = %q, want exactly one line, %q", warnings.String(), want)
	}
}

func TestProbeSaysNothingForALeastPrivilegedRole(t *testing.T) {
	l, warnings, _ := lockerWith(t, db.RoleOK, nil)

	pool, err := l.openChecked(context.Background())

	if err != nil || pool == nil {
		t.Fatalf("got (%v, %v), want a pool and no error", pool, err)
	}
	if warnings.Len() != 0 {
		t.Errorf("warning = %q, want nothing", warnings.String())
	}
}

// A check that fails does not read as "fine", and its text is the fixed one even when
// the driver's own text carries the connection details.
func TestProbeFailsClosedWhenTheRoleCannotBeChecked(t *testing.T) {
	for name, checkErr := range map[string]error{
		"the shared check's own error": db.ErrRoleCheckFailed,
		"any other error":              errors.New("failed to connect to `host=127.0.0.1 user=probe_role`: driver-text-marker probe-role-marker-pw"),
	} {
		t.Run(name, func(t *testing.T) {
			l, _, opened := lockerWith(t, db.RoleOK, checkErr)

			pool, err := l.openChecked(context.Background())

			if pool != nil || !errors.Is(err, db.ErrRoleCheckFailed) {
				t.Fatalf("got (%v, %v), want no pool and the fixed failure", pool, err)
			}
			for _, leak := range probeRoleLeaks {
				if strings.Contains(err.Error(), leak) {
					t.Errorf("the error carries %q", leak)
				}
			}
			if !isClosed(*opened) {
				t.Error("the pool is still open after the failed check")
			}
		})
	}
}

// A database that cannot be opened keeps the message it always had.
func TestProbeOpenFailureKeepsItsFixedMessage(t *testing.T) {
	stubRoleSteps(t,
		func(context.Context, string) (*pgxpool.Pool, error) {
			return nil, errors.New("failed to connect to `host=127.0.0.1 user=probe_role`: driver-text-marker")
		},
		func(context.Context, db.RowQuerier, string) (db.RoleVerdict, error) {
			t.Error("the role was checked although no pool opened")
			return db.RoleOK, nil
		},
	)

	_, err := (&dbLocker{dsn: probeRoleDSN}).openChecked(context.Background())

	if !errors.Is(err, errOpenDatabase) {
		t.Errorf("error = %v, want errOpenDatabase", err)
	}
}

// The probe's results are on stdout, so its warning goes to stderr unless a test
// stands in for it.
func TestProbeWarningsGoToStderrByDefault(t *testing.T) {
	if roleWarnings != io.Writer(os.Stderr) {
		t.Errorf("roleWarnings = %v, want os.Stderr", fmt.Sprintf("%T", roleWarnings))
	}
}

// A refused role is not a lock that could not be taken: the message says what is wrong,
// which is the role, and that no Doffin call was made. The lock is never mentioned,
// because it was never tried.
func TestLockFailureForARoleRefusalDoesNotBlameTheLock(t *testing.T) {
	for _, cause := range []error{db.ErrRoleTooPrivileged, db.ErrRoleCheckFailed} {
		got := lockFailure(cause)
		if !errors.Is(got, cause) {
			t.Errorf("lockFailure(%v) = %v, which does not wrap its cause", cause, got)
		}
		if strings.Contains(got.Error(), "budget lock") {
			t.Errorf("lockFailure(%v) = %q blames the budget lock for a role refusal", cause, got)
		}
		if !strings.Contains(got.Error(), "no Doffin calls") {
			t.Errorf("lockFailure(%v) = %q does not say that no Doffin call was made", cause, got)
		}
	}
}
