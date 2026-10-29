package store

import (
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
)

func newJob(org, kind string, createdAt int64) Job {
	return Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		OrgID:     org,
		Kind:      kind,
		Status:    "accepted",
		Policy:    quarrycontracts.DefaultRunPolicy(),
		CreatedAt: createdAt,
	}
}

// TestMemoryJobs_OrgScopedListAndGet mirrors
// TestMemorySources_OrgScopedCRUD — ListByOrg and GetByOrg must never let
// one org see or fetch another org's jobs.
func TestMemoryJobs_OrgScopedListAndGet(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	j := db.Jobs()

	now := time.Now().UnixMilli()
	a1 := newJob("org_a", "crawl", now)
	a2 := newJob("org_a", "scrape", now+1)
	b1 := newJob("org_b", "crawl", now+2)
	for _, job := range []Job{a1, a2, b1} {
		if err := j.Create(job); err != nil {
			t.Fatalf("create %s: %v", job.ID, err)
		}
	}

	// ListByOrg returns only the caller's org rows.
	aList, _ := j.ListByOrg("org_a", 50, "")
	if len(aList) != 2 {
		t.Fatalf("org_a list len=%d want=2", len(aList))
	}
	bList, _ := j.ListByOrg("org_b", 50, "")
	if len(bList) != 1 {
		t.Fatalf("org_b list len=%d want=1", len(bList))
	}
	if bList[0].ID != b1.ID {
		t.Fatalf("org_b list returned %s want=%s", bList[0].ID, b1.ID)
	}

	// GetByOrg is org-guarded: org_b CANNOT read org_a's job by id.
	if _, ok := j.GetByOrg("org_b", a1.ID); ok {
		t.Fatal("IDOR: org_b read org_a's job via GetByOrg")
	}
	if got, ok := j.GetByOrg("org_a", a1.ID); !ok || got.ID != a1.ID {
		t.Fatalf("org_a could not read its own job: ok=%v", ok)
	}
}

// TestMemoryJobs_ListByKindIsOrgScoped guards the GET /v1/{kind}/jobs path:
// a job with the right Kind but a different OrgID must never leak into
// another tenant's kind-filtered list.
func TestMemoryJobs_ListByKindIsOrgScoped(t *testing.T) {
	t.Parallel()
	db := NewMemory()
	j := db.Jobs()

	now := time.Now().UnixMilli()
	aCrawl := newJob("org_a", "crawl", now)
	aScrape := newJob("org_a", "scrape", now+1)
	bCrawl := newJob("org_b", "crawl", now+2)
	for _, job := range []Job{aCrawl, aScrape, bCrawl} {
		if err := j.Create(job); err != nil {
			t.Fatalf("create %s: %v", job.ID, err)
		}
	}

	items, _ := j.ListByKind("org_a", "crawl", 50, "")
	if len(items) != 1 || items[0].ID != aCrawl.ID {
		t.Fatalf("org_a crawl jobs = %v, want only %s", items, aCrawl.ID)
	}

	// org_b's crawl job must not appear in org_a's list, and vice versa.
	bItems, _ := j.ListByKind("org_b", "crawl", 50, "")
	if len(bItems) != 1 || bItems[0].ID != bCrawl.ID {
		t.Fatalf("org_b crawl jobs = %v, want only %s", bItems, bCrawl.ID)
	}
}
