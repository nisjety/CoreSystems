package authz

import (
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
)

const deriveMethod = "/model_plane.v1.BrowserBroker/DeriveRunGrant"

func TestDeriveRunGrantNeedsBrowserWriteForServicesAndPinsTheTenant(t *testing.T) {
	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	if err := Authorize(user, deriveMethod, &mpv1.DeriveRunGrantRequest{OrgId: "org-a"}); err != nil {
		t.Fatalf("user in own tenant: %v", err)
	}
	if err := Authorize(user, deriveMethod, &mpv1.DeriveRunGrantRequest{OrgId: "org-b"}); err == nil {
		t.Fatal("user deriving in another tenant was authorized")
	}

	reader := authctx.Principal{OrganizationID: "org-a", ActorID: "service:quarry", PrincipalType: "service", Scopes: []string{ScopeRead}}
	if err := Authorize(reader, deriveMethod, &mpv1.DeriveRunGrantRequest{OrgId: "org-a"}); err == nil {
		t.Fatal("a browser:read service (quarry-edge, X-1) must not derive grants")
	}
	noScope := authctx.Principal{OrganizationID: "org-a", ActorID: "service:x", PrincipalType: "service"}
	if err := Authorize(noScope, deriveMethod, &mpv1.DeriveRunGrantRequest{OrgId: "org-a"}); err == nil {
		t.Fatal("a service without scopes derived a grant")
	}
	writer := authctx.Principal{OrganizationID: "org-a", ActorID: "service:y", PrincipalType: "service", Scopes: []string{ScopeWrite}}
	if err := Authorize(writer, deriveMethod, &mpv1.DeriveRunGrantRequest{OrgId: "org-a"}); err != nil {
		t.Fatalf("browser:write service: %v", err)
	}
	if err := Authorize(writer, deriveMethod, &mpv1.DeriveRunGrantRequest{OrgId: "org-b"}); err == nil {
		t.Fatal("a browser:write service derived in another tenant")
	}
}
