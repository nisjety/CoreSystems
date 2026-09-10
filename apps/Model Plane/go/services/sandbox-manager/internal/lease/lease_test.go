package lease

import (
	"errors"
	"sync"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
)

func TestStoreScopesCopiesAndReleasesLeases(t *testing.T) {
	store := NewStore()
	created, err := store.Create("scope-a", "agent", "org-a", "user-a", "", "", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	created.OrgID = "attacker"

	for _, tc := range []struct{ name, org, owner string }{
		{name: "wrong org", org: "org-b", owner: "user-a"},
		{name: "wrong owner", org: "org-a", owner: "user-b"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := store.GetScoped(created.ID, tc.org, tc.owner, ""); !errors.Is(err, ErrLeaseNotFound) {
				t.Fatalf("error = %v", err)
			}
			if _, err := store.ReleaseScoped(created.ID, tc.org, tc.owner, ""); !errors.Is(err, ErrLeaseNotFound) {
				t.Fatalf("release error = %v", err)
			}
		})
	}

	visible, err := store.GetScoped(created.ID, "org-a", "", "")
	if err != nil {
		t.Fatal(err)
	}
	if visible.OrgID != "org-a" || visible.OwnerID != "user-a" {
		t.Fatalf("stored lease mutated: %#v", visible)
	}
	visible.OwnerID = "attacker"
	visibleAgain, err := store.GetScoped(created.ID, "org-a", "user-a", "")
	if err != nil || visibleAgain.OwnerID != "user-a" {
		t.Fatalf("returned lease was not copied: %#v, %v", visibleAgain, err)
	}

	if ok, err := store.ReleaseScoped(created.ID, "org-a", "user-a", ""); err != nil || !ok {
		t.Fatalf("release = %v, %v", ok, err)
	}
	if _, err := store.GetScoped(created.ID, "org-a", "user-a", ""); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("error = %v", err)
	}
}

func TestStoreReportsExpiredAndIDGenerationErrors(t *testing.T) {
	store := NewStore()
	store.nowFn = func() time.Time { return time.Unix(100, 0) }
	created, err := store.Create("scope-a", "agent", "org-a", "user-a", "", "", time.Second)
	if err != nil {
		t.Fatal(err)
	}
	store.nowFn = func() time.Time { return time.Unix(102, 0) }
	if _, err := store.GetScoped(created.ID, "org-a", "user-a", ""); !errors.Is(err, ErrLeaseExpired) {
		t.Fatalf("error = %v", err)
	}

	store.randFn = func([]byte) (int, error) { return 0, errors.New("entropy unavailable") }
	if _, err := store.Create("scope", "agent", "org-a", "user-a", "", "", time.Minute); err == nil {
		t.Fatal("expected entropy error")
	}
}

// TestGetScopedChecksExpiryBeforeBackendMismatch pins the ordering the S3.2
// design's test plan requires: an expired lease must report ErrLeaseExpired
// even when the caller's asserted backend id is also wrong, so an operator
// debugging an expiry never sees a misleading backend-mismatch error instead.
func TestGetScopedChecksExpiryBeforeBackendMismatch(t *testing.T) {
	store := NewStore()
	store.nowFn = func() time.Time { return time.Unix(100, 0) }
	created, err := store.Create("scope-a", "agent", "org-a", "user-a", "space-a", "backend-a", time.Second)
	if err != nil {
		t.Fatal(err)
	}
	store.nowFn = func() time.Time { return time.Unix(102, 0) }
	if _, err := store.GetScoped(created.ID, "org-a", "user-a", "backend-b"); !errors.Is(err, ErrLeaseExpired) {
		t.Fatalf("error = %v, want ErrLeaseExpired", err)
	}
}

func TestGetScopedRejectsBackendMismatch(t *testing.T) {
	store := NewStore()
	created, err := store.Create("scope-a", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetScoped(created.ID, "org-a", "user-a", "backend-b"); !errors.Is(err, ErrLeaseBackendMismatch) {
		t.Fatalf("error = %v, want ErrLeaseBackendMismatch", err)
	}
	if _, err := store.ReleaseScoped(created.ID, "org-a", "user-a", "backend-b"); !errors.Is(err, ErrLeaseBackendMismatch) {
		t.Fatalf("release error = %v, want ErrLeaseBackendMismatch", err)
	}
	if _, err := store.GetScoped(created.ID, "org-a", "user-a", "backend-a"); err != nil {
		t.Fatalf("matching backend id should succeed: %v", err)
	}
}

func TestCreateStartsASpaceScopedLeaseInScratch(t *testing.T) {
	store := NewStore()
	created, err := store.Create("scope-a", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if created.State != mpv1.SandboxLifecycleState_SCRATCH {
		t.Fatalf("State = %v, want SCRATCH", created.State)
	}
}

func TestActivateTransitionsScratchToActiveAndIsIdempotent(t *testing.T) {
	store := NewStore()
	created, err := store.Create("scope-a", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	activated, err := store.Activate(created.ID, "org-a", "user-a", "backend-a")
	if err != nil {
		t.Fatal(err)
	}
	if activated.State != mpv1.SandboxLifecycleState_ACTIVE {
		t.Fatalf("State = %v, want ACTIVE", activated.State)
	}
	// Activating an already-ACTIVE lease is a no-op, not an error.
	activatedAgain, err := store.Activate(created.ID, "org-a", "user-a", "backend-a")
	if err != nil {
		t.Fatal(err)
	}
	if activatedAgain.State != mpv1.SandboxLifecycleState_ACTIVE {
		t.Fatalf("State = %v, want ACTIVE", activatedAgain.State)
	}
}

func TestActivateEnforcesTheSameScopeAndBackendPinAsGetScoped(t *testing.T) {
	store := NewStore()
	created, err := store.Create("scope-a", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Activate(created.ID, "org-b", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("wrong org: error = %v, want ErrLeaseNotFound", err)
	}
	if _, err := store.Activate(created.ID, "org-a", "user-a", "backend-b"); !errors.Is(err, ErrLeaseBackendMismatch) {
		t.Fatalf("wrong backend: error = %v, want ErrLeaseBackendMismatch", err)
	}
}

func TestBeginSnapshotRejectsAScratchSpaceScopedLeaseButNotANonSpaceLease(t *testing.T) {
	store := NewStore()

	spaceScoped, err := store.Create("scope-a", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.BeginSnapshot(spaceScoped.ID, "org-a", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotActivated) {
		t.Fatalf("error = %v, want ErrLeaseNotActivated", err)
	}
	if _, err := store.Activate(spaceScoped.ID, "org-a", "user-a", "backend-a"); err != nil {
		t.Fatal(err)
	}
	snapshotting, err := store.BeginSnapshot(spaceScoped.ID, "org-a", "user-a", "backend-a")
	if err != nil {
		t.Fatalf("snapshot of an ACTIVE Space-scoped lease should succeed: %v", err)
	}
	if snapshotting.State != mpv1.SandboxLifecycleState_SNAPSHOTTING {
		t.Fatalf("State = %v, want SNAPSHOTTING", snapshotting.State)
	}
	store.EndSnapshot(spaceScoped.ID)
	returned, err := store.GetScoped(spaceScoped.ID, "org-a", "user-a", "backend-a")
	if err != nil {
		t.Fatal(err)
	}
	if returned.State != mpv1.SandboxLifecycleState_ACTIVE {
		t.Fatalf("State after EndSnapshot = %v, want ACTIVE", returned.State)
	}

	// A non-Space lease predates this state machine entirely: it starts in
	// SCRATCH like every lease, but SnapshotSandbox must keep working for it
	// exactly as it always has, unconditionally.
	nonSpace, err := store.Create("scope-b", "agent", "org-a", "user-a", "", "", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.BeginSnapshot(nonSpace.ID, "org-a", "user-a", ""); err != nil {
		t.Fatalf("non-Space lease snapshot should never require activation: %v", err)
	}
}

func TestEndSnapshotReturnsToActiveEvenWhenTheSnapshotFailed(t *testing.T) {
	store := NewStore()
	created, err := store.Create("scope-a", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.Activate(created.ID, "org-a", "user-a", "backend-a"); err != nil {
		t.Fatal(err)
	}
	if _, err := store.BeginSnapshot(created.ID, "org-a", "user-a", "backend-a"); err != nil {
		t.Fatal(err)
	}
	// Simulate the caller's snapshot-creation step failing after
	// BeginSnapshot succeeded: EndSnapshot must still run (the caller's own
	// defer) and leave the lease usable, never stuck in SNAPSHOTTING.
	store.EndSnapshot(created.ID)
	returned, err := store.GetScoped(created.ID, "org-a", "user-a", "backend-a")
	if err != nil {
		t.Fatal(err)
	}
	if returned.State != mpv1.SandboxLifecycleState_ACTIVE {
		t.Fatalf("State = %v, want ACTIVE", returned.State)
	}
}

func TestReleaseIsIdempotentAndDestroyedLeaseRejectsFurtherSnapshotAndActivate(t *testing.T) {
	store := NewStore()
	created, err := store.Create("scope-a", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	if ok, err := store.ReleaseScoped(created.ID, "org-a", "user-a", "backend-a"); err != nil || !ok {
		t.Fatalf("first release: ok=%v err=%v", ok, err)
	}
	// A second release of the same lease is a clean idempotent success, not
	// ErrLeaseNotFound.
	if ok, err := store.ReleaseScoped(created.ID, "org-a", "user-a", "backend-a"); err != nil || !ok {
		t.Fatalf("second (idempotent) release: ok=%v err=%v", ok, err)
	}
	if _, err := store.GetScoped(created.ID, "org-a", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("GetScoped after destroy: error = %v, want ErrLeaseNotFound", err)
	}
	if _, err := store.BeginSnapshot(created.ID, "org-a", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("BeginSnapshot after destroy: error = %v, want ErrLeaseNotFound", err)
	}
	if _, err := store.Activate(created.ID, "org-a", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("Activate after destroy: error = %v, want ErrLeaseNotFound", err)
	}
}

// TestFreshStoreRejectsPreRestartLeaseID is the S3.2 close-out design's
// named "restart" scenario: this store is in-memory only (no durable
// backend — see cmd/main.go's ephemeral-development gate), so a restart
// discards it entirely. A lease id from the discarded store must fail
// closed as not-found in a fresh one, never be treated as expired,
// mismatched, or any other state that would imply the id was ever known.
func TestFreshStoreRejectsPreRestartLeaseID(t *testing.T) {
	before := NewStore()
	created, err := before.Create("scope-a", "agent", "org-a", "user-a", "space-a", "backend-a", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	after := NewStore()
	if _, err := after.GetScoped(created.ID, "org-a", "user-a", "backend-a"); !errors.Is(err, ErrLeaseNotFound) {
		t.Fatalf("error = %v, want ErrLeaseNotFound", err)
	}
}

// TestStoreConcurrentCreateIsRaceFree is the S3.2 close-out design's named
// "concurrent provision" scenario. Ideally gated under `go test -race` in
// CI; this Windows dev host has CGO_ENABLED=0, where -race cannot run at
// all (see memory: model-plane-build-and-deploy), so this only proves
// correctness (unique ids, no lost ExpiresAt) under plain concurrent
// execution here — it does not by itself prove the absence of a data race.
func TestStoreConcurrentCreateIsRaceFree(t *testing.T) {
	store := NewStore()
	const concurrency = 50
	var wg sync.WaitGroup
	leases := make([]*Lease, concurrency)
	errs := make([]error, concurrency)
	for i := 0; i < concurrency; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			leases[i], errs[i] = store.Create("scope-a", "agent", "org-a", "user-a", "", "", time.Minute)
		}(i)
	}
	wg.Wait()

	seen := make(map[string]bool, concurrency)
	for i, err := range errs {
		if err != nil {
			t.Fatalf("Create[%d]: unexpected error: %v", i, err)
		}
		if leases[i].ID == "" {
			t.Fatalf("Create[%d]: empty lease id", i)
		}
		if seen[leases[i].ID] {
			t.Fatalf("Create[%d]: duplicate lease id %q", i, leases[i].ID)
		}
		seen[leases[i].ID] = true
		if leases[i].ExpiresAt.IsZero() {
			t.Fatalf("Create[%d]: lost ExpiresAt", i)
		}
	}
}
