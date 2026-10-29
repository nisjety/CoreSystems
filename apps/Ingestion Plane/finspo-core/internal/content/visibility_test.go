package content

import (
	"testing"

	"github.com/triodelab/finspo/internal/store"
)

func TestClassifyVisibility(t *testing.T) {
	cases := []struct {
		name        string
		permissions []store.Permission
		want        string
	}{
		{
			name:        "no captured ACL fails closed to private",
			permissions: nil,
			want:        VisibilityPrivate,
		},
		{
			name:        "empty ACL fails closed to private",
			permissions: []store.Permission{},
			want:        VisibilityPrivate,
		},
		{
			name: "tenant-wide sharing link is org",
			permissions: []store.Permission{
				{LinkScope: "organization", LinkType: "view"},
			},
			want: VisibilityOrg,
		},
		{
			name: "anonymous sharing link is org",
			permissions: []store.Permission{
				{LinkScope: "anonymous", LinkType: "view"},
			},
			want: VisibilityOrg,
		},
		{
			name: "link scope casing and padding are tolerated",
			permissions: []store.Permission{
				{LinkScope: "  Organization "},
			},
			want: VisibilityOrg,
		},
		{
			name: "group grant is org",
			permissions: []store.Permission{
				{PrincipalType: "group", PrincipalName: "Aquatiq Finance", Roles: []string{"read"}},
			},
			want: VisibilityOrg,
		},
		{
			name: "SharePoint site group grant is org",
			permissions: []store.Permission{
				{PrincipalType: "siteGroup", PrincipalName: "Intranett Members", Roles: []string{"read"}},
			},
			want: VisibilityOrg,
		},
		{
			name: "individually-granted item stays private",
			permissions: []store.Permission{
				{PrincipalType: "user", PrincipalName: "Ima Fernandes da Costa", Roles: []string{"write"}},
				{PrincipalType: "siteUser", PrincipalName: "Robert Røsten", Roles: []string{"read"}},
			},
			want: VisibilityPrivate,
		},
		{
			// The connector's own app principal is present in almost every ACL
			// it can read. Counting it would make every item look org-visible.
			name: "connector application principal alone does not widen",
			permissions: []store.Permission{
				{PrincipalType: "application", PrincipalName: "finspo-core", Roles: []string{"read"}},
			},
			want: VisibilityPrivate,
		},
		{
			name: "application plus individuals still private",
			permissions: []store.Permission{
				{PrincipalType: "application", PrincipalName: "finspo-core"},
				{PrincipalType: "user", PrincipalName: "Ima Fernandes da Costa"},
			},
			want: VisibilityPrivate,
		},
		{
			name: "a group grant anywhere in the ACL wins over individuals",
			permissions: []store.Permission{
				{PrincipalType: "user", PrincipalName: "Ima Fernandes da Costa"},
				{PrincipalType: "application", PrincipalName: "finspo-core"},
				{PrincipalType: "group", PrincipalName: "Alle ansatte"},
			},
			want: VisibilityOrg,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := ClassifyVisibility(tc.permissions); got != tc.want {
				t.Fatalf("ClassifyVisibility() = %q, want %q", got, tc.want)
			}
		})
	}
}
