package rbac

import (
	"slices"
	"testing"
)

func TestDefaultCapabilitiesByBaseRoleForCompliance(t *testing.T) {
	tests := []struct {
		role    string
		has     []string
		hasNot  []string
		present bool
	}{
		{
			role:    "compliance",
			present: true,
			has:     []string{"org:read", "members:read", "legal_holds:read", "legal_holds:manage", "compliance_audit:read"},
			hasNot:  []string{"org:update", "org:delete", "members:invite", "members:remove", "roles:manage", "billing:read", "billing:manage", "resources:read", "resources:create", "integrations:read", "support:recurrence:read"},
		},
		{
			// An admin may be the subject of a matter: it reads holds but never lifts one.
			role:    "admin",
			present: true,
			has:     []string{"legal_holds:read", "compliance_audit:read", "roles:manage"},
			hasNot:  []string{"legal_holds:manage"},
		},
		{
			role:    "owner",
			present: true,
			has:     []string{"legal_holds:read", "legal_holds:manage", "compliance_audit:read"},
		},
		{
			role:    "viewer",
			present: true,
			hasNot:  []string{"legal_holds:read", "legal_holds:manage", "compliance_audit:read"},
		},
		{
			role:    "member",
			present: true,
			hasNot:  []string{"legal_holds:read", "legal_holds:manage", "compliance_audit:read"},
		},
	}
	keys := CatalogKeys()
	for _, test := range tests {
		t.Run(test.role, func(t *testing.T) {
			got, ok := defaultCapabilitiesByBaseRole[test.role]
			if ok != test.present {
				t.Fatalf("role %q present=%v, want %v", test.role, ok, test.present)
			}
			for _, key := range got {
				if _, known := keys[key]; !known {
					t.Fatalf("role %q default %q is not in the catalog", test.role, key)
				}
			}
			for _, key := range test.has {
				if !slices.Contains(got, key) {
					t.Fatalf("role %q must hold %q; got %v", test.role, key, got)
				}
			}
			for _, key := range test.hasNot {
				if slices.Contains(got, key) {
					t.Fatalf("role %q must not hold %q", test.role, key)
				}
			}
		})
	}
}
