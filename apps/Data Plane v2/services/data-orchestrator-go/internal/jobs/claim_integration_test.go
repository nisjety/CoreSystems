package jobs

// Postgres coverage for PostgresJobStore.ClaimNext and ExpireExhausted (P2-4).
//
// worker_test.go already asserts the same four properties, but against
// memoryJobStore — a Go fake that re-implements the claim rules by hand. It can
// prove the *semantics* are the ones we want; it cannot prove the shipped SQL
// implements them. Nothing else in the package executes the real statements, so
// a typo in the claim predicate, a dropped `attempts < $3` guard, or an
// ExpireExhausted that forgot `completed_at` would leave every test green.
//
// These tests run the real queries against a real server, so they cover the
// parts only Postgres can answer: `FOR UPDATE SKIP LOCKED` candidate selection,
// `lease_until <= NOW()` evaluated on the database clock, and the
// `data_orchestrator_jobs_terminal_shape_check` constraint that makes
// status/error_message/completed_at inseparable.
//
// ClaimNext and ExpireExhausted deliberately run on the unscoped pool (they are
// the cross-org halves of the queue), so unlike the scoped store methods they
// need no `dataplane_app` role. Seeding still goes through Create, which is
// scoped — the fixture grants the role for that.

import (
	"context"
	"fmt"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/dataplane/services/data-orchestrator-go/internal/model"
)

func TestPostgresClaimTakesOldestPendingJobAndLeasesIt(t *testing.T) {
	pool := orchestratorIntegrationPool(t)
	store := NewPostgresJobStore(pool)
	ctx := context.Background()
	now := time.Now().UTC()

	// Deliberately two different organizations: the claim is cross-org by
	// design, so this also proves it reaches past a single tenant. A scoped
	// claim would see only one of these rows and still return successfully.
	newer := seedPendingJob(ctx, t, store, "org-claim-a", "claim-fifo-newer", now)
	older := seedPendingJob(ctx, t, store, "org-claim-b", "claim-fifo-older", now.Add(-time.Hour))

	claimed, err := store.ClaimNext(ctx, "worker-1", time.Minute, MaxAttempts)
	if err != nil {
		t.Fatalf("ClaimNext: %v", err)
	}
	if claimed == nil {
		t.Fatal("ClaimNext returned nothing; two pending jobs were available")
	}
	if claimed.JobID != older.JobID {
		t.Fatalf("claimed %s, want the oldest job %s — without FIFO an operator's first request starves",
			claimed.JobID, older.JobID)
	}
	if claimed.OrgID != "org-claim-b" {
		t.Fatalf("claimed org = %q, want org-claim-b — the row must carry its own org for the scoped writes that follow",
			claimed.OrgID)
	}
	if claimed.Status != model.StatusRunning || claimed.StartedAt == nil {
		t.Fatalf("claim must return the job running with a start time; got %+v", claimed)
	}

	state := readLeaseState(ctx, t, pool, older.JobID)
	if state.attempts != 1 {
		t.Errorf("attempts = %d, want 1 — attempts count on CLAIM so a worker that dies silently still burns one",
			state.attempts)
	}
	if state.owner == nil || *state.owner != "worker-1" {
		t.Errorf("lease_owner = %v, want worker-1 (a stuck lease must be traceable to a replica)", state.owner)
	}
	if state.until == nil || state.live == nil || !*state.live {
		t.Errorf("lease_until = %v (live=%v), want a deadline in the future", state.until, state.live)
	}

	// The younger job must be untouched — one claim leases exactly one row.
	untouched := readLeaseState(ctx, t, pool, newer.JobID)
	if untouched.attempts != 0 || untouched.owner != nil || untouched.until != nil {
		t.Errorf("unclaimed job carries lease state %+v, want none", untouched)
	}
	stillPending, err := store.Get(ctx, "org-claim-a", newer.JobID)
	if err != nil || stillPending.Status != model.StatusPending {
		t.Errorf("unclaimed job = %+v err=%v, want status pending", stillPending, err)
	}
}

func TestPostgresLiveLeaseIsNotStolenBySecondWorker(t *testing.T) {
	pool := orchestratorIntegrationPool(t)
	store := NewPostgresJobStore(pool)
	ctx := context.Background()

	job := seedPendingJob(ctx, t, store, "org-claim-lease", "claim-live-lease", time.Now().UTC())

	first, err := store.ClaimNext(ctx, "worker-1", time.Minute, MaxAttempts)
	if err != nil || first == nil {
		t.Fatalf("first claim: job=%v err=%v", first, err)
	}

	second, err := store.ClaimNext(ctx, "worker-2", time.Minute, MaxAttempts)
	if err != nil {
		t.Fatalf("second claim: %v", err)
	}
	if second != nil {
		t.Fatalf("worker-2 stole the live lease on %s; two workers would run the same job concurrently", second.JobID)
	}

	state := readLeaseState(ctx, t, pool, job.JobID)
	if state.attempts != 1 {
		t.Errorf("attempts = %d, want 1 — a rejected claim must not burn an attempt", state.attempts)
	}
	if state.owner == nil || *state.owner != "worker-1" {
		t.Errorf("lease_owner = %v, want the original holder worker-1", state.owner)
	}
}

func TestPostgresExpiredLeaseIsReclaimed(t *testing.T) {
	// The crash-recovery property: a worker that died mid-job leaves a `running`
	// row whose lease lapses, and another worker must pick it up. Under the
	// previous fire-and-forget design that row was stranded forever.
	pool := orchestratorIntegrationPool(t)
	store := NewPostgresJobStore(pool)
	ctx := context.Background()

	job := seedPendingJob(ctx, t, store, "org-claim-reclaim", "claim-reclaim-job", time.Now().UTC())

	dead, err := store.ClaimNext(ctx, "worker-dead", time.Minute, MaxAttempts)
	if err != nil || dead == nil {
		t.Fatalf("initial claim: job=%v err=%v", dead, err)
	}

	// A `running` row with no lease at all is the pre-P2-4 orphan shape the
	// migration's data-fix exists to clear. The claim predicate requires an
	// *expired* lease, so such a row must stay unclaimable rather than be
	// treated as free.
	if _, err := pool.Exec(ctx, `
		UPDATE data_orchestrator_jobs SET lease_until = NULL WHERE job_id = $1::uuid
	`, job.JobID); err != nil {
		t.Fatalf("clear lease_until: %v", err)
	}
	if orphan, err := store.ClaimNext(ctx, "worker-live", time.Minute, MaxAttempts); err != nil || orphan != nil {
		t.Fatalf("a running row with a NULL lease was claimed: job=%v err=%v", orphan, err)
	}

	expireLease(ctx, t, pool, job.JobID)

	reclaimed, err := store.ClaimNext(ctx, "worker-live", time.Minute, MaxAttempts)
	if err != nil {
		t.Fatalf("reclaim: %v", err)
	}
	if reclaimed == nil {
		t.Fatal("expired lease was not reclaimed; a crashed worker's job would be stranded")
	}
	if reclaimed.JobID != job.JobID {
		t.Fatalf("reclaimed %s, want %s", reclaimed.JobID, job.JobID)
	}
	if dead.StartedAt == nil || reclaimed.StartedAt == nil || !reclaimed.StartedAt.Equal(*dead.StartedAt) {
		t.Errorf("started_at = %v, want the original %v — COALESCE must report when the work first began, not when the retry did",
			reclaimed.StartedAt, dead.StartedAt)
	}

	state := readLeaseState(ctx, t, pool, job.JobID)
	if state.attempts != 2 {
		t.Errorf("attempts = %d, want 2 after a reclaim — otherwise a job that reliably kills its worker retries forever",
			state.attempts)
	}
	if state.owner == nil || *state.owner != "worker-live" {
		t.Errorf("lease_owner = %v, want worker-live", state.owner)
	}
	if state.live == nil || !*state.live {
		t.Errorf("lease_until = %v (live=%v), want a fresh deadline in the future", state.until, state.live)
	}
}

func TestPostgresExhaustedJobStopsBeingClaimedAndExpiresTerminally(t *testing.T) {
	pool := orchestratorIntegrationPool(t)
	store := NewPostgresJobStore(pool)
	ctx := context.Background()
	now := time.Now().UTC()

	exhausted := seedPendingJob(ctx, t, store, "org-claim-exhausted", "claim-exhausted-job", now.Add(-time.Hour))

	// Burn every attempt: claim, then lapse the lease so the next claim is free
	// to take the same row again.
	for attempt := 1; attempt <= MaxAttempts; attempt++ {
		claimed, err := store.ClaimNext(ctx, fmt.Sprintf("worker-%d", attempt), time.Minute, MaxAttempts)
		if err != nil || claimed == nil {
			t.Fatalf("claim %d: job=%v err=%v", attempt, claimed, err)
		}
		if claimed.JobID != exhausted.JobID {
			t.Fatalf("claim %d took %s, want %s", attempt, claimed.JobID, exhausted.JobID)
		}
		expireLease(ctx, t, pool, exhausted.JobID)
	}
	if state := readLeaseState(ctx, t, pool, exhausted.JobID); state.attempts != MaxAttempts {
		t.Fatalf("attempts = %d after the burn loop, want %d", state.attempts, MaxAttempts)
	}

	// The lease is already expired, so `attempts < $3` is the only thing that
	// can be holding this row back — which is exactly the guard under test.
	if again, err := store.ClaimNext(ctx, "worker-late", time.Minute, MaxAttempts); err != nil || again != nil {
		t.Fatalf("exhausted job was claimable again: job=%v err=%v", again, err)
	}

	// A job with attempts to spare must survive the sweep. Seeded only now so
	// the burn loop above could not have picked it up instead.
	healthy := seedPendingJob(ctx, t, store, "org-claim-healthy", "claim-healthy-job", now)

	n, err := store.ExpireExhausted(ctx, MaxAttempts)
	if err != nil {
		t.Fatalf("ExpireExhausted: %v", err)
	}
	if n != 1 {
		t.Fatalf("expired %d jobs, want 1 — an exhausted job otherwise sits in `running` forever, never reported failed", n)
	}

	// Terminal shape. data_orchestrator_jobs_terminal_shape_check makes
	// status/error_message/completed_at inseparable, so a sweep that set only
	// some of them could not have committed at all — these assertions name what
	// the constraint is protecting.
	final, err := store.Get(ctx, "org-claim-exhausted", exhausted.JobID)
	if err != nil {
		t.Fatalf("Get exhausted job: %v", err)
	}
	if final.Status != model.StatusFailed || final.ErrorMessage == nil || *final.ErrorMessage == "" || final.CompletedAt == nil {
		t.Fatalf("terminal shape violated: %+v (status/error_message/completed_at are required together)", final)
	}
	if state := readLeaseState(ctx, t, pool, exhausted.JobID); state.owner != nil || state.until != nil {
		t.Errorf("terminal job still holds lease state %+v; a retired row must not look claimed", state)
	}

	stillPending, err := store.Get(ctx, "org-claim-healthy", healthy.JobID)
	if err != nil || stillPending.Status != model.StatusPending {
		t.Errorf("healthy job = %+v err=%v, want status pending — the sweep must only retire rows at maxAttempts",
			stillPending, err)
	}
}

// seedPendingJob records one pending job through the real Create path, with a
// caller-chosen created_at so FIFO ordering is deterministic rather than a race
// between inserts.
func seedPendingJob(
	ctx context.Context,
	t *testing.T,
	store *PostgresJobStore,
	orgID, idempotencyKey string,
	createdAt time.Time,
) *model.Job {
	t.Helper()
	job, created, err := store.Create(ctx, model.Job{
		JobID:          uuid.NewString(),
		OrgID:          orgID,
		JobType:        model.JobReindex,
		Status:         model.StatusPending,
		DocumentIDs:    []string{"doc-1"},
		Total:          1,
		IdempotencyKey: idempotencyKey,
		CreatedAt:      createdAt,
		UpdatedAt:      createdAt,
	})
	if err != nil || !created {
		t.Fatalf("seed %s/%s: created=%v err=%v", orgID, idempotencyKey, created, err)
	}
	return job
}

// leaseState holds the storage-only columns jobColumns deliberately omits, so
// the assertions above check what the shipped SQL wrote rather than what
// model.Job happens to expose.
type leaseState struct {
	attempts int
	owner    *string
	until    *time.Time
	// live is `lease_until > NOW()` evaluated by Postgres, so a lease deadline
	// is never judged against the host clock.
	live *bool
}

func readLeaseState(ctx context.Context, t *testing.T, pool *pgxpool.Pool, jobID string) leaseState {
	t.Helper()
	var state leaseState
	if err := pool.QueryRow(ctx, `
		SELECT attempts, lease_owner, lease_until, lease_until > NOW()
		FROM data_orchestrator_jobs
		WHERE job_id = $1::uuid
	`, jobID).Scan(&state.attempts, &state.owner, &state.until, &state.live); err != nil {
		t.Fatalf("read lease state for %s: %v", jobID, err)
	}
	return state
}

// expireLease backdates a lease instead of sleeping through one. The claim
// predicate reads `lease_until <= NOW()`, so moving the deadline into the past
// is equivalent to waiting for it — and keeps these tests deterministic and
// fast rather than trading real seconds for a timing race.
func expireLease(ctx context.Context, t *testing.T, pool *pgxpool.Pool, jobID string) {
	t.Helper()
	tag, err := pool.Exec(ctx, `
		UPDATE data_orchestrator_jobs
		SET lease_until = NOW() - INTERVAL '1 second'
		WHERE job_id = $1::uuid
	`, jobID)
	if err != nil {
		t.Fatalf("expire lease for %s: %v", jobID, err)
	}
	if tag.RowsAffected() != 1 {
		t.Fatalf("expire lease for %s matched %d rows, want 1", jobID, tag.RowsAffected())
	}
}
