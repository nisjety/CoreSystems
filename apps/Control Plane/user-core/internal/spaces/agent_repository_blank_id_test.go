package spaces

import (
	"context"
	"testing"
)

// E-PRIO-01: a blank agent id names no agent. Without the early return the
// visibility query's `$3 = ''` arm matched every visible agent, so the caller got
// the first one (and, for the access read, its access list) instead of a 404.
// The repository here has no database: the blank id must answer before any query.
func TestVisibleAgentReadsTreatABlankIDAsNotFound(t *testing.T) {
	repo := &Repository{}
	for _, id := range []string{"", "   ", "\t"} {
		view, err := repo.VisibleAgent(context.Background(), "org-1", "user-1", id)
		if err != nil || view != nil {
			t.Fatalf("VisibleAgent(%q) = (%v, %v), want (nil, nil)", id, view, err)
		}
		view, entries, err := repo.VisibleAgentAccess(context.Background(), "org-1", "user-1", id)
		if err != nil || view != nil || entries != nil {
			t.Fatalf("VisibleAgentAccess(%q) = (%v, %v, %v), want (nil, nil, nil)", id, view, entries, err)
		}
	}
}

// A real id still reaches the registry, which answers unavailable without a database.
func TestVisibleAgentAccessWithoutADatabaseIsUnavailable(t *testing.T) {
	_, _, err := (&Repository{}).VisibleAgentAccess(context.Background(), "org-1", "user-1", "agent-1")
	if err == nil {
		t.Fatal("VisibleAgentAccess without a database returned no error")
	}
}
