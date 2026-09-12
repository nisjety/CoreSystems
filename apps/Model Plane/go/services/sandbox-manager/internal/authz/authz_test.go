package authz

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

type testClaims struct {
	OrgID         string   `json:"org_id"`
	UserID        string   `json:"user_id,omitempty"`
	ServiceID     string   `json:"service_id,omitempty"`
	PrincipalType string   `json:"principal_type"`
	Scopes        []string `json:"scopes,omitempty"`
	jwt.RegisteredClaims
}

func TestAuthorizationPinsTenantAndPrincipalContext(t *testing.T) {
	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	if err := Authorize(user, "/model_plane.v1.SandboxManager/AcquireLease", &mpv1.AcquireLeaseRequest{OrgId: "org-b"}); err == nil {
		t.Fatal("wrong tenant unexpectedly authorized")
	}
	if err := Authorize(user, "/model_plane.v1.SandboxManager/FutureMethod", nil); err == nil {
		t.Fatal("unknown method unexpectedly authorized")
	}
	if OwnerFilter(user) != "user-a" {
		t.Fatal("user owner filter not pinned")
	}
	service := authctx.Principal{OrganizationID: "org-a", ActorID: "service:execution-core", PrincipalType: "service", Scopes: []string{ScopeRead}}
	if OwnerFilter(service) != "" {
		t.Fatal("scoped service should use organization-wide owner filter")
	}
	if err := Authorize(service, "/model_plane.v1.SandboxManager/Health", nil); err != nil {
		t.Fatal(err)
	}

	if _, err := Principal(context.Background()); status.Code(err) != codes.Unauthenticated {
		t.Fatalf("principal code = %v", status.Code(err))
	}
	verifier, key := testAuth(t)
	interceptor := UnaryInterceptor(verifier)
	ctx := metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+token(t, key, Audience, "org-a", "user-a", "user")))
	_, err := interceptor(ctx, nil, &grpc.UnaryServerInfo{FullMethod: "/model_plane.v1.SandboxManager/Health"}, func(ctx context.Context, _ any) (any, error) {
		principal, err := Principal(ctx)
		if err != nil || principal.ActorID != "user-a" {
			t.Fatalf("principal = %#v, %v", principal, err)
		}
		return "ok", nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

// TestActivateLeaseAndGetWorkspaceManifestAreKnownMethods is a direct,
// JWT-independent regression lock for the 2026-09-12 fix: Authorize's
// switch previously had no case for ActivateLease at all (it fell into
// `default: return errors.New("unknown sandbox-manager method")`) and
// GetWorkspaceManifest never existed before 3.5.C. Both must now be
// recognized, with the same ScopeWrite/ScopeRead split as every other
// mutating/read-only method on this service.
func TestActivateLeaseAndGetWorkspaceManifestAreKnownMethods(t *testing.T) {
	writeOnly := authctx.Principal{OrganizationID: "org-a", ActorID: "service:execution-core", PrincipalType: "service", Scopes: []string{ScopeWrite}}
	if err := Authorize(writeOnly, "/model_plane.v1.SandboxManager/ActivateLease", nil); err != nil {
		t.Fatalf("ActivateLease with sandbox:write unexpectedly refused: %v", err)
	}
	readOnly := authctx.Principal{OrganizationID: "org-a", ActorID: "service:execution-core", PrincipalType: "service", Scopes: []string{ScopeRead}}
	if err := Authorize(readOnly, "/model_plane.v1.SandboxManager/ActivateLease", nil); err == nil {
		t.Fatal("ActivateLease with only sandbox:read unexpectedly authorized")
	}
	if err := Authorize(readOnly, "/model_plane.v1.SandboxManager/GetWorkspaceManifest", nil); err != nil {
		t.Fatalf("GetWorkspaceManifest with sandbox:read unexpectedly refused: %v", err)
	}
	noScope := authctx.Principal{OrganizationID: "org-a", ActorID: "service:execution-core", PrincipalType: "service"}
	if err := Authorize(noScope, "/model_plane.v1.SandboxManager/GetWorkspaceManifest", nil); err == nil {
		t.Fatal("GetWorkspaceManifest with no scope unexpectedly authorized")
	}
}

func testAuth(t *testing.T) (*authctx.Verifier, *rsa.PrivateKey) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := authctx.NewVerifier(authctx.Config{Audiences: []string{Audience}, Issuer: "https://auth.test/issuer", PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der})})
	if err != nil {
		t.Fatal(err)
	}
	return verifier, key
}

func token(t *testing.T, key *rsa.PrivateKey, audience, org, actor, principalType string, scopes ...string) string {
	t.Helper()
	claims := testClaims{OrgID: org, PrincipalType: principalType, Scopes: scopes, RegisteredClaims: jwt.RegisteredClaims{Audience: jwt.ClaimStrings{audience}, Issuer: "https://auth.test/issuer", Subject: actor, IssuedAt: jwt.NewNumericDate(time.Now().Add(-time.Minute)), NotBefore: jwt.NewNumericDate(time.Now().Add(-time.Minute)), ExpiresAt: jwt.NewNumericDate(time.Now().Add(time.Hour))}}
	if principalType == "service" {
		claims.ServiceID = actor
	} else {
		claims.UserID = actor
	}
	raw, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func TestInterceptorFailsClosedAndEnforcesServiceScopes(t *testing.T) {
	verifier, key := testAuth(t)
	interceptor := UnaryInterceptor(verifier)
	handler := func(context.Context, any) (any, error) { return "ok", nil }
	tests := []struct {
		name, method, authorization string
		want                        codes.Code
	}{
		{name: "missing", method: "/model_plane.v1.SandboxManager/AcquireLease", want: codes.Unauthenticated},
		{name: "malformed", method: "/model_plane.v1.SandboxManager/AcquireLease", authorization: "Bearer no", want: codes.Unauthenticated},
		{name: "wrong audience", method: "/model_plane.v1.SandboxManager/AcquireLease", authorization: "Bearer " + token(t, key, "model-gateway", "org-a", "user-a", "user"), want: codes.Unauthenticated},
		{name: "internal credential missing scope", method: "/model_plane.v1.SandboxManager/SnapshotSandbox", authorization: "Bearer " + token(t, key, Audience, "org-a", "service:execution-core", "service", ScopeRead), want: codes.PermissionDenied},
		{name: "valid service", method: "/model_plane.v1.SandboxManager/SnapshotSandbox", authorization: "Bearer " + token(t, key, Audience, "org-a", "service:execution-core", "service", ScopeWrite), want: codes.OK},
		// ActivateLease was missing from Authorize's switch entirely until
		// 2026-09-12 (S3.3 step 3.5.C) -- these two cases lock in the fix:
		// it is reachable through the real interceptor now, with the same
		// ScopeWrite gate as the other three write RPCs, not silently
		// refused as "unknown sandbox-manager method" regardless of scope.
		{name: "ActivateLease missing scope", method: "/model_plane.v1.SandboxManager/ActivateLease", authorization: "Bearer " + token(t, key, Audience, "org-a", "service:execution-core", "service", ScopeRead), want: codes.PermissionDenied},
		{name: "ActivateLease valid service", method: "/model_plane.v1.SandboxManager/ActivateLease", authorization: "Bearer " + token(t, key, Audience, "org-a", "service:execution-core", "service", ScopeWrite), want: codes.OK},
		{name: "GetWorkspaceManifest valid service with read scope", method: "/model_plane.v1.SandboxManager/GetWorkspaceManifest", authorization: "Bearer " + token(t, key, Audience, "org-a", "service:execution-core", "service", ScopeRead), want: codes.OK},
		{name: "GetWorkspaceManifest missing read scope", method: "/model_plane.v1.SandboxManager/GetWorkspaceManifest", authorization: "Bearer " + token(t, key, Audience, "org-a", "service:execution-core", "service", ScopeWrite), want: codes.PermissionDenied},
		{name: "custom health is protected", method: "/model_plane.v1.SandboxManager/Health", want: codes.Unauthenticated},
		{name: "standard health is public", method: "/grpc.health.v1.Health/Check", want: codes.OK},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			ctx := context.Background()
			if tc.authorization != "" {
				ctx = metadata.NewIncomingContext(ctx, metadata.Pairs("authorization", tc.authorization))
			}
			_, err := interceptor(ctx, nil, &grpc.UnaryServerInfo{FullMethod: tc.method}, handler)
			if got := status.Code(err); got != tc.want {
				t.Fatalf("code = %v, want %v (err=%v)", got, tc.want, err)
			}
		})
	}
}

func TestStartupAuthenticationConfigurationFailsClosed(t *testing.T) {
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("AUTH_CORE_ISSUER", "")
	t.Setenv("SANDBOX_MANAGER_AUTH_AUDIENCE", Audience)
	if _, err := NewVerifierFromEnv(); err == nil {
		t.Fatal("missing trust configuration unexpectedly succeeded")
	}
	t.Setenv("SANDBOX_MANAGER_AUTH_AUDIENCE", "model-gateway")
	if _, err := NewVerifierFromEnv(); err == nil {
		t.Fatal("wrong configured audience unexpectedly succeeded")
	}
}
