package server

import (
	"context"
	"net"
	"strings"
	"testing"
	"time"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/browser-broker/internal/grant"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
)

func newTestClient(t *testing.T) mpv1.BrowserBrokerClient {
	t.Helper()

	listener := bufconn.Listen(1024 * 1024)
	grpcServer := grpc.NewServer()
	Register(grpcServer, newTestServer())

	go func() {
		_ = grpcServer.Serve(listener)
	}()

	t.Cleanup(func() {
		grpcServer.Stop()
		_ = listener.Close()
	})

	conn, err := grpc.DialContext(
		context.Background(),
		"bufnet",
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithContextDialer(func(context.Context, string) (net.Conn, error) {
			return listener.Dial()
		}),
	)
	if err != nil {
		t.Fatalf("DialContext: unexpected error: %v", err)
	}
	t.Cleanup(func() {
		_ = conn.Close()
	})

	return mpv1.NewBrowserBrokerClient(conn)
}

func newTestServer() *Server {
	return testServerFor(grant.NewStore(), authctx.Principal{
		OrganizationID: "org1", ActorID: "user1", PrincipalType: "user",
	})
}

func testServerFor(store *grant.Store, principal authctx.Principal) *Server {
	srv := NewServer(store)
	srv.principal = func(context.Context) (authctx.Principal, error) { return principal, nil }
	return srv
}

func TestGrantAccessIsPinnedToVerifiedOrganizationAndUser(t *testing.T) {
	store := grant.NewStore()
	owner := testServerFor(store, authctx.Principal{OrganizationID: "org1", ActorID: "user1", PrincipalType: "user"})
	acquired, err := owner.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "session1", Mode: "cloud", AllowedDomains: []string{"example.com"},
	})
	if err != nil {
		t.Fatal(err)
	}

	tests := []struct {
		name      string
		principal authctx.Principal
	}{
		{name: "wrong tenant", principal: authctx.Principal{OrganizationID: "org2", ActorID: "user1", PrincipalType: "user"}},
		{name: "wrong user", principal: authctx.Principal{OrganizationID: "org1", ActorID: "user2", PrincipalType: "user"}},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			other := testServerFor(store, tc.principal)
			if _, err := other.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: acquired.GetGrantId()}); status.Code(err) != codes.NotFound {
				t.Fatalf("validate code = %v, want NotFound", status.Code(err))
			}
			if _, err := other.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: acquired.GetGrantId()}); status.Code(err) != codes.NotFound {
				t.Fatalf("revoke code = %v, want NotFound", status.Code(err))
			}
		})
	}

	service := testServerFor(store, authctx.Principal{OrganizationID: "org1", ActorID: "service:quarry", PrincipalType: "service", Scopes: []string{"browser:read"}})
	if _, err := service.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: acquired.GetGrantId()}); err != nil {
		t.Fatalf("same-tenant scoped service validation: %v", err)
	}
}

func TestAcquireGrantRejectsCallerSuppliedWrongOrganization(t *testing.T) {
	_, err := newTestServer().AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org2", SessionKey: "session1", Mode: "cloud", AllowedDomains: []string{"example.com"},
	})
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("code = %v, want PermissionDenied", status.Code(err))
	}
}

func TestHealth_ReturnsOK(t *testing.T) {
	srv := newTestServer()
	resp, err := srv.Health(context.Background(), &BrowserHealthRequest{})
	if err != nil {
		t.Fatalf("Health: unexpected error: %v", err)
	}
	if resp.Status != "ok" {
		t.Errorf("Health: expected status=ok, got %q", resp.Status)
	}
}

func TestAcquireGrant_Success(t *testing.T) {
	srv := newTestServer()
	req := &AcquireGrantRequest{
		OrgId:      "org1",
		SessionKey: "session1",
		Mode:       "cloud",
		AllowedDomains: []string{
			"example.com",
		},
	}
	resp, err := srv.AcquireGrant(context.Background(), req)
	if err != nil {
		t.Fatalf("AcquireGrant: unexpected error: %v", err)
	}
	if resp.GetGrantId() == "" {
		t.Error("AcquireGrant: expected non-empty GrantID")
	}
	const wantPrefix = "https://browser-broker.local/grants/"
	if !strings.HasPrefix(resp.GetEndpoint(), wantPrefix) {
		t.Errorf("Endpoint %q missing prefix %q", resp.GetEndpoint(), wantPrefix)
	}
	if !strings.HasSuffix(resp.GetEndpoint(), resp.GetGrantId()) {
		t.Errorf("Endpoint %q missing GrantID suffix %q", resp.GetEndpoint(), resp.GetGrantId())
	}
	if resp.GetExpiresAt() == nil || !resp.GetExpiresAt().AsTime().After(time.Now()) {
		t.Errorf("ExpiresAt %v not in future", resp.GetExpiresAt())
	}
}

func TestRevokeGrant_NotFound(t *testing.T) {
	srv := newTestServer()
	_, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: "missing"})
	if got := status.Code(err); got != codes.NotFound {
		t.Errorf("expected NotFound, got %v (err=%v)", got, err)
	}
}

func TestRevokeGrant_Success(t *testing.T) {
	srv := newTestServer()
	acquired, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId:      "org1",
		SessionKey: "session1",
		Mode:       "cloud",
		AllowedDomains: []string{
			"example.com",
		},
	})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	resp, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: acquired.GetGrantId()})
	if err != nil {
		t.Fatalf("RevokeGrant: unexpected error: %v", err)
	}
	if !resp.Revoked {
		t.Error("expected Revoked=true")
	}
}

func TestValidateGrant_NotFound(t *testing.T) {
	srv := newTestServer()
	_, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: "missing"})
	if got := status.Code(err); got != codes.NotFound {
		t.Errorf("expected NotFound, got %v (err=%v)", got, err)
	}
}

func TestAcquireGrant_ValidatesRequiredFields(t *testing.T) {
	tests := []struct {
		name string
		req  *AcquireGrantRequest
	}{
		{name: "missing session_key", req: &AcquireGrantRequest{OrgId: "org1", Mode: "cloud"}},
		{name: "missing org_id", req: &AcquireGrantRequest{SessionKey: "session1", Mode: "cloud"}},
		{name: "invalid mode", req: &AcquireGrantRequest{SessionKey: "session1", OrgId: "org1", Mode: "desktop", AllowedDomains: []string{"example.com"}}},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			srv := newTestServer()
			_, err := srv.AcquireGrant(context.Background(), tc.req)
			if got := status.Code(err); got != codes.InvalidArgument {
				t.Fatalf("expected InvalidArgument, got %v (err=%v)", got, err)
			}
		})
	}
}

func TestValidateGrant_Success(t *testing.T) {
	srv := newTestServer()
	acquired, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId:      "org1",
		SessionKey: "session1",
		Mode:       "cloud",
		AllowedDomains: []string{
			"example.com",
		},
	})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	resp, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: acquired.GetGrantId()})
	if err != nil {
		t.Fatalf("ValidateGrant: unexpected error: %v", err)
	}
	if !resp.Active {
		t.Error("expected Active=true")
	}
	if resp.GetGrantId() != acquired.GetGrantId() {
		t.Errorf("GrantID: got %q, want %q", resp.GetGrantId(), acquired.GetGrantId())
	}
}

func TestValidateGrant_RevokedReturnsFailedPrecondition(t *testing.T) {
	srv := newTestServer()
	acquired, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId:      "org1",
		SessionKey: "session1",
		Mode:       "cloud",
		AllowedDomains: []string{
			"example.com",
		},
	})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	if _, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{GrantId: acquired.GetGrantId()}); err != nil {
		t.Fatalf("RevokeGrant: %v", err)
	}
	_, err = srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: acquired.GetGrantId()})
	if got := status.Code(err); got != codes.FailedPrecondition {
		t.Errorf("expected FailedPrecondition, got %v (err=%v)", got, err)
	}
}

func TestRevokeGrant_EmptyGrantIDReturnsInvalidArgument(t *testing.T) {
	srv := newTestServer()
	_, err := srv.RevokeGrant(context.Background(), &RevokeGrantRequest{})
	if got := status.Code(err); got != codes.InvalidArgument {
		t.Fatalf("expected InvalidArgument, got %v (err=%v)", got, err)
	}
}

func TestValidateGrant_EmptyGrantIDReturnsInvalidArgument(t *testing.T) {
	srv := newTestServer()
	_, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{})
	if got := status.Code(err); got != codes.InvalidArgument {
		t.Fatalf("expected InvalidArgument, got %v (err=%v)", got, err)
	}
}

func TestAcquireGrant_TransportRoundTrip(t *testing.T) {
	client := newTestClient(t)

	resp, err := client.AcquireGrant(context.Background(), &mpv1.AcquireGrantRequest{
		SessionKey: "session-transport",
		Mode:       "cloud",
		OrgId:      "org1",
		AllowedDomains: []string{
			"example.com",
		},
	})
	if err != nil {
		t.Fatalf("AcquireGrant transport: unexpected error: %v", err)
	}
	if resp.GetGrantId() == "" {
		t.Error("expected non-empty grant_id")
	}
	const wantPrefix = "https://browser-broker.local/grants/"
	if !strings.HasPrefix(resp.GetEndpoint(), wantPrefix) {
		t.Errorf("Endpoint %q missing prefix %q", resp.GetEndpoint(), wantPrefix)
	}
	if resp.GetExpiresAt() == nil || resp.GetExpiresAt().AsTime().Before(time.Now()) {
		t.Fatalf("expected future expires_at, got %v", resp.GetExpiresAt())
	}
}

func TestAcquireGrantRejectsEmptyDomainPolicy(t *testing.T) {
	_, err := newTestServer().AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "session1", Mode: "cloud",
	})
	if got := status.Code(err); got != codes.InvalidArgument {
		t.Fatalf("code = %v, want InvalidArgument (empty policy must never mean unrestricted)", got)
	}
}

func TestValidateGrantReturnsBrokerOwnedNormalizedDomainPolicy(t *testing.T) {
	srv := newTestServer()
	acquired, err := srv.AcquireGrant(context.Background(), &AcquireGrantRequest{
		OrgId: "org1", SessionKey: "session1", Mode: "cloud",
		AllowedDomains: []string{
			"  EXAMPLE.com  ",
			"api.example.com",
			"example.com",
		},
	})
	if err != nil {
		t.Fatalf("AcquireGrant: %v", err)
	}
	if got, want := acquired.GetAllowedDomains(), []string{"api.example.com", "example.com"}; !equalStrings(got, want) {
		t.Fatalf("acquired policy = %#v, want %#v", got, want)
	}

	validated, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: acquired.GetGrantId()})
	if err != nil {
		t.Fatalf("ValidateGrant: %v", err)
	}
	if got, want := validated.GetAllowedDomains(), []string{"api.example.com", "example.com"}; !equalStrings(got, want) {
		t.Fatalf("validated policy = %#v, want %#v", got, want)
	}

	// The response is a copy. A caller cannot mutate the persisted policy
	// returned to a later execution validation.
	validated.AllowedDomains[0] = "attacker.example"
	again, err := srv.ValidateGrant(context.Background(), &ValidateGrantRequest{GrantId: acquired.GetGrantId()})
	if err != nil {
		t.Fatalf("ValidateGrant after mutation: %v", err)
	}
	if got, want := again.GetAllowedDomains(), []string{"api.example.com", "example.com"}; !equalStrings(got, want) {
		t.Fatalf("persisted policy mutated = %#v, want %#v", got, want)
	}
}

func equalStrings(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	for index := range got {
		if got[index] != want[index] {
			return false
		}
	}
	return true
}
