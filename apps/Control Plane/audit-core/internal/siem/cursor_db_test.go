package siem

import (
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// These tests need Postgres (TEST_DATABASE_URL), like the chain tests, and skip
// without it. They apply migration 010 from its file so they hold whether or
// not store.Migrate lists it yet.
func cursorDB(t *testing.T) (context.Context, *pgxpool.Pool) {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping Postgres integration test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	t.Cleanup(cancel)
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(pool.Close)
	if err := store.Migrate(ctx, pool); err != nil {
		t.Fatalf("migrate: %v", err)
	}
	ddl, err := os.ReadFile("../store/migrations/010_audit_export_cursors.sql")
	if err != nil {
		t.Fatalf("read migration: %v", err)
	}
	if _, err := pool.Exec(ctx, string(ddl)); err != nil {
		t.Fatalf("apply migration 010: %v", err)
	}
	return ctx, pool
}

func TestPGCursorsAdvanceOnlyForwardAndResetTheFailureStreak(t *testing.T) {
	ctx, pool := cursorDB(t)
	cursors := NewPGCursors(pool)
	d := Destination{ID: fmt.Sprintf("dest-test-%d", time.Now().UnixNano()), OrgID: "org-cursor-test", Kind: "ndjson_webhook"}
	t.Cleanup(func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM audit_export_cursors WHERE destination_id = $1`, d.ID)
	})

	if cur, err := cursors.Get(ctx, d.ID); err != nil || cur != (Cursor{}) {
		t.Fatalf("unknown destination = %+v %v, want the zero cursor", cur, err)
	}
	if streak, err := cursors.Fail(ctx, d, "http_500"); err != nil || streak != 1 {
		t.Fatalf("first failure: %d %v", streak, err)
	}
	if streak, err := cursors.Fail(ctx, d, "timeout"); err != nil || streak != 2 {
		t.Fatalf("second failure: %d %v", streak, err)
	}
	if cur, _ := cursors.Get(ctx, d.ID); cur.Seq != 0 || cur.ConsecutiveFailures != 2 {
		t.Fatalf("failures must not move the cursor: %+v", cur)
	}
	if err := cursors.Advance(ctx, d, 40, 40); err != nil {
		t.Fatal(err)
	}
	if err := cursors.Advance(ctx, d, 25, 10); err != nil {
		t.Fatal(err)
	}
	cur, err := cursors.Get(ctx, d.ID)
	if err != nil || cur.Seq != 40 || cur.ConsecutiveFailures != 0 {
		t.Fatalf("after advances: %+v %v, want seq 40 (never backwards) and a reset streak", cur, err)
	}
}

func TestExportStatusReadsTheCursorsOfOneOrgOnly(t *testing.T) {
	ctx, pool := cursorDB(t)
	cursors := NewPGCursors(pool)
	suffix := time.Now().UnixNano()
	mine := Destination{ID: fmt.Sprintf("dest-mine-%d", suffix), OrgID: fmt.Sprintf("org-status-a-%d", suffix), Kind: "splunk_hec"}
	other := Destination{ID: fmt.Sprintf("dest-other-%d", suffix), OrgID: fmt.Sprintf("org-status-b-%d", suffix), Kind: "ndjson_webhook"}
	t.Cleanup(func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM audit_export_cursors WHERE destination_id = ANY($1)`, []string{mine.ID, other.ID})
	})
	if err := cursors.Advance(ctx, mine, 12, 12); err != nil {
		t.Fatal(err)
	}
	if _, err := cursors.Fail(ctx, other, "http_503"); err != nil {
		t.Fatal(err)
	}

	status, err := store.New(pool).ExportStatus(ctx, mine.OrgID)
	if err != nil {
		t.Fatal(err)
	}
	if !status.Active || len(status.Destinations) != 1 {
		t.Fatalf("status = %+v", status)
	}
	got := status.Destinations[0]
	if got.DestinationID != mine.ID || got.Kind != "splunk_hec" || got.CursorSeq != 12 || got.DeliveredTotal != 12 || got.LastError != "" || got.LastSuccessAt == nil {
		t.Fatalf("destination status = %+v", got)
	}
	empty, err := store.New(pool).ExportStatus(ctx, "org-with-no-exports-"+fmt.Sprint(suffix))
	if err != nil || empty.Active || len(empty.Destinations) != 0 {
		t.Fatalf("org without destinations = %+v %v", empty, err)
	}
}
