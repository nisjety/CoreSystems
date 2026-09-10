package lease

import (
	"errors"
	"testing"
	"time"
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
