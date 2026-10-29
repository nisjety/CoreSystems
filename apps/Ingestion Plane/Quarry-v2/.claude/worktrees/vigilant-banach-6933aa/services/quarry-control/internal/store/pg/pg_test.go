package pg

import (
	"context"
	"os"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// envDSN gates real-database tests. Set QUARRY_TEST_DSN to e.g.
//
//	postgres://postgres:postgres@localhost:5432/quarry_test?sslmode=disable
//
// to exercise the migrator + Postgres CRUD path end-to-end.
const envDSN = "QUARRY_TEST_DSN"

func openOrSkip(t *testing.T) store.DB {
	t.Helper()
	dsn := os.Getenv(envDSN)
	if dsn == "" {
		t.Skipf("%s unset — skipping postgres integration test", envDSN)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	db, err := New(ctx, dsn)
	if err != nil {
		t.Fatalf("pg.New: %v", err)
	}
	return db
}

func TestPostgres_JobsCRUD(t *testing.T) {
	db := openOrSkip(t)
	j := store.Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		Kind:      "scrape",
		Status:    "accepted",
		Policy:    quarrycontracts.DefaultRunPolicy(),
		CreatedAt: time.Now().UnixMilli(),
	}
	if err := db.Jobs().Create(j); err != nil {
		t.Fatalf("create: %v", err)
	}
	got, ok := db.Jobs().Get(j.ID)
	if !ok || got.ID != j.ID {
		t.Fatalf("get mismatch: %+v ok=%v", got, ok)
	}
	items, _ := db.Jobs().List(10, "")
	if len(items) == 0 {
		t.Fatal("list empty")
	}
	if err := db.Jobs().Delete(j.ID); err != nil {
		t.Fatalf("delete: %v", err)
	}
	if _, ok := db.Jobs().Get(j.ID); ok {
		t.Fatal("expected missing after delete")
	}
}

func TestPostgres_EventAppendDedupe(t *testing.T) {
	db := openOrSkip(t)
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	evt := quarrycontracts.Event{
		EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
		RunID:     &runID,
		Type:      quarrycontracts.EvtPageFetched,
		Timestamp: time.Now().UTC(),
		Seq:       1,
	}
	if err := db.Events().Append(evt); err != nil {
		t.Fatalf("append: %v", err)
	}
	// Same (run_id, seq) must conflict.
	dup := evt
	dup.EventID = quarrycontracts.NewID(quarrycontracts.KindEvent)
	if err := db.Events().Append(dup); err != store.ErrConflict {
		t.Fatalf("expected ErrConflict on (run_id, seq) unique, got %v", err)
	}

	list := db.Events().ForRun(runID, 0, 10)
	if len(list) != 1 {
		t.Fatalf("ForRun len=%d, want 1", len(list))
	}
}
