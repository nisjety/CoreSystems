package cron

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// errInjected is the statement error the fakes below return.
var errInjected = errors.New("injected statement failure")

// failingTx fails the statement whose SQL contains failOn and records every
// statement it is asked to run, in order.
type failingTx struct {
	pgx.Tx
	failOn string
	sqls   []string
}

func (tx *failingTx) Exec(_ context.Context, sql string, _ ...any) (pgconn.CommandTag, error) {
	tx.sqls = append(tx.sqls, sql)
	if strings.Contains(sql, tx.failOn) {
		return pgconn.CommandTag{}, errInjected
	}
	return pgconn.CommandTag{}, nil
}

// Nothing may run after a failed statement, because Postgres rejects every later
// statement on the aborted transaction. A fire therefore stops at its first error
// and hands that error back, and the caller rolls the fire's savepoint back.
func TestFireOneReturnsTheFailedStatement(t *testing.T) {
	slot := time.Date(2026, 10, 26, 9, 0, 0, 0, time.UTC)
	cases := []struct {
		name     string
		failOn   string
		wantSQLs int
	}{
		{name: "task insert fails", failOn: "INSERT INTO tasks", wantSQLs: 1},
		{name: "pending fire row fails", failOn: "INSERT INTO cron_fires", wantSQLs: 2},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			tx := &failingTx{failOn: tc.failOn}
			err := (&Sweeper{}).fireOne(context.Background(), tx, dueSchedule{
				id:             "sched-1",
				orgID:          "org-1",
				spaceRef:       "space-1",
				subjectID:      "subject-1",
				templateDigest: "sha256:" + strings.Repeat("a", 64),
				template:       []byte(`{"kind":"cron","title":"Nightly"}`),
				nextFire:       &slot,
			}, slot.Add(time.Second))
			if !errors.Is(err, errInjected) {
				t.Fatalf("fireOne returned %v, want the failed statement's error", err)
			}
			if len(tx.sqls) != tc.wantSQLs {
				t.Fatalf("fireOne issued %d statements, want %d: %q", len(tx.sqls), tc.wantSQLs, tx.sqls)
			}
		})
	}
}

// fakeTx stands in for the batch transaction. Begin hands out savepoints that record
// whether they were released or rolled back.
type fakeTx struct {
	pgx.Tx
	savepoints []*fakeSavepoint
}

func (tx *fakeTx) Begin(context.Context) (pgx.Tx, error) {
	sp := &fakeSavepoint{}
	tx.savepoints = append(tx.savepoints, sp)
	return sp, nil
}

type fakeSavepoint struct {
	pgx.Tx
	closed string
}

func (sp *fakeSavepoint) Commit(context.Context) error {
	sp.closed = "released"
	return nil
}

func (sp *fakeSavepoint) Rollback(context.Context) error {
	sp.closed = "rolled back"
	return nil
}

// A failing schedule's savepoint must be rolled back and its error handed back,
// while a healthy schedule's savepoint is released as usual.
func TestInSavepointRollsBackOnlyTheFailedSchedule(t *testing.T) {
	ctx := context.Background()
	tx := &fakeTx{}
	if err := inSavepoint(ctx, tx, func(pgx.Tx) error { return nil }); err != nil {
		t.Fatalf("healthy schedule returned %v", err)
	}
	if err := inSavepoint(ctx, tx, func(pgx.Tx) error { return errInjected }); !errors.Is(err, errInjected) {
		t.Fatalf("failing schedule returned %v, want the statement error", err)
	}
	if len(tx.savepoints) != 2 {
		t.Fatalf("opened %d savepoints, want 2", len(tx.savepoints))
	}
	if got := tx.savepoints[0].closed; got != "released" {
		t.Fatalf("healthy schedule's savepoint was %q, want released", got)
	}
	if got := tx.savepoints[1].closed; got != "rolled back" {
		t.Fatalf("failing schedule's savepoint was %q, want rolled back", got)
	}
}
