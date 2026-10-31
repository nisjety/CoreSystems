package authz

import (
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
)

const renewMethod = "/model_plane.v1.BrowserBroker/RenewGrant"

// WS8.C: RenewGrant is a user's call. A service needs browser:write for it, and
// the plan registers no principal with that scope on a person's session; the
// handler also refuses a non-user (TestRenewGrant_RequiresUserPrincipal).
func TestRenewGrantIsAKnownMethodForUsersAndPinsTheTenant(t *testing.T) {
	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	if err := Authorize(user, renewMethod, &mpv1.RenewGrantRequest{GrantId: "g", OrgId: "org-a"}); err != nil {
		t.Fatalf("user renewing in own tenant: %v", err)
	}
	if err := Authorize(user, renewMethod, &mpv1.RenewGrantRequest{GrantId: "g", OrgId: "org-b"}); err == nil {
		t.Fatal("user renewing in another tenant was authorized")
	}

	reader := authctx.Principal{OrganizationID: "org-a", ActorID: "service:quarry", PrincipalType: "service", Scopes: []string{ScopeRead}}
	if err := Authorize(reader, renewMethod, &mpv1.RenewGrantRequest{GrantId: "g", OrgId: "org-a"}); err == nil {
		t.Fatal("a browser:read service (quarry-edge, X-1) must not renew grants")
	}
	noScope := authctx.Principal{OrganizationID: "org-a", ActorID: "service:x", PrincipalType: "service"}
	if err := Authorize(noScope, renewMethod, &mpv1.RenewGrantRequest{GrantId: "g", OrgId: "org-a"}); err == nil {
		t.Fatal("a service without scopes renewed a grant")
	}
	writer := authctx.Principal{OrganizationID: "org-a", ActorID: "service:y", PrincipalType: "service", Scopes: []string{ScopeWrite}}
	if err := Authorize(writer, renewMethod, &mpv1.RenewGrantRequest{GrantId: "g", OrgId: "org-b"}); err == nil {
		t.Fatal("a browser:write service renewed in another tenant")
	}
}

func TestUnknownBrowserBrokerMethodsStayRefused(t *testing.T) {
	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	if err := Authorize(user, "/model_plane.v1.BrowserBroker/RenewGrants", &mpv1.RenewGrantRequest{OrgId: "org-a"}); err == nil {
		t.Fatal("an unknown method was authorized")
	}
}
