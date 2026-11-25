//go:build integration

package skillbundle

import (
	"context"
	"testing"
)

// P2-go-13-rel1 against a real Postgres: the scoped listing (SKILL_BUNDLE_SCOPE_RULES on) reports a deck template only once
// the bundle is stamped, as the unscoped list does. Needs the DB harness (testcontainers).
func TestScopedListingReportsADeckTemplateOnlyOnceStamped(t *testing.T) {
	ctx := context.Background()
	pool := setupBundleDB(t)
	applyDeckMigrations(t, pool)
	if _, err := pool.Exec(ctx, visibilityStubSchema); err != nil {
		t.Fatalf("create agent_skills: %v", err)
	}
	if _, err := pool.Exec(ctx, `
		INSERT INTO agent_skills (id, org_id, scope, owner_user_id, review_state, shared_with)
		VALUES ('skill_clean', 'org-a', 'org', NULL, 'accepted', '[]'::jsonb)
	`); err != nil {
		t.Fatalf("insert skill: %v", err)
	}
	store := NewStore(pool)
	clean := templateBundle(t, themeFixture(t))
	if _, err := store.Put(ctx, "org-a", "skill_clean", "user-admin", clean); err != nil {
		t.Fatalf("Put: %v", err)
	}
	if scopedDeckFlag(t, store, "user-a", "skill_clean") {
		t.Fatal("an unstamped bundle with a theme is listed as a deck template by the scoped listing")
	}
	if err := store.MarkDeckValidated(ctx, "org-a", "skill_clean", clean.ManifestSHA256); err != nil {
		t.Fatalf("MarkDeckValidated: %v", err)
	}
	if !scopedDeckFlag(t, store, "user-a", "skill_clean") {
		t.Fatal("a stamped bundle with a theme is not listed as a deck template by the scoped listing")
	}
}

// scopedDeckFlag reports the deck_template flag the scoped listing gives the skill's bundle to the caller, failing when the
// caller cannot see the skill.
func scopedDeckFlag(t *testing.T, store *Store, caller, skillID string) bool {
	t.Helper()
	summaries, err := store.ListVisible(context.Background(), "org-a", caller, false, nil)
	if err != nil {
		t.Fatalf("ListVisible(%s): %v", caller, err)
	}
	for _, summary := range summaries {
		if summary.SkillID == skillID {
			return summary.DeckTemplate
		}
	}
	t.Fatalf("skill %s is not listed for %s", skillID, caller)
	return false
}
