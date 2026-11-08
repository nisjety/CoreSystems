package pg

import (
	"sync"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// Written in the W2-U32 build (WS6.H). Not run in the build: these need a
// Postgres at QUARRY_TEST_DSN and skip without it.

// addJob inserts one job for the organization with the given status and
// creation time (unix milliseconds).
func addJob(t *testing.T, db store.DB, org, status string, createdMs int64) {
	t.Helper()
	if err := db.Jobs().Create(store.Job{
		ID:        quarrycontracts.NewID(quarrycontracts.KindJob),
		OrgID:     org,
		Kind:      "crawl",
		Status:    status,
		CreatedAt: createdMs,
	}); err != nil {
		t.Fatal(err)
	}
}

// Concurrent claimers must never lease past an organization's cap. The claim
// takes a transaction-scoped advisory lock before it counts the jobs in use,
// so the second claimer sees the first one's leases. Without the lock every
// claimer could count the same zero in use and lease the cap each.
func TestPostgres_ConcurrentClaimsNeverExceedTheCap(t *testing.T) {
	db := openIsolatedOrSkip(t)
	const (
		capPerOrg = 3
		waiting   = 24
		claimers  = 8
	)
	org := "org_race_" + string(quarrycontracts.NewID(quarrycontracts.KindJob))
	base := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)
	for i := 0; i < waiting; i++ {
		addJob(t, db, org, "accepted", base.Add(time.Duration(i)*time.Second).UnixMilli())
	}

	var (
		wg    sync.WaitGroup
		mu    sync.Mutex
		total int
		fails []error
	)
	start := make(chan struct{})
	for c := 0; c < claimers; c++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			got, err := db.Jobs().ClaimAccepted(waiting, base.Add(time.Minute), time.Minute, capPerOrg)
			mu.Lock()
			defer mu.Unlock()
			if err != nil {
				fails = append(fails, err)
				return
			}
			for _, j := range got {
				if j.OrgID == org {
					total++
				}
			}
		}()
	}
	close(start)
	wg.Wait()

	if len(fails) > 0 {
		t.Fatalf("a claim failed: %v", fails[0])
	}
	if total != capPerOrg {
		t.Fatalf("%d concurrent claimers leased %d jobs for one organization with cap %d", claimers, total, capPerOrg)
	}
}

// The team concurrency read counts running jobs and the accepted jobs a
// dispatcher holds a lease on, because a leased job takes a cap slot.
func TestPostgres_ConcurrencyCountsLeasedAcceptedJobs(t *testing.T) {
	db := openIsolatedOrSkip(t)
	org := "org_conc_" + string(quarrycontracts.NewID(quarrycontracts.KindJob))
	now := time.Now()
	addJob(t, db, org, "running", now.Add(-4*time.Second).UnixMilli())
	addJob(t, db, org, "running", now.Add(-3*time.Second).UnixMilli())
	addJob(t, db, org, "accepted", now.Add(-2*time.Second).UnixMilli())
	addJob(t, db, org, "accepted", now.Add(-1*time.Second).UnixMilli())

	// Lease exactly one accepted job for ten minutes (the claim reads the
	// database clock, so the lease is taken from real time).
	claimed, err := db.Jobs().ClaimAccepted(1, now, 10*time.Minute, 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(claimed) != 1 {
		t.Fatalf("claimed %d jobs, want 1", len(claimed))
	}

	row, err := db.TeamUsage().Concurrency(org)
	if err != nil {
		t.Fatal(err)
	}
	if row.Current != 3 {
		t.Fatalf("Current = %d, want 2 running plus 1 leased accepted", row.Current)
	}
}
