package authz

import (
	"context"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/pem"
	"math/big"
	"net/http"
	"net/http/httptest"
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

// INNH-10 MP-1: what an api_key principal may call on the broker.

func keyPrincipal(scopes ...string) authctx.Principal {
	return authctx.Principal{
		OrganizationID: "org-a", ActorID: "user-a", PrincipalType: PrincipalAPIKey,
		KeyID: "key-1", Scopes: scopes,
	}
}

const bb = "/model_plane.v1.BrowserBroker/"

func TestAuthorize_APIKeyMethodTable(t *testing.T) {
	ok := &mpv1.AcquireGrantRequest{OrgId: "org-a", SessionKey: "s", AllowedDomains: []string{"example.com"}}
	cases := []struct {
		name      string
		principal authctx.Principal
		method    string
		request   any
		allowed   bool
	}{
		{"acquire allow-list session", keyPrincipal(ScopeAgent), "AcquireGrant", ok, true},
		{"revoke", keyPrincipal(ScopeAgent), "RevokeGrant", &mpv1.RevokeGrantRequest{GrantId: "g"}, true},
		{"acquire without browser:agent", keyPrincipal(), "AcquireGrant", ok, false},
		{"acquire with browser:write only", keyPrincipal(ScopeWrite, ScopeRead), "AcquireGrant", ok, false},
		{"revoke without browser:agent", keyPrincipal(ScopeRead), "RevokeGrant", &mpv1.RevokeGrantRequest{GrantId: "g"}, false},
		{"validate", keyPrincipal(ScopeAgent, ScopeRead), "ValidateGrant", &mpv1.ValidateGrantRequest{GrantId: "g"}, false},
		{"renew", keyPrincipal(ScopeAgent, ScopeWrite), "RenewGrant", &mpv1.RenewGrantRequest{GrantId: "g", OrgId: "org-a"}, false},
		{"derive run", keyPrincipal(ScopeAgent, ScopeWrite), "DeriveRunGrant", &mpv1.DeriveRunGrantRequest{OrgId: "org-a", ParentGrantId: "g"}, false},
		{"health", keyPrincipal(ScopeAgent), "Health", &mpv1.BrowserHealthRequest{}, false},
		{"unknown method", keyPrincipal(ScopeAgent), "AcquireKeyGrant", ok, false},
		{"acquire human_browse", keyPrincipal(ScopeAgent), "AcquireGrant",
			&mpv1.AcquireGrantRequest{OrgId: "org-a", SessionKey: "s", GrantKind: "human_browse"}, false},
		{"acquire unknown kind", keyPrincipal(ScopeAgent), "AcquireGrant",
			&mpv1.AcquireGrantRequest{OrgId: "org-a", SessionKey: "s", GrantKind: "session", AllowedDomains: []string{"example.com"}}, false},
		{"acquire sensitive action", keyPrincipal(ScopeAgent), "AcquireGrant",
			&mpv1.AcquireGrantRequest{OrgId: "org-a", SessionKey: "s", AllowedDomains: []string{"example.com"}, AllowedActions: []string{"download_ref"}}, false},
		{"acquire with a frame scope", keyPrincipal(ScopeAgent), "AcquireGrant",
			&mpv1.AcquireGrantRequest{OrgId: "org-a", SessionKey: "s", AllowedDomains: []string{"example.com"}, AllowedFrameIds: []string{"f"}}, false},
		{"acquire with a dialog scope", keyPrincipal(ScopeAgent), "AcquireGrant",
			&mpv1.AcquireGrantRequest{OrgId: "org-a", SessionKey: "s", AllowedDomains: []string{"example.com"}, AllowedDialogIds: []string{"d"}}, false},
		{"acquire with an artifact scope", keyPrincipal(ScopeAgent), "AcquireGrant",
			&mpv1.AcquireGrantRequest{OrgId: "org-a", SessionKey: "s", AllowedDomains: []string{"example.com"}, AllowedArtifactIds: []string{"art_1"}}, false},
		{"acquire under a parent", keyPrincipal(ScopeAgent), "AcquireGrant",
			&mpv1.AcquireGrantRequest{OrgId: "org-a", SessionKey: "s", AllowedDomains: []string{"example.com"}, ParentGrantId: "g"}, false},
		{"acquire in another org", keyPrincipal(ScopeAgent), "AcquireGrant",
			&mpv1.AcquireGrantRequest{OrgId: "org-b", SessionKey: "s", AllowedDomains: []string{"example.com"}}, false},
		{"acquire with a mistyped request", keyPrincipal(ScopeAgent), "AcquireGrant", &mpv1.RevokeGrantRequest{}, false},
		{"acquire with a nil request", keyPrincipal(ScopeAgent), "AcquireGrant", (*mpv1.AcquireGrantRequest)(nil), false},
		{"key principal without key id", authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: PrincipalAPIKey, Scopes: []string{ScopeAgent}}, "AcquireGrant", ok, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := Authorize(tc.principal, bb+tc.method, tc.request)
			if tc.allowed && err != nil {
				t.Fatalf("refused: %v", err)
			}
			if !tc.allowed && err == nil {
				t.Fatal("authorized")
			}
		})
	}
}

// browser:agent is the key's scope only: a user or service is not changed by it.
func TestAuthorize_BrowserAgentGivesAServiceNothing(t *testing.T) {
	service := authctx.Principal{OrganizationID: "org-a", ActorID: "service:gw", PrincipalType: "service", Scopes: []string{ScopeAgent}}
	for _, method := range []string{"AcquireGrant", "RevokeGrant", "ValidateGrant", "DeriveRunGrant", "RenewGrant"} {
		if err := Authorize(service, bb+method, &mpv1.AcquireGrantRequest{OrgId: "org-a"}); err == nil {
			t.Fatalf("a browser:agent service was authorized for %s", method)
		}
	}
}

func TestKeyMatches(t *testing.T) {
	key := keyPrincipal(ScopeAgent)
	if !KeyMatches(key, "key-1") {
		t.Fatal("own key refused")
	}
	for _, other := range []string{"", "key-2", "KEY-1", "key-1 "} {
		if KeyMatches(key, other) {
			t.Fatalf("key matched grant key %q", other)
		}
	}
	noKey := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: PrincipalAPIKey}
	if KeyMatches(noKey, "") {
		t.Fatal("a key principal without key id matched an unkeyed grant")
	}
	user := authctx.Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: "user"}
	if !KeyMatches(user, "") || !KeyMatches(user, "key-1") {
		t.Fatal("the key rule must not narrow a user (the owner rule applies)")
	}
}

// End to end through the real interceptor: the broker verifier accepts a
// well-formed api_key token and Authorize decides per method.
func TestInterceptor_APIKeyTokenReachesAcquireButNotValidate(t *testing.T) {
	t.Setenv("BROWSER_BROKER_AUTH_AUDIENCE", "")
	verifier, key := apiKeyAuth(t)
	interceptor := UnaryInterceptor(verifier)
	now := time.Now()
	raw, err := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"iss": "https://auth.test/issuer", "aud": []string{Audience}, "sub": "apikey:key-1",
		"principal_type": "api_key", "key_id": "key-1", "acting_user_id": "user-a", "org_id": "org-a",
		"scopes": []string{ScopeAgent}, "iat": now.Add(-time.Minute).Unix(), "nbf": now.Add(-time.Minute).Unix(),
		"exp": now.Add(2 * time.Minute).Unix(),
	}).SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	ctx := metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+raw))
	handler := func(ctx context.Context, _ any) (any, error) {
		p, err := Principal(ctx)
		if err != nil || p.KeyID != "key-1" || p.ActorID != "user-a" {
			t.Fatalf("principal = %+v, %v", p, err)
		}
		return "ok", nil
	}
	if _, err := interceptor(ctx, &mpv1.AcquireGrantRequest{OrgId: "org-a"}, &grpc.UnaryServerInfo{FullMethod: bb + "AcquireGrant"}, handler); err != nil {
		t.Fatalf("acquire: %v", err)
	}
	_, err = interceptor(ctx, &mpv1.ValidateGrantRequest{GrantId: "g"}, &grpc.UnaryServerInfo{FullMethod: bb + "ValidateGrant"}, handler)
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("validate code = %v, want PermissionDenied", status.Code(err))
	}
}

// apiKeyAuth is testAuth with the broker's AcceptAPIKey setting.
func apiKeyAuth(t *testing.T) (*authctx.Verifier, *rsa.PrivateKey) {
	t.Helper()
	_, key := testAuth(t)
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences: []string{Audience}, Issuer: "https://auth.test/issuer",
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der}), AcceptAPIKey: true,
	})
	if err != nil {
		t.Fatal(err)
	}
	return verifier, key
}

// The broker's own verifier opts in to the api_key shape (the other Model Plane
// services do not). JWKS is needed to build it from the environment, so this
// reads the config the broker passes.
func TestNewVerifierFromEnv_AcceptsAPIKeyShape(t *testing.T) {
	_, key := testAuth(t)
	der, _ := x509.MarshalPKIXPublicKey(&key.PublicKey)
	jwks := jwksServer(t, key, der)
	t.Setenv("AUTH_CORE_JWKS_URL", jwks)
	t.Setenv("AUTH_CORE_ISSUER", "https://auth.test/issuer")
	t.Setenv("BROWSER_BROKER_AUTH_AUDIENCE", "")
	verifier, err := NewVerifierFromEnv()
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"iss": "https://auth.test/issuer", "aud": []string{Audience}, "sub": "apikey:key-1",
		"principal_type": "api_key", "key_id": "key-1", "acting_user_id": "user-a", "org_id": "org-a",
		"iat": now.Add(-time.Minute).Unix(), "nbf": now.Add(-time.Minute).Unix(), "exp": now.Add(time.Minute).Unix(),
	})
	token.Header["kid"] = "k1"
	raw, err := token.SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	if p, err := verifier.Verify(raw); err != nil || p.KeyID != "key-1" {
		t.Fatalf("broker verifier refused a well-formed api_key token: %+v %v", p, err)
	}
}

func jwksServer(t *testing.T, key *rsa.PrivateKey, _ []byte) string {
	t.Helper()
	n := base64.RawURLEncoding.EncodeToString(key.PublicKey.N.Bytes())
	e := base64.RawURLEncoding.EncodeToString(big.NewInt(int64(key.PublicKey.E)).Bytes())
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"keys":[{"kid":"k1","kty":"RSA","use":"sig","alg":"RS256","n":"` + n + `","e":"` + e + `"}]}`))
	}))
	t.Cleanup(server.Close)
	return server.URL
}
