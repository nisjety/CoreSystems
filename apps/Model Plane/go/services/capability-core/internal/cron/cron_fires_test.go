package cron

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// recordingTx captures the statements one fire writes. Only Exec is reached.
type recordingTx struct {
	pgx.Tx
	execs []recordedExec
}

type recordedExec struct {
	sql  string
	args []any
}

func (tx *recordingTx) Exec(_ context.Context, sql string, args ...any) (pgconn.CommandTag, error) {
	tx.execs = append(tx.execs, recordedExec{sql: sql, args: args})
	return pgconn.CommandTag{}, nil
}

// A fire row names the slot it fired. Session Core matches the scheduled run
// back to the fire through that key, so a fire without it can never be settled.
func TestFireRowCarriesFireKey(t *testing.T) {
	slot := time.Date(2026, 10, 23, 9, 30, 0, 0, time.UTC)
	wantKey := slot.Format(time.RFC3339Nano)
	tx := &recordingTx{}
	(&Sweeper{}).fireOne(context.Background(), tx, dueSchedule{
		id:             "sched-1",
		orgID:          "org-1",
		spaceRef:       "space-1",
		subjectID:      "subject-1",
		templateDigest: "sha256:" + strings.Repeat("a", 64),
		template:       []byte(`{"kind":"cron","title":"Nightly"}`),
		nextFire:       &slot,
	}, slot.Add(time.Second))

	if len(tx.execs) != 2 {
		t.Fatalf("a fire writes the task and the fire row, got %d statements", len(tx.execs))
	}
	fire := tx.execs[1]
	if !strings.Contains(fire.sql, "INSERT INTO cron_fires") ||
		!strings.Contains(fire.sql, "fire_key") ||
		!strings.Contains(fire.sql, "'pending'") {
		t.Fatalf("fire row must be a pending row that carries fire_key: %s", fire.sql)
	}
	for _, arg := range fire.args {
		if arg == wantKey {
			return
		}
	}
	t.Fatalf("fire args %v do not carry the slot key %q", fire.args, wantKey)
}
