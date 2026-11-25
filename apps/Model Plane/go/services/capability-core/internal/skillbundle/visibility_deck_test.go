package skillbundle

import (
	"context"
	"strings"
	"testing"
)

// deckColumnText is the DeckTemplate column of a bundle listing with its whitespace collapsed: the theme file of the
// bundle AND the deck stamp (F-f09-08). A listing that reports a deck template without the stamp reports an unvalidated
// bundle as one.
const deckColumnText = "(EXISTS (SELECT 1 FROM skill_bundle_files f WHERE f.bundle_id = b.id AND f.path = $3) AND b.deck_validated_at IS NOT NULL)"

// P2-go-13-rel1: the unscoped list (List) and the scoped list (ListVisible, SKILL_BUNDLE_SCOPE_RULES on) both list a bundle
// as a deck template only after it has passed ValidateDeckTemplate. The scoped predicate was first written into
// visibility.go, a file another track owns, so a rewrite that drops it fails this test instead of listing unstamped
// bundles as templates.
func TestDeckTemplateColumnNeedsTheStampInEveryListing(t *testing.T) {
	ctx := context.Background()

	db := &sweepDB{}
	if _, err := NewStore(db).List(ctx, "org-a", nil); err != nil {
		t.Fatalf("List: %v", err)
	}
	requireDeckStampedColumn(t, "List", db.lastSQL)

	scoped := &sweepDB{}
	if _, err := NewStore(scoped).ListVisible(ctx, "org-a", "user-a", false, nil); err != nil {
		t.Fatalf("ListVisible: %v", err)
	}
	requireDeckStampedColumn(t, "ListVisible", scoped.lastSQL)
}

// requireDeckStampedColumn fails when the select list of a bundle listing does not gate its deck template column on the
// deck stamp.
func requireDeckStampedColumn(t *testing.T, name, sql string) {
	t.Helper()
	selectList, _, found := strings.Cut(sql, "FROM skill_bundles b")
	if !found {
		t.Fatalf("%s has no FROM skill_bundles b:\n%s", name, sql)
	}
	if !strings.Contains(strings.Join(strings.Fields(selectList), " "), deckColumnText) {
		t.Fatalf("%s does not gate the deck template column on the deck stamp:\n%s", name, sql)
	}
}
