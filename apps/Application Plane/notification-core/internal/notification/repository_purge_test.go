package notification

import (
	"context"
	"testing"
)

// The purge refuses an empty organization and the shared catalogue before it looks at the pool, so no path can widen
// into a delete of every row.
func TestOrganizationPurgeRefusesTheCatalogueAndAnEmptyOrganization(t *testing.T) {
	repository := NewRepository(nil)
	for _, organizationID := range []string{"", "   ", "_default", " _default "} {
		removed, err := repository.PurgeOrganization(context.Background(), organizationID)
		if err == nil || removed != nil {
			t.Fatalf("PurgeOrganization(%q) = (%v, %v), want an error and no counts", organizationID, removed, err)
		}
	}
}

// Without a configured pool a purge fails cleanly rather than dereferencing a nil pool.
func TestOrganizationPurgeNeedsAConfiguredRepository(t *testing.T) {
	if removed, err := NewRepository(nil).PurgeOrganization(context.Background(), "org-1"); err == nil || removed != nil {
		t.Fatalf("PurgeOrganization() without a pool = (%v, %v), want an error", removed, err)
	}
}
