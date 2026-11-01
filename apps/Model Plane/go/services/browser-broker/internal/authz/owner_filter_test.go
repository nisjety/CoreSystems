package authz

import (
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
)

// INNH-10 fix first: OwnerFilter returned "" (any owner in the org) for every
// principal that was not a user, so a new principal type would have been read
// as an org-wide scoped service. Only a service gets the empty filter now.
func TestOwnerFilter_OnlyAServiceGetsTheOrgWideFilter(t *testing.T) {
	cases := []struct {
		name      string
		principal authctx.Principal
		want      string
	}{
		{"user is pinned to itself", authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}, "user-a"},
		{"service is org wide", authctx.Principal{OrganizationID: "org-a", ActorID: "service:quarry", PrincipalType: "service"}, ""},
		{"api_key is never org wide", authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "api_key"}, noOwner},
		{"unknown type matches nothing", authctx.Principal{OrganizationID: "org-a", ActorID: "x", PrincipalType: "robot"}, noOwner},
		{"empty type matches nothing", authctx.Principal{OrganizationID: "org-a", ActorID: "x"}, noOwner},
		{"case variant is not a service", authctx.Principal{OrganizationID: "org-a", ActorID: "x", PrincipalType: "Service"}, noOwner},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := OwnerFilter(tc.principal); got != tc.want {
				t.Fatalf("OwnerFilter = %q, want %q", got, tc.want)
			}
			if tc.principal.PrincipalType != "service" && OwnerFilter(tc.principal) == "" {
				t.Fatal("a non-service principal got the org-wide owner filter")
			}
		})
	}
}

// A principal type the broker does not know is refused on every method, even
// when it holds every browser scope; it no longer falls into the service branch.
func TestAuthorize_UnknownPrincipalTypesAreRefusedOnEveryMethod(t *testing.T) {
	methods := []string{"AcquireGrant", "RevokeGrant", "ValidateGrant", "DeriveRunGrant", "RenewGrant", "Health"}
	for _, kind := range []string{"", "robot", "Service", "USER", "api_key_v2"} {
		principal := authctx.Principal{
			OrganizationID: "org-a", ActorID: "x", PrincipalType: kind,
			Scopes: []string{ScopeRead, ScopeWrite, "browser:agent"},
		}
		for _, method := range methods {
			full := "/model_plane.v1.BrowserBroker/" + method
			if err := Authorize(principal, full, &mpv1.AcquireGrantRequest{OrgId: "org-a"}); err == nil {
				t.Fatalf("principal type %q was authorized for %s", kind, method)
			}
		}
	}
}
