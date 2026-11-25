//go:build integration

package skillbundle

import (
	"context"
	"errors"
	"slices"
	"testing"
)

// visibilityStubSchema is the agent_skills columns the visibility rule reads. The table itself comes from session-core's
// migrations, which this package's fixture does not apply, so the fixture creates the same columns.
const visibilityStubSchema = `
CREATE TABLE IF NOT EXISTS agent_skills (
	id text NOT NULL,
	org_id text NOT NULL,
	scope text NOT NULL,
	owner_user_id text,
	review_state text NOT NULL,
	shared_with jsonb NOT NULL DEFAULT '[]'::jsonb,
	PRIMARY KEY (org_id, id)
)`

// TestVisibleBundlesFollowTheSkillVisibilityRule seeds one live bundle per skill the rule separates, then reads the
// listing and the manifest as several callers. A bundle is visible exactly when its skill is: an org skill; a person's
// own skill, proposal or not; an accepted skill shared with the caller; and, for an admin, an accepted skill some person
// shares. An unshared private skill and a proposal that is shared with someone are invisible to others.
func TestVisibleBundlesFollowTheSkillVisibilityRule(t *testing.T) {
	pool := setupBundleDB(t)
	ctx := context.Background()
	if _, err := pool.Exec(ctx, visibilityStubSchema); err != nil {
		t.Fatalf("create agent_skills: %v", err)
	}
	skills := []struct {
		id, scope, owner, review, shared string
	}{
		{"org-skill", "org", "", "accepted", "[]"},
		{"own-proposal", "user", "user-a", "proposed", "[]"},
		{"shared-accepted", "user", "user-b", "accepted", `["user-a"]`},
		{"private-accepted", "user", "user-b", "accepted", "[]"},
		{"shared-proposal", "user", "user-b", "proposed", `["user-a"]`},
	}
	store := NewStore(pool)
	for _, skill := range skills {
		if _, err := pool.Exec(ctx, `
			INSERT INTO agent_skills (id, org_id, scope, owner_user_id, review_state, shared_with)
			VALUES ($1, 'org-a', $2, NULLIF($3, ''), $4, $5::jsonb)
		`, skill.id, skill.scope, skill.owner, skill.review, skill.shared); err != nil {
			t.Fatalf("insert skill %s: %v", skill.id, err)
		}
		if _, err := store.Put(ctx, "org-a", skill.id, "admin-1", bundleOf(t, "body of "+skill.id)); err != nil {
			t.Fatalf("store the bundle of %s: %v", skill.id, err)
		}
	}

	cases := []struct {
		caller string
		admin  bool
		want   []string
	}{
		{"user-a", false, []string{"org-skill", "own-proposal", "shared-accepted"}},
		{"user-b", false, []string{"org-skill", "private-accepted", "shared-accepted", "shared-proposal"}},
		{"user-c", false, []string{"org-skill"}},
		{"user-c", true, []string{"org-skill", "shared-accepted"}},
		{"user-a", true, []string{"org-skill", "own-proposal", "shared-accepted"}},
	}
	for _, tc := range cases {
		summaries, err := store.ListVisible(ctx, "org-a", tc.caller, tc.admin, nil)
		if err != nil {
			t.Fatalf("ListVisible(%s, admin=%v): %v", tc.caller, tc.admin, err)
		}
		got := make([]string, 0, len(summaries))
		for _, summary := range summaries {
			got = append(got, summary.SkillID)
		}
		slices.Sort(got)
		if !slices.Equal(got, tc.want) {
			t.Fatalf("ListVisible(%s, admin=%v) = %v, want %v", tc.caller, tc.admin, got, tc.want)
		}
	}

	visible := []struct {
		skill, caller string
		admin         bool
	}{
		{"shared-accepted", "user-a", false},
		{"org-skill", "user-c", false},
		{"shared-accepted", "user-c", true},
	}
	for _, tc := range visible {
		manifest, err := store.ManifestVisible(ctx, "org-a", tc.caller, tc.admin, tc.skill, "")
		if err != nil || len(manifest.Files) == 0 {
			t.Fatalf("ManifestVisible(%s, %s): %+v, %v; want the visible bundle", tc.skill, tc.caller, manifest, err)
		}
	}

	// An invisible skill answers the same ErrNotFound as a skill with no bundle, and a caller with no id reads nothing.
	hidden := []struct {
		skill, caller string
		admin         bool
	}{
		{"private-accepted", "user-a", false},
		{"private-accepted", "user-a", true},
		{"shared-proposal", "user-c", false},
		{"shared-accepted", "", true},
		{"no-such-skill", "user-a", true},
	}
	for _, tc := range hidden {
		if _, err := store.ManifestVisible(ctx, "org-a", tc.caller, tc.admin, tc.skill, ""); !errors.Is(err, ErrNotFound) {
			t.Fatalf("ManifestVisible(%s, %s, admin=%v) = %v, want ErrNotFound", tc.skill, tc.caller, tc.admin, err)
		}
	}
}
