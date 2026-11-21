package notification

import (
	"context"
	"testing"
)

// The user purge refuses an empty user and the shared catalogue before it looks at the pool, so no path can widen into a
// delete of every user's rows.
func TestUserPurgeRefusesTheCatalogueAndAnEmptyUser(t *testing.T) {
	repository := NewRepository(nil)
	for _, userID := range []string{"", "   ", "_default", " _default "} {
		removed, err := repository.PurgeUser(context.Background(), userID)
		if err == nil || removed != nil {
			t.Fatalf("PurgeUser(%q) = (%v, %v), want an error and no counts", userID, removed, err)
		}
	}
}

// Without a configured pool a user purge fails cleanly rather than dereferencing a nil pool.
func TestUserPurgeNeedsAConfiguredRepository(t *testing.T) {
	if removed, err := NewRepository(nil).PurgeUser(context.Background(), "user-1"); err == nil || removed != nil {
		t.Fatalf("PurgeUser() without a pool = (%v, %v), want an error", removed, err)
	}
}
