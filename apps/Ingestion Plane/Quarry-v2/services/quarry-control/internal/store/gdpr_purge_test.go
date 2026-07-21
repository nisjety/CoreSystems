package store

import (
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

func newJobForOrg(org, kind string) Job {
	return Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		OrgID:     org,
		Kind:      kind,
		Status:    "accepted",
		Policy:    quarrycontracts.DefaultRunPolicy(),
		CreatedAt: time.Now().UnixMilli(),
	}
}

func newScheduleForOrg(org string) Schedule {
	return Schedule{
		ID:         quarrycontracts.NewID(quarrycontracts.KindSchedule),
		OrgID:      org,
		Cron:       "0 * * * *",
		TargetKind: "crawl",
		TargetRef:  "https://example.com",
		Enabled:    true,
		CreatedAt:  time.Now().UnixMilli(),
	}
}

// TestMemoryPurgeOrg_HardDeletesOnlyTheTargetOrg proves PurgeOrg removes
// every job/schedule/source belonging to the target org while leaving
// another org's rows of the same resource types completely untouched — the
// same cross-tenant isolation guarantee SoftDeleteByOrg/GetByOrg enforce
// elsewhere in this package, but for a permanent, all-resources purge.
func TestMemoryPurgeOrg_HardDeletesOnlyTheTargetOrg(t *testing.T) {
	t.Parallel()
	db := NewMemory()

	aJob1 := newJobForOrg("org_a", "crawl")
	aJob2 := newJobForOrg("org_a", "scrape")
	bJob := newJobForOrg("org_b", "crawl")
	for _, j := range []Job{aJob1, aJob2, bJob} {
		if err := db.Jobs().Create(j); err != nil {
			t.Fatalf("create job %s: %v", j.ID, err)
		}
	}

	aSchedule := newScheduleForOrg("org_a")
	bSchedule := newScheduleForOrg("org_b")
	for _, s := range []Schedule{aSchedule, bSchedule} {
		if err := db.Schedules().Create(s); err != nil {
			t.Fatalf("create schedule %s: %v", s.ID, err)
		}
	}

	aSource := newSource("org_a", "A blog", "https://a.example/blog")
	bSource := newSource("org_b", "B blog", "https://b.example/blog")
	for _, s := range []Source{aSource, bSource} {
		if err := db.Sources().Create(s); err != nil {
			t.Fatalf("create source %s: %v", s.ID, err)
		}
	}

	result, err := db.PurgeOrg("org_a")
	if err != nil {
		t.Fatalf("PurgeOrg: %v", err)
	}
	if result.JobsDeleted != 2 {
		t.Errorf("JobsDeleted=%d want=2", result.JobsDeleted)
	}
	if result.SchedulesDeleted != 1 {
		t.Errorf("SchedulesDeleted=%d want=1", result.SchedulesDeleted)
	}
	if result.SourcesDeleted != 1 {
		t.Errorf("SourcesDeleted=%d want=1", result.SourcesDeleted)
	}
	if result.Total() != 4 {
		t.Errorf("Total()=%d want=4", result.Total())
	}

	// org_a's rows are gone.
	if _, ok := db.Jobs().Get(aJob1.ID); ok {
		t.Error("org_a job1 survived PurgeOrg")
	}
	if _, ok := db.Jobs().Get(aJob2.ID); ok {
		t.Error("org_a job2 survived PurgeOrg")
	}
	if _, ok := db.Schedules().Get(aSchedule.ID); ok {
		t.Error("org_a schedule survived PurgeOrg")
	}
	if _, ok := db.Sources().GetByOrg("org_a", aSource.ID); ok {
		t.Error("org_a source survived PurgeOrg")
	}

	// org_b's rows of the SAME resource types are completely untouched —
	// the cross-tenant isolation guarantee a purge query must never violate.
	if _, ok := db.Jobs().Get(bJob.ID); !ok {
		t.Error("IDOR: org_b's job was deleted by org_a's PurgeOrg")
	}
	if _, ok := db.Schedules().Get(bSchedule.ID); !ok {
		t.Error("IDOR: org_b's schedule was deleted by org_a's PurgeOrg")
	}
	if _, ok := db.Sources().GetByOrg("org_b", bSource.ID); !ok {
		t.Error("IDOR: org_b's source was deleted by org_a's PurgeOrg")
	}
}

// TestMemoryPurgeOrg_PurgesSoftDeletedSourcesToo proves PurgeOrg closes out
// a soft-delete tombstone left by SoftDeleteByOrg, not just live rows — a
// GDPR erasure must remove tombstones too, since SoftDeleteByOrg alone
// leaves the row (with its URL/name) resident in storage.
func TestMemoryPurgeOrg_PurgesSoftDeletedSourcesToo(t *testing.T) {
	t.Parallel()
	db := NewMemory()

	src := newSource("org_a", "A blog", "https://a.example/blog")
	if err := db.Sources().Create(src); err != nil {
		t.Fatalf("create: %v", err)
	}
	if err := db.Sources().SoftDeleteByOrg("org_a", src.ID); err != nil {
		t.Fatalf("soft delete: %v", err)
	}

	result, err := db.PurgeOrg("org_a")
	if err != nil {
		t.Fatalf("PurgeOrg: %v", err)
	}
	if result.SourcesDeleted != 1 {
		t.Errorf("SourcesDeleted=%d want=1 (soft-deleted row must still be purged)", result.SourcesDeleted)
	}
}

// TestMemoryPurgeOrg_IsIdempotent proves a second PurgeOrg call for the same
// org — the at-least-once NATS redelivery case — is a safe no-op rather
// than an error, matching the fixed contract's idempotency requirement.
func TestMemoryPurgeOrg_IsIdempotent(t *testing.T) {
	t.Parallel()
	db := NewMemory()

	job := newJobForOrg("org_a", "crawl")
	if err := db.Jobs().Create(job); err != nil {
		t.Fatalf("create: %v", err)
	}

	first, err := db.PurgeOrg("org_a")
	if err != nil {
		t.Fatalf("first PurgeOrg: %v", err)
	}
	if first.Total() != 1 {
		t.Fatalf("first PurgeOrg total=%d want=1", first.Total())
	}

	second, err := db.PurgeOrg("org_a")
	if err != nil {
		t.Fatalf("second PurgeOrg (redelivery) returned an error, want nil: %v", err)
	}
	if second.Total() != 0 {
		t.Errorf("second PurgeOrg total=%d want=0 (nothing left to delete)", second.Total())
	}
}

// TestMemoryPurgeOrg_RejectsEmptyOrgID guards against a malformed/blank
// event ever fanning out into a purge with no org boundary at all.
func TestMemoryPurgeOrg_RejectsEmptyOrgID(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	if _, err := db.PurgeOrg(""); err == nil {
		t.Error("PurgeOrg(\"\") = nil error, want an error")
	}
}
