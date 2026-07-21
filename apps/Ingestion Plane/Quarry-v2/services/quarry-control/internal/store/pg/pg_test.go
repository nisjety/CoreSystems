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

// TestPostgres_SourcesUpsertByOrgAndURL is the real-database counterpart to
// the in-memory `TestMemorySources_UpsertByOrgAndURL_CollapsesDuplicates`
// (store package). It exercises the actual `ON CONFLICT (org_id, url) WHERE
// deleted_at IS NULL DO UPDATE ... RETURNING ... (xmax = 0)` SQL against the
// `quarry_sources_org_url_uniq` partial unique index (migration 011) — the
// in-memory store's linear scan can't catch a typo in that SQL, only a live
// Postgres round trip can. 2026-07-20 Aquatiq crawl-to-KB audit fix.
func TestPostgres_SourcesUpsertByOrgAndURL(t *testing.T) {
	db := openOrSkip(t)
	org := "org_upsert_" + quarrycontracts.NewID(quarrycontracts.KindRun).String()

	mk := func() store.Source {
		now := time.Now().UnixMilli()
		return store.Source{
			ID:        quarrycontracts.NewID(quarrycontracts.KindSource),
			OrgID:     org,
			Name:      "example.com",
			URL:       "https://example.com/upsert-test",
			Kind:      "crawl",
			Status:    "active",
			CreatedAt: now,
			UpdatedAt: now,
		}
	}

	first := mk()
	got1, created1, err := db.Sources().UpsertByOrgAndURL(first)
	if err != nil {
		t.Fatalf("first upsert: %v", err)
	}
	if !created1 {
		t.Fatal("first upsert should report created=true")
	}
	if got1.ID != first.ID {
		t.Fatalf("first upsert id=%s want=%s", got1.ID, first.ID)
	}

	// A second page on the same host, same org: must collapse into the
	// SAME row (created=false, same id) rather than erroring or duplicating.
	second := mk()
	got2, created2, err := db.Sources().UpsertByOrgAndURL(second)
	if err != nil {
		t.Fatalf("second upsert: %v", err)
	}
	if created2 {
		t.Fatal("second upsert for the same (org_id, url) should report created=false")
	}
	if got2.ID != first.ID {
		t.Fatalf("second upsert returned id=%s, want the original=%s", got2.ID, first.ID)
	}

	list, _ := db.Sources().ListByOrg(org, 50, "")
	if len(list) != 1 {
		t.Fatalf("expected exactly 1 row after 2 upserts of the same URL, got %d", len(list))
	}

	// Soft-delete then re-register the same URL: the partial unique index
	// (WHERE deleted_at IS NULL) must let a fresh row through instead of
	// permanently blocking re-registration of a once-deleted source.
	if err := db.Sources().SoftDeleteByOrg(org, first.ID); err != nil {
		t.Fatalf("soft delete: %v", err)
	}
	third := mk()
	got3, created3, err := db.Sources().UpsertByOrgAndURL(third)
	if err != nil {
		t.Fatalf("upsert after soft-delete: %v", err)
	}
	if !created3 {
		t.Fatal("upsert after soft-delete of the prior row should report created=true (fresh row)")
	}
	if got3.ID == first.ID {
		t.Fatal("upsert after soft-delete should mint a NEW row, not resurrect the tombstoned one")
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
