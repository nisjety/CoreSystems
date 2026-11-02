package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/rawstore"
)

// Doffin allows 30 searches and 10 downloads a minute per key. doffin-sync
// serialises on a Postgres advisory lock, the budget lock, so that one process
// spends that budget at a time. The probe takes the same lock whenever ANBUD_DSN
// names the database, so it cannot overspend the budget while a sync runs.

// envDSN names the variable that holds the database DSN, the one doffin-sync reads.
const envDSN = "ANBUD_DSN"

// Exit codes. exitFailed is the code for usage, configuration and a failed probe,
// as it has always been. exitLocked is the code doffin-sync uses when another sync
// holds the budget lock; the probe uses it for the same condition.
const (
	exitFailed = 1
	exitLocked = 7
)

// releaseTimeout bounds the lock release. The release runs on a context that a time
// limit cannot end, so it needs a bound of its own, as in doffin-sync.
const releaseTimeout = 10 * time.Second

// What the header says about the lock.
const (
	lockHeldLine     = "budget lock: held"
	lockNotTakenLine = "budget lock: not taken (" + envDSN + " is not set); do not run this while a sync is running"
)

// errBudgetLocked ends a run that found the budget lock held by another process.
// The probe made no Doffin call. finish turns it into exit code 7.
var errBudgetLocked = errors.New("another sync holds the Doffin budget lock; the probe made no Doffin calls")

// The real locker's errors have fixed texts. A pgx error can quote the connection
// details, so none of one is passed on.
var (
	errOpenDatabase = errors.New("could not open the database; check " + envDSN)
	errTakeLock     = errors.New("could not take the budget lock")
)

// budgetLocker is the budget lock as the probe uses it: take it without waiting,
// and give it back. dbLocker is the real one. Tests stand in a fake, so the lock
// step runs without a database.
type budgetLocker interface {
	// TryLock returns rawstore.ErrLocked when another session holds the lock.
	TryLock(ctx context.Context) error
	// Release gives the lock back and frees whatever TryLock opened. It is called
	// only after a TryLock that succeeded.
	Release(ctx context.Context) error
}

// The steps of opening the pool, as variables so a test can stand in for them: the
// pool opener, the runtime-role check (O2) and where its warning goes. The probe's
// results are on stdout, so the warning is on stderr.
//
// These are package-level variables that tests overwrite, which is safe only because the tests
// that do (stubRoleSteps) never call t.Parallel and restore the originals with t.Cleanup, and
// the tests that do run in parallel stand in a fake budgetLocker and never reach dbLocker. A test
// that replaced them while another ran in parallel would race; do not make one.
var (
	openPool                   = db.Open
	checkRuntimeRole           = db.CheckRuntimeRole
	roleWarnings     io.Writer = os.Stderr
)

// dbLocker is the real budgetLocker. TryLock opens a pool on the DSN, checks the role
// the DSN names, and takes the lock through rawstore, on a connection of its own that
// lives as long as the lock. Release unlocks, then closes the pool. It applies no
// migration and writes nothing.
type dbLocker struct {
	dsn  string // already checked by db.CheckTLS; never printed
	pool *pgxpool.Pool
	lock *rawstore.Lock
}

// openChecked opens the pool and applies the runtime-role rule to its role, the rule
// doffin-sync applies to the pool it writes through (db.CheckRuntimeRole): a role that is a
// superuser, holds CREATEROLE, CREATEDB or BYPASSRLS or belongs to a role that does, belongs to
// a predefined role that reaches files, programs or other sessions or to the owner or DBA
// roles of the role model, or owns a table, view, sequence, function, schema or the
// database, is refused when the host is not loopback, and on a loopback host is one warning
// line and the run goes on. Every error is fixed text, and the pool is closed on each of them.
func (l *dbLocker) openChecked(ctx context.Context) (*pgxpool.Pool, error) {
	pool, err := openPool(ctx, l.dsn)
	if err != nil {
		return nil, errOpenDatabase
	}
	verdict, err := checkRuntimeRole(ctx, pool, l.dsn)
	switch {
	case errors.Is(err, db.ErrRoleTooPrivileged):
		pool.Close()
		return nil, db.ErrRoleTooPrivileged
	case err != nil:
		pool.Close()
		return nil, db.ErrRoleCheckFailed
	case verdict == db.RoleWarn:
		_, _ = fmt.Fprintln(roleWarnings, "doffin-probe: "+db.RoleWarning)
	}
	return pool, nil
}

func (l *dbLocker) TryLock(ctx context.Context) error {
	pool, err := l.openChecked(ctx)
	if err != nil {
		return err
	}
	lock, err := rawstore.New(pool).TryLock(ctx)
	switch {
	case errors.Is(err, rawstore.ErrLocked):
		pool.Close()
		return rawstore.ErrLocked
	case err != nil:
		pool.Close()
		return errTakeLock
	}
	l.pool, l.lock = pool, lock
	return nil
}

func (l *dbLocker) Release(ctx context.Context) error {
	err := l.lock.Release(ctx)
	if l.pool != nil {
		l.pool.Close()
	}
	l.lock, l.pool = nil, nil
	return err
}

// lockFromEnv returns the locker for the database ANBUD_DSN names, or nil when the
// variable is unset or blank, and then the probe takes no lock. The DSN is read once.
// It is never printed and never put in an error. A DSN that would send the password
// to a host that is not loopback without verifying its certificate is refused here,
// before any connection, by the rule doffin-sync applies.
func lockFromEnv() (budgetLocker, error) {
	dsn := strings.TrimSpace(os.Getenv(envDSN))
	if dsn == "" {
		return nil, nil
	}
	if err := db.CheckTLS(dsn); err != nil {
		return nil, err
	}
	return &dbLocker{dsn: dsn}, nil
}

// lockFailure is the error for a budget lock that could not be taken. The probe made
// no Doffin call either way.
func lockFailure(err error) error {
	switch {
	case errors.Is(err, rawstore.ErrLocked):
		return errBudgetLocked
	case errors.Is(err, db.ErrRoleTooPrivileged), errors.Is(err, db.ErrRoleCheckFailed):
		// Refused before the lock was tried: it is the role that is wrong, not the lock.
		return fmt.Errorf("the probe made no Doffin calls: %w", err)
	}
	return fmt.Errorf("the budget lock could not be taken, so the probe made no Doffin calls: %w", err)
}

// finish prints err, if there is one, and returns the exit code: 0 for none, 7 when
// another sync holds the budget lock, and 1 for any other error.
func finish(err error, stderr io.Writer) int {
	if err == nil {
		return 0
	}
	_, _ = fmt.Fprintln(stderr, "doffin-probe:", err)
	if errors.Is(err, errBudgetLocked) {
		return exitLocked
	}
	return exitFailed
}

// releaseLock gives the budget lock back on a context that a time limit or a signal
// cannot cancel, with a bound of its own. A release that fails is reported without
// its error text, which can quote connection details, and does not change the
// probe's result: the lock's session is closed either way, and that frees the lock.
func (p *probe) releaseLock(parent context.Context) {
	ctx, cancel := context.WithTimeout(context.WithoutCancel(parent), releaseTimeout)
	defer cancel()
	p.lockHeld = false
	if err := p.locker.Release(ctx); err != nil {
		p.printf("\nnote: the budget lock could not be released cleanly; closing its session frees it\n")
	}
}

// lockLine is what the header says about the budget lock.
func (p *probe) lockLine() string {
	if p.lockHeld {
		return lockHeldLine
	}
	return lockNotTakenLine
}
