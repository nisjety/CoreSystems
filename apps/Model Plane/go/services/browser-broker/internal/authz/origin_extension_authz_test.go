package authz

import (
	"testing"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
)

const acquireMethod = "/model_plane.v1.BrowserBroker/AcquireGrant"

// E3.2-e1: an origin extension records a person's consent. A user may ask; a
// workload with browser:write may not, and an api key never.
func TestOriginExtensionIsAUsersAskOnly(t *testing.T) {
	req := &mpv1.AcquireGrantRequest{OrgId: "org-a", GrantKind: GrantKindOriginExtension, ConsentScope: "turn"}

	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	if err := Authorize(user, acquireMethod, req); err != nil {
		t.Fatalf("user: %v", err)
	}
	writer := authctx.Principal{OrganizationID: "org-a", ActorID: "service:y", PrincipalType: "service", Scopes: []string{ScopeWrite}}
	if err := Authorize(writer, acquireMethod, req); err == nil {
		t.Fatal("a browser:write service was authorized to issue an origin extension")
	}
	// The same service still acquires an ordinary grant.
	if err := Authorize(writer, acquireMethod, &mpv1.AcquireGrantRequest{OrgId: "org-a"}); err != nil {
		t.Fatalf("service ordinary acquire: %v", err)
	}
	key := authctx.Principal{
		OrganizationID: "org-a", ActorID: "user-a", PrincipalType: PrincipalAPIKey, KeyID: "key-1",
		Scopes: []string{ScopeAgent},
	}
	if err := Authorize(key, acquireMethod, req); err == nil {
		t.Fatal("an api key was authorized to issue an origin extension")
	}
	// A consent field alone, on an ordinary request, is not a key's to send.
	if err := Authorize(key, acquireMethod, &mpv1.AcquireGrantRequest{OrgId: "org-a", ConsentScope: "turn"}); err == nil {
		t.Fatal("an api key carried a consent")
	}
	if err := Authorize(key, acquireMethod, &mpv1.AcquireGrantRequest{OrgId: "org-a", ConsentDeniedDomains: []string{"x.org"}}); err == nil {
		t.Fatal("an api key carried consent denies")
	}
}
