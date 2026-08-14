package spaces

import "testing"

func grant(subjectType, subjectID, role string) MemberGrant {
	return MemberGrant{SubjectType: subjectType, SubjectID: subjectID, Role: role}
}

func TestMembershipReplacementValidate(t *testing.T) {
	tests := []struct {
		name        string
		replacement MembershipReplacement
		wantErr     bool
	}{
		{
			name:        "a roster of users and service identities is accepted",
			replacement: MembershipReplacement{SpaceRef: "space-1", Members: []MemberGrant{grant("user", "u1", "editor"), grant("service", "agent-1", "viewer")}},
		},
		{
			// The product model allows agent/service identities as members, so
			// rejecting them here would make the room human-only by accident.
			name:        "a service subject is a first-class member",
			replacement: MembershipReplacement{SpaceRef: "space-1", Members: []MemberGrant{grant("service", "agent-1", "manager")}},
		},
		{
			// Absence means revocation, so an empty set is a legitimate
			// instruction ("nobody but the owner"), not a malformed one.
			name:        "an empty roster is a valid instruction",
			replacement: MembershipReplacement{SpaceRef: "space-1"},
		},
		{
			name:        "a Space reference is required",
			replacement: MembershipReplacement{Members: []MemberGrant{grant("user", "u1", "editor")}},
			wantErr:     true,
		},
		{
			name:        "an unknown subject type is rejected",
			replacement: MembershipReplacement{SpaceRef: "space-1", Members: []MemberGrant{grant("robot", "u1", "editor")}},
			wantErr:     true,
		},
		{
			name:        "an unknown role is rejected",
			replacement: MembershipReplacement{SpaceRef: "space-1", Members: []MemberGrant{grant("user", "u1", "admin")}},
			wantErr:     true,
		},
		{
			name:        "an empty subject id is rejected",
			replacement: MembershipReplacement{SpaceRef: "space-1", Members: []MemberGrant{grant("user", "   ", "editor")}},
			wantErr:     true,
		},
		{
			// Two rows for one subject would make the stored role depend on
			// iteration order. The caller has to decide, not us.
			name:        "a duplicate subject is rejected rather than last-write-wins",
			replacement: MembershipReplacement{SpaceRef: "space-1", Members: []MemberGrant{grant("user", "u1", "viewer"), grant("user", "u1", "owner")}},
			wantErr:     true,
		},
		{
			// Same id under different subject types is two different subjects.
			name:        "the same id as user and service are distinct members",
			replacement: MembershipReplacement{SpaceRef: "space-1", Members: []MemberGrant{grant("user", "shared", "viewer"), grant("service", "shared", "viewer")}},
		},
		{
			name:        "an oversized roster is rejected",
			replacement: MembershipReplacement{SpaceRef: "space-1", Members: oversizedRoster()},
			wantErr:     true,
		},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			err := tc.replacement.Validate()
			if tc.wantErr && err == nil {
				t.Fatalf("expected a validation error, got none")
			}
			if !tc.wantErr && err != nil {
				t.Fatalf("expected the roster to validate, got %v", err)
			}
		})
	}
}

func oversizedRoster() []MemberGrant {
	members := make([]MemberGrant, 0, maxSpaceMembers+1)
	for index := 0; index <= maxSpaceMembers; index++ {
		members = append(members, MemberGrant{
			SubjectType: "user",
			SubjectID:   string(rune('a'+index%26)) + itoa(index),
			Role:        "viewer",
		})
	}
	return members
}

func itoa(value int) string {
	if value == 0 {
		return "0"
	}
	digits := ""
	for value > 0 {
		digits = string(rune('0'+value%10)) + digits
		value /= 10
	}
	return digits
}
