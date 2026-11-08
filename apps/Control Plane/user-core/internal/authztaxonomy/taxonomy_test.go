package authztaxonomy

import "testing"

// TestEveryTypeInExactlyOneSet is the CI lint required by the ownership plan:
// every visibility-bearing resource type must be classified in exactly one set,
// and the two sets must never overlap or be empty.
func TestEveryTypeInExactlyOneSet(t *testing.T) {
	own := Ownable()
	team := TeamShared()

	if len(own) == 0 {
		t.Fatal("ownable set is empty; at least 'document' is expected")
	}
	if len(team) == 0 {
		t.Fatal("team_shared set is empty")
	}

	seen := map[string]int{}
	for _, ty := range own {
		seen[ty]++
	}
	for _, ty := range team {
		seen[ty]++
	}
	for ty, n := range seen {
		if n != 1 {
			t.Errorf("resource type %q appears in %d sets; every type must be in exactly one", ty, n)
		}
	}
}

func TestClassify(t *testing.T) {
	cases := []struct {
		resourceType string
		want         Category
		wantErr      bool
	}{
		{"document", CategoryOwnable, false},
		{"inbox", CategoryTeamShared, false},
		{"conversation", CategoryTeamShared, false},
		{"ticket", CategoryTeamShared, false},
		{"billing", CategoryTeamShared, false},
		{"audit_log", CategoryTeamShared, false},
		{"org_settings", CategoryTeamShared, false},
		{"quarry_source", CategoryTeamShared, false},
		{"quarry_run", CategoryTeamShared, false},
		{"capability_registry", CategoryTeamShared, false},
		{"nonexistent_type", "", true},
		{"", "", true},
	}
	for _, tc := range cases {
		got, err := Classify(tc.resourceType)
		if tc.wantErr {
			if err == nil {
				t.Errorf("Classify(%q): expected error, got category %q", tc.resourceType, got)
			}
			continue
		}
		if err != nil {
			t.Errorf("Classify(%q): unexpected error: %v", tc.resourceType, err)
			continue
		}
		if got != tc.want {
			t.Errorf("Classify(%q) = %q, want %q", tc.resourceType, got, tc.want)
		}
	}
}

func TestValidateUserGrant(t *testing.T) {
	// Ownable types may be granted to a single user / made private.
	if err := ValidateUserGrant("document"); err != nil {
		t.Errorf("ValidateUserGrant(document): unexpected error: %v", err)
	}
	// Team-shared types must be rejected — no private inbox, no per-user billing grant.
	for _, ty := range []string{"inbox", "billing", "audit_log", "conversation", "ticket"} {
		if err := ValidateUserGrant(ty); err == nil {
			t.Errorf("ValidateUserGrant(%q): expected rejection of per-user grant on team-shared type", ty)
		}
	}
	// Unknown types are rejected (fail closed).
	if err := ValidateUserGrant("totally_unknown"); err == nil {
		t.Error("ValidateUserGrant(unknown): expected rejection of unclassified type")
	}
}

func TestIsOwnableIsTeamShared(t *testing.T) {
	if !IsOwnable("document") {
		t.Error("document should be ownable")
	}
	if IsTeamShared("document") {
		t.Error("document should not be team-shared")
	}
	if !IsTeamShared("inbox") {
		t.Error("inbox should be team-shared")
	}
	if IsOwnable("inbox") {
		t.Error("inbox should not be ownable")
	}
}

// TestConversationSnapshotIsOwnableAndNotTeamShared pins the share model (EP-12
// D10): a share snapshot is its own ownable type so per-user grants can name it,
// while the live conversation stays team-shared. The type has no owner service
// and no Space grant yet, so no grant can be written for it until the snapshot
// store registers one.
func TestConversationSnapshotIsOwnableAndNotTeamShared(t *testing.T) {
	got, err := Classify("conversation_snapshot")
	if err != nil {
		t.Fatalf("Classify(conversation_snapshot): unexpected error: %v", err)
	}
	if got != CategoryOwnable {
		t.Errorf("Classify(conversation_snapshot) = %q, want %q", got, CategoryOwnable)
	}
	if IsTeamShared("conversation_snapshot") {
		t.Error("conversation_snapshot should not be team-shared")
	}
	if err := ValidateUserGrant("conversation_snapshot"); err != nil {
		t.Errorf("ValidateUserGrant(conversation_snapshot): unexpected error: %v", err)
	}
	if IsOwnable("conversation") {
		t.Error("conversation should stay team-shared, not ownable")
	}
	if owner, ok := OwnerService("conversation_snapshot"); ok {
		t.Errorf("conversation_snapshot has owner service %q; none is expected until the snapshot store ships", owner)
	}
	if SpaceGrantable("conversation_snapshot") {
		t.Error("conversation_snapshot should not be Space-grantable")
	}
}
