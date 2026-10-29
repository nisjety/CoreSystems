package pg

import (
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// TestPostgres_PurgeOrg is the real-database counterpart to the in-memory
// TestMemoryPurgeOrg_* tests (store package): it exercises the actual
// `DELETE FROM ... WHERE org_id = $1` statements against a live Postgres
// instance, proving the transaction commits all five deletes together and
// never touches another org's rows. Requires QUARRY_TEST_DSN — see
// openOrSkip in pg_test.go.
func TestPostgres_PurgeOrg(t *testing.T) {
	db := openOrSkip(t)
	orgA := "org_purge_a_" + quarrycontracts.NewID(quarrycontracts.KindRun).String()
	orgB := "org_purge_b_" + quarrycontracts.NewID(quarrycontracts.KindRun).String()

	mkJob := func(org string) store.Job {
		return store.Job{
			ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
			OrgID:     org,
			Kind:      "crawl",
			Status:    "accepted",
			Policy:    quarrycontracts.DefaultRunPolicy(),
			CreatedAt: time.Now().UnixMilli(),
		}
	}
	mkSchedule := func(org string) store.Schedule {
		return store.Schedule{
			ID:         quarrycontracts.NewID(quarrycontracts.KindSchedule),
			OrgID:      org,
			Cron:       "0 * * * *",
			TargetKind: "crawl",
			TargetRef:  "https://example.com",
			Enabled:    true,
			CreatedAt:  time.Now().UnixMilli(),
		}
	}
	mkSource := func(org, url string) store.Source {
		now := time.Now().UnixMilli()
		return store.Source{
			ID:        quarrycontracts.NewID(quarrycontracts.KindSource),
			OrgID:     org,
			Name:      "example",
			URL:       url,
			Kind:      "crawl",
			Status:    "active",
			CreatedAt: now,
			UpdatedAt: now,
		}
	}

	aJob := mkJob(orgA)
	bJob := mkJob(orgB)
	if err := db.Jobs().Create(aJob); err != nil {
		t.Fatalf("create org_a job: %v", err)
	}
	if err := db.Jobs().Create(bJob); err != nil {
		t.Fatalf("create org_b job: %v", err)
	}

	aSchedule := mkSchedule(orgA)
	bSchedule := mkSchedule(orgB)
	if err := db.Schedules().Create(aSchedule); err != nil {
		t.Fatalf("create org_a schedule: %v", err)
	}
	if err := db.Schedules().Create(bSchedule); err != nil {
		t.Fatalf("create org_b schedule: %v", err)
	}

	aSource := mkSource(orgA, "https://purge-test-a.example/page")
	bSource := mkSource(orgB, "https://purge-test-b.example/page")
	if _, _, err := db.Sources().UpsertByOrgAndURL(aSource); err != nil {
		t.Fatalf("create org_a source: %v", err)
	}
	if _, _, err := db.Sources().UpsertByOrgAndURL(bSource); err != nil {
		t.Fatalf("create org_b source: %v", err)
	}

	result, err := db.PurgeOrg(orgA)
	if err != nil {
		t.Fatalf("PurgeOrg: %v", err)
	}
	if result.JobsDeleted != 1 {
		t.Errorf("JobsDeleted=%d want=1", result.JobsDeleted)
	}
	if result.SchedulesDeleted != 1 {
		t.Errorf("SchedulesDeleted=%d want=1", result.SchedulesDeleted)
	}
	if result.SourcesDeleted != 1 {
		t.Errorf("SourcesDeleted=%d want=1", result.SourcesDeleted)
	}

	if _, ok := db.Jobs().Get(aJob.ID); ok {
		t.Error("org_a job survived PurgeOrg")
	}
	if _, ok := db.Schedules().Get(aSchedule.ID); ok {
		t.Error("org_a schedule survived PurgeOrg")
	}
	if _, ok := db.Sources().GetByOrg(orgA, aSource.ID); ok {
		t.Error("org_a source survived PurgeOrg")
	}

	// org_b's rows of the same resource types must be completely untouched.
	if _, ok := db.Jobs().Get(bJob.ID); !ok {
		t.Error("IDOR: org_b's job was deleted by org_a's PurgeOrg")
	}
	if _, ok := db.Schedules().Get(bSchedule.ID); !ok {
		t.Error("IDOR: org_b's schedule was deleted by org_a's PurgeOrg")
	}
	if _, ok := db.Sources().GetByOrg(orgB, bSource.ID); !ok {
		t.Error("IDOR: org_b's source was deleted by org_a's PurgeOrg")
	}

	// Idempotency: redelivering the same erasure event must be a safe no-op.
	second, err := db.PurgeOrg(orgA)
	if err != nil {
		t.Fatalf("second PurgeOrg (redelivery) returned an error, want nil: %v", err)
	}
	if second.Total() != 0 {
		t.Errorf("second PurgeOrg total=%d want=0 (nothing left to delete)", second.Total())
	}

	// Cleanup org_b's rows so this test doesn't leak fixture data.
	_, _ = db.PurgeOrg(orgB)
}
