package canvas

import (
	"context"
	"testing"
)

// TestActiveCanvasIDNamesOnlyTheSpacesOwnActiveCanvas reads the id the HTTP layer compares a decision against. It is empty for a
// Space with no canvas, and it is never another org's canvas, even when the other org uses the same Space ref. Gated on
// WIKI_TEST_DATABASE_URL.
func TestActiveCanvasIDNamesOnlyTheSpacesOwnActiveCanvas(t *testing.T) {
	pool := setupCanvasPool(t)
	repo := NewRepo(pool)
	ctx := context.Background()
	mine := createCanvas(ctx, t, repo, "org-a", "space-a", "alpha")
	other := createCanvas(ctx, t, repo, "org-b", "space-a", "bravo")

	got, err := repo.ActiveCanvasID(ctx, "org-a", "space-a")
	if err != nil || got != mine.CanvasID {
		t.Fatalf("ActiveCanvasID(org-a, space-a) = %q, %v; want %q", got, err, mine.CanvasID)
	}
	got, err = repo.ActiveCanvasID(ctx, "org-b", "space-a")
	if err != nil || got != other.CanvasID {
		t.Fatalf("ActiveCanvasID(org-b, space-a) = %q, %v; want %q", got, err, other.CanvasID)
	}
	got, err = repo.ActiveCanvasID(ctx, "org-a", "space-empty")
	if err != nil || got != "" {
		t.Fatalf("ActiveCanvasID of a Space with no canvas = %q, %v; want empty", got, err)
	}
	got, err = repo.ActiveCanvasID(ctx, "org-c", "space-a")
	if err != nil || got != "" {
		t.Fatalf("ActiveCanvasID for an org with no canvas = %q, %v; want empty, never another org's canvas", got, err)
	}
}
