package rawstore

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db/dbtest"
	"github.com/triodelab/anbud/internal/doffinsync"
)

// fixedNow is the engine clock of every database test. The windows used are
// closed days before it, so the window rules pass without depending on the wall clock.
var fixedNow = time.Date(2026, time.October, 9, 12, 0, 0, 0, time.UTC)

// openStore returns a store over a fresh, migrated throwaway database.
func openStore(t *testing.T) (*Store, *pgxpool.Pool) {
	t.Helper()
	pool := dbtest.Open(t)
	return New(pool), pool
}

// testCtx bounds one test's database work.
func testCtx(t *testing.T) context.Context {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	t.Cleanup(cancel)
	return ctx
}

// utcDay parses a YYYY-MM-DD label into a UTC midnight.
func utcDay(t *testing.T, label string) time.Time {
	t.Helper()
	d, err := time.Parse("2006-01-02", label)
	if err != nil {
		t.Fatalf("parse day %q: %v", label, err)
	}
	return d.UTC()
}

// beginSyncRun opens a sync run over [from, to] and returns its id.
func beginSyncRun(t *testing.T, s *Store, from, to string) int64 {
	t.Helper()
	id, err := s.BeginRun(testCtx(t), doffinsync.RunStart{
		Mode: doffinsync.ModeSync,
		From: utcDay(t, from),
		To:   utcDay(t, to),
	})
	if err != nil {
		t.Fatalf("BeginRun: %v", err)
	}
	return id
}

// pgCode returns the SQLSTATE of err, or "" when err is not a PostgreSQL error.
func pgCode(err error) string {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		return pgErr.Code
	}
	return ""
}

// count returns the single integer a query yields.
func count(t *testing.T, pool *pgxpool.Pool, query string, args ...any) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(testCtx(t), query, args...).Scan(&n); err != nil {
		t.Fatalf("count query %q: %v", query, err)
	}
	return n
}
