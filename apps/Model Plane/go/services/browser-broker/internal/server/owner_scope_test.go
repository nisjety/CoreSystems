package server

import (
	"context"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// INNH-10 fix first, at the handler: even if a principal of a type the broker
// does not know got past Authorize, it reads and revokes nothing in its org (it
// used to get the empty, org-wide owner filter of a scoped service).
func TestUnknownPrincipalTypeSeesNoGrantOfItsOrg(t *testing.T) {
	store := grant.NewMemoryRepository()
	owner := testServerFor(store, authctx.Principal{OrganizationID: "org1", ActorID: "user1", PrincipalType: "user"})
	acquired, err := owner.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "s1", Mode: "cloud", AllowedDomains: []string{"example.com"},
	})
	if err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"", "robot", "api_key"} {
		stranger := testServerFor(store, authctx.Principal{
			OrganizationID: "org1", ActorID: "user1", PrincipalType: kind,
			Scopes: []string{"browser:read", "browser:write"},
		})
		if _, err := stranger.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: acquired.GetGrantId()}); status.Code(err) != codes.NotFound {
			t.Fatalf("type %q validate code = %v, want NotFound", kind, status.Code(err))
		}
		if _, err := stranger.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: acquired.GetGrantId()}); status.Code(err) != codes.NotFound {
			t.Fatalf("type %q revoke code = %v, want NotFound", kind, status.Code(err))
		}
	}
	if _, err := owner.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: acquired.GetGrantId()}); err != nil {
		t.Fatalf("the grant must still be live for its owner: %v", err)
	}
}
