package store

import (
	"testing"

	"github.com/triodelab/finspo/internal/sharepoint"
)

func TestCanonicalPermHashIsStableAcrossRoleOrdering(t *testing.T) {
	t.Parallel()

	uid := sharepoint.PermissionIdentitySet{User: &sharepoint.PermissionIdentity{ID: "u1", DisplayName: "Alice"}}

	a := normalizePermissions([]sharepoint.PermissionEntry{{
		ID:          "p1",
		Roles:       []string{"read", "write"},
		GrantedToV2: &uid,
	}})
	b := normalizePermissions([]sharepoint.PermissionEntry{{
		ID:          "p2",
		Roles:       []string{"write", "read"}, // reversed
		GrantedToV2: &uid,
	}})

	if len(a) != 1 || len(b) != 1 {
		t.Fatalf("normalize length a=%d b=%d", len(a), len(b))
	}
	if canonicalPermHash(a[0]) != canonicalPermHash(b[0]) {
		t.Errorf("hashes differ across role ordering: %q vs %q", canonicalPermHash(a[0]), canonicalPermHash(b[0]))
	}
}

func TestCanonicalPermHashChangesWhenPrincipalChanges(t *testing.T) {
	t.Parallel()

	roles := []string{"read"}
	first := normalizePermissions([]sharepoint.PermissionEntry{{
		Roles:       roles,
		GrantedToV2: &sharepoint.PermissionIdentitySet{User: &sharepoint.PermissionIdentity{ID: "u1"}},
	}})
	second := normalizePermissions([]sharepoint.PermissionEntry{{
		Roles:       roles,
		GrantedToV2: &sharepoint.PermissionIdentitySet{User: &sharepoint.PermissionIdentity{ID: "u2"}},
	}})

	if canonicalPermHash(first[0]) == canonicalPermHash(second[0]) {
		t.Error("expected different hashes for different principals")
	}
}

func TestNormalizePermissionsExpandsGrantedToIdentitiesV2(t *testing.T) {
	t.Parallel()

	entry := sharepoint.PermissionEntry{
		Roles: []string{"read"},
		GrantedToIdentitiesV2: []sharepoint.PermissionIdentitySet{
			{User: &sharepoint.PermissionIdentity{ID: "u1"}},
			{Group: &sharepoint.PermissionIdentity{ID: "g1"}},
		},
	}
	out := normalizePermissions([]sharepoint.PermissionEntry{entry})
	if len(out) != 2 {
		t.Fatalf("len = %d, want 2", len(out))
	}
	if out[0].principalType != "user" || out[1].principalType != "group" {
		t.Errorf("types = %q,%q", out[0].principalType, out[1].principalType)
	}
}

func TestNormalizePermissionsKeepsLinkOnlyEntry(t *testing.T) {
	t.Parallel()

	out := normalizePermissions([]sharepoint.PermissionEntry{{
		Roles: []string{"read"},
		Link:  &sharepoint.PermissionLink{Scope: "anonymous", Type: "view"},
	}})
	if len(out) != 1 {
		t.Fatalf("len = %d, want 1", len(out))
	}
	if out[0].principalID != "" || out[0].linkScope != "anonymous" {
		t.Errorf("row = %#v", out[0])
	}
}
