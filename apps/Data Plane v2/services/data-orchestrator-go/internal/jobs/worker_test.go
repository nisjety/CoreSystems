package jobs

import (
	"context"
	"testing"
	"time"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

// seedPending inserts a pending job the way CreateJob would.
func seedPending(t *testing.T, store *memoryJobStore, jobID, orgID string, created time.Time) {
	t.Helper()
	job := model.Job{
		JobID:          jobID,
		OrgID:          orgID,
		JobType:        model.JobReindex,
		Status:         model.StatusPending,
		DocumentIDs:    []string{"doc-1"},
		Total:          1,
		IdempotencyKey: "key-" + jobID,
		CreatedAt:      created,
	}
	store.jobs[orgID+"/"+jobID] = &job
}

func TestClaimTakesOldestPendingJobAndLeasesIt(t *testing.T) {
	store := newMemoryJobStore()
	now := time.Now()
	seedPending(t, store, "job-new", "org-a", now)
	seedPending(t, store, "job-old", "org-a", now.Add(-time.Hour))

	claimed, err := store.ClaimNext(context.Background(), "worker-1", time.Minute, MaxAttempts)
	if err != nil {
		t.Fatalf("ClaimNext: %v", err)
	}
	if claimed == nil {
		t.Fatal("ClaimNext returned nothing; a pending job was available")
	}
	if claimed.JobID != "job-old" {
		t.Fatalf("claimed %q, want the oldest job (job-old) — FIFO or an operator's first request starves", claimed.JobID)
	}
	if claimed.Status != model.StatusRunning || claimed.StartedAt == nil {
		t.Fatalf("claim must mark the job running with a start time; got %+v", claimed)
	}
	if store.attempts["job-old"] != 1 {
		t.Fatalf("attempts = %d, want 1 — attempts must count on CLAIM so a worker that dies silently still burns one",
			store.attempts["job-old"])
	}
	if store.leaseOwner["job-old"] != "worker-1" {
		t.Fatalf("lease_owner = %q, want worker-1 (a stuck lease must be traceable to a replica)", store.leaseOwner["job-old"])
	}
}

func TestLeasedJobIsNotStolenWhileTheLeaseHolds(t *testing.T) {
	store := newMemoryJobStore()
	seedPending(t, store, "job-1", "org-a", time.Now())

	first, err := store.ClaimNext(context.Background(), "worker-1", time.Minute, MaxAttempts)
	if err != nil || first == nil {
		t.Fatalf("first claim: job=%v err=%v", first, err)
	}
	second, err := store.ClaimNext(context.Background(), "worker-2", time.Minute, MaxAttempts)
	if err != nil {
		t.Fatalf("second claim: %v", err)
	}
	if second != nil {
		t.Fatal("a live lease was stolen; two workers would run the same job concurrently")
	}
}

func TestExpiredLeaseIsReclaimed(t *testing.T) {
	// The crash-recovery property: a worker that died mid-job leaves a `running`
	// row whose lease expires, and another worker must pick it up. Under the
	// previous fire-and-forget design this row was stranded forever.
	store := newMemoryJobStore()
	seedPending(t, store, "job-1", "org-a", time.Now())

	if _, err := store.ClaimNext(context.Background(), "worker-dead", time.Millisecond, MaxAttempts); err != nil {
		t.Fatalf("initial claim: %v", err)
	}
	time.Sleep(5 * time.Millisecond) // let the lease lapse

	reclaimed, err := store.ClaimNext(context.Background(), "worker-live", time.Minute, MaxAttempts)
	if err != nil {
		t.Fatalf("reclaim: %v", err)
	}
	if reclaimed == nil {
		t.Fatal("expired lease was not reclaimed; a crashed worker's job would be stranded")
	}
	if store.leaseOwner["job-1"] != "worker-live" {
		t.Fatalf("lease_owner = %q, want worker-live", store.leaseOwner["job-1"])
	}
	if store.attempts["job-1"] != 2 {
		t.Fatalf("attempts = %d, want 2 after a reclaim", store.attempts["job-1"])
	}
}

func TestExhaustedJobStopsBeingClaimedAndIsFailedTerminally(t *testing.T) {
	store := newMemoryJobStore()
	seedPending(t, store, "job-1", "org-a", time.Now())

	// Burn every attempt with an immediately-expiring lease.
	for i := 0; i < MaxAttempts; i++ {
		if _, err := store.ClaimNext(context.Background(), "worker-1", time.Nanosecond, MaxAttempts); err != nil {
			t.Fatalf("claim %d: %v", i, err)
		}
		time.Sleep(time.Millisecond)
	}

	if again, err := store.ClaimNext(context.Background(), "worker-1", time.Minute, MaxAttempts); err != nil || again != nil {
		t.Fatalf("exhausted job must not be claimable again; got job=%v err=%v", again, err)
	}

	n, err := store.ExpireExhausted(context.Background(), MaxAttempts)
	if err != nil {
		t.Fatalf("ExpireExhausted: %v", err)
	}
	if n != 1 {
		t.Fatalf("expired %d jobs, want 1 — otherwise it sits in `running` forever, never reported failed", n)
	}
	final, err := store.Get(context.Background(), "org-a", "job-1")
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if final.Status != model.StatusFailed || final.ErrorMessage == nil || final.CompletedAt == nil {
		t.Fatalf("terminal shape violated: %+v (status/error_message/completed_at are all required together)", final)
	}
}

func TestEmptyQueueClaimsNothing(t *testing.T) {
	store := newMemoryJobStore()
	job, err := store.ClaimNext(context.Background(), "worker-1", time.Minute, MaxAttempts)
	if err != nil {
		t.Fatalf("ClaimNext on empty queue must not error: %v", err)
	}
	if job != nil {
		t.Fatalf("claimed %+v from an empty queue", job)
	}
}
