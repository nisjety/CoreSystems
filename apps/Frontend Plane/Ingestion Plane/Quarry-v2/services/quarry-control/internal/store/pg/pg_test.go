package pg

import (
	"context"
	"net/url"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store/storetest"
)

// envDSN gates real-database tests. Set QUARRY_TEST_DSN to e.g.
//
//	postgres://postgres:postgres@localhost:5432/quarry_test?sslmode=disable
//
// to exercise the migrator + Postgres CRUD path end-to-end.
const envDSN = "QUARRY_TEST_DSN"

// envRequireDB turns the "no database" skip into a failure. The CI job that
// provides a Postgres service sets it to 1, so a lane that lost its database
// (a renamed secret, a dropped service block) is red instead of green with
// every persistence test silently skipped.
const envRequireDB = "QUARRY_REQUIRE_DB"

// testDSN returns the database the integration tests run against. Unset, it
// skips the test, unless QUARRY_REQUIRE_DB=1, where it fails it.
func testDSN(t *testing.T) string {
	t.Helper()
	dsn := os.Getenv(envDSN)
	if dsn == "" {
		if os.Getenv(envRequireDB) == "1" {
			t.Fatalf("%s is unset but %s=1: the postgres integration tests must run here", envDSN, envRequireDB)
		}
		t.Skipf("%s unset — skipping postgres integration test", envDSN)
	}
	return dsn
}

func openOrSkip(t *testing.T) store.DB {
	t.Helper()
	dsn := testDSN(t)
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

// TestPostgres_JobsGetByRunID is the real-database counterpart to
// TestMemoryJobs's in-memory GetByRunID coverage. Exercises the actual
// `WHERE run_id=$1` query against the partial index from
// 006_jobs_run_id.sql — the query resolving GET /v1/runs/:id/events'
// owning job (org_id/kind) for the JobHistoryEvent translation.
func TestPostgres_JobsGetByRunID(t *testing.T) {
	db := openOrSkip(t)
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	j := store.Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		OrgID:     "org_getbyrunid_test",
		Kind:      "crawl",
		Status:    "running",
		Policy:    quarrycontracts.DefaultRunPolicy(),
		CreatedAt: time.Now().UnixMilli(),
		RunID:     &runID,
	}
	if err := db.Jobs().Create(j); err != nil {
		t.Fatalf("create: %v", err)
	}
	defer func() { _ = db.Jobs().Delete(j.ID) }()

	got, ok := db.Jobs().GetByRunID(runID)
	if !ok {
		t.Fatal("GetByRunID: not found")
	}
	if got.ID != j.ID || got.OrgID != j.OrgID || got.Kind != j.Kind {
		t.Fatalf("GetByRunID mismatch: %+v", got)
	}

	if _, ok := db.Jobs().GetByRunID(quarrycontracts.NewID(quarrycontracts.KindRun)); ok {
		t.Fatal("expected miss for an unassigned run_id")
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

// Statistics are written once, by the terminal event, and read back by every
// jobs query: Get, GetByOrg, GetByRunID, the three lists and the schedule list.
// A later status-only update must not erase them.
func TestPostgres_JobStatisticsRoundTrip(t *testing.T) {
	db := openOrSkip(t)
	org := "org_stats_" + string(quarrycontracts.NewID(quarrycontracts.KindJob))
	runID := quarrycontracts.NewID(quarrycontracts.KindRun)
	id := quarrycontracts.NewID(quarrycontracts.KindJob)
	if err := db.Jobs().Create(store.Job{
		ID: id, OrgID: org, Kind: "crawl", Status: "running", RunID: &runID,
		CreatedAt: time.Now().UnixMilli(),
	}); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = db.Jobs().Delete(id) })

	if job, ok := db.Jobs().Get(id); !ok || job.Statistics != nil {
		t.Fatalf("a new job has no statistics: %#v ok=%v", job, ok)
	}

	job, _ := db.Jobs().Get(id)
	job.Status = "completed"
	job.Statistics = map[string]any{
		"pages_visited":  float64(5),
		"status_classes": map[string]any{"2xx": float64(4), "4xx": float64(1)},
	}
	if err := db.Jobs().Update(job); err != nil {
		t.Fatal(err)
	}

	check := func(where string, got store.Job, ok bool) {
		t.Helper()
		if !ok {
			t.Fatalf("%s: job not found", where)
		}
		if got.Status != "completed" || got.Statistics["pages_visited"] != float64(5) {
			t.Fatalf("%s: %#v", where, got)
		}
		classes, _ := got.Statistics["status_classes"].(map[string]any)
		if classes["4xx"] != float64(1) {
			t.Fatalf("%s: nested counts lost: %#v", where, got.Statistics)
		}
	}
	got, ok := db.Jobs().Get(id)
	check("Get", got, ok)
	got, ok = db.Jobs().GetByOrg(org, id)
	check("GetByOrg", got, ok)
	got, ok = db.Jobs().GetByRunID(runID)
	check("GetByRunID", got, ok)
	for name, list := range map[string][]store.Job{
		"ListByOrg":  first(db.Jobs().ListByOrg(org, 10, "")),
		"ListByKind": first(db.Jobs().ListByKind(org, "crawl", 10, "")),
	} {
		if len(list) != 1 {
			t.Fatalf("%s returned %d jobs", name, len(list))
		}
		check(name, list[0], true)
	}

	// A status-only update (the dispatcher's shape) keeps what the run reported.
	again, _ := db.Jobs().Get(id)
	again.Statistics = nil
	again.Status = "cancelled"
	if err := db.Jobs().Update(again); err != nil {
		t.Fatal(err)
	}
	final, _ := db.Jobs().Get(id)
	if final.Status != "cancelled" || final.Statistics["pages_visited"] != float64(5) {
		t.Fatalf("a status-only update erased the statistics: %#v", final)
	}
}

func first[T any](items []T, _ string) []T { return items }

// openIsolatedOrSkip opens the database in a schema of its own, dropped when
// the test ends. A claim is global across organizations, so a suite that claims
// jobs must not see the rows other tests left behind in the shared schema.
func openIsolatedOrSkip(t *testing.T) store.DB {
	t.Helper()
	dsn := testDSN(t)
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	admin, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("admin pool: %v", err)
	}
	schema := "adm_" + strings.ToLower(string(quarrycontracts.NewID(quarrycontracts.KindJob)))
	if _, err := admin.Exec(ctx, "CREATE SCHEMA "+schema); err != nil {
		admin.Close()
		t.Fatalf("create schema: %v", err)
	}
	t.Cleanup(func() {
		_, _ = admin.Exec(context.Background(), "DROP SCHEMA "+schema+" CASCADE")
		admin.Close()
	})
	u, err := url.Parse(dsn)
	if err != nil {
		t.Fatal(err)
	}
	query := u.Query()
	query.Set("search_path", schema)
	u.RawQuery = query.Encode()
	db, err := New(ctx, u.String())
	if err != nil {
		t.Fatalf("pg.New: %v", err)
	}
	return db
}

// The claim statement must behave exactly like the in-memory one: the same
// contract suite runs against both.
func TestPostgres_JobAdmission(t *testing.T) {
	storetest.AdmissionConformance(t, func(t *testing.T) store.DB { return openIsolatedOrSkip(t) })
}
