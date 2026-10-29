package authz

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
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

func TestHTTPMiddlewareFailsClosedAndEnforcesDedicatedAudienceAndScopes(t *testing.T) {
	verifier, key := testAuth(t)
	called := false
	handler := verifier.HTTPMiddleware(Authorize)(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { called = true; w.WriteHeader(http.StatusNoContent) }))
	tests := []struct {
		name, method, authorization string
		want                        int
	}{
		{name: "missing", method: http.MethodGet, want: http.StatusUnauthorized},
		{name: "malformed", method: http.MethodGet, authorization: "Bearer no", want: http.StatusUnauthorized},
		{name: "wrong audience", method: http.MethodGet, authorization: "Bearer " + token(t, key, "model-gateway", "org-a", "user-a", "user"), want: http.StatusUnauthorized},
		{name: "internal credential missing read scope", method: http.MethodGet, authorization: "Bearer " + token(t, key, Audience, "org-a", "service:bridge-cli", "service", ScopeWrite), want: http.StatusForbidden},
		{name: "internal credential missing write scope", method: http.MethodPost, authorization: "Bearer " + token(t, key, Audience, "org-a", "service:bridge-cli", "service", ScopeRead), want: http.StatusForbidden},
		{name: "valid user", method: http.MethodPost, authorization: "Bearer " + token(t, key, Audience, "org-a", "user-a", "user"), want: http.StatusNoContent},
		{name: "valid service", method: http.MethodGet, authorization: "Bearer " + token(t, key, Audience, "org-a", "service:bridge-cli", "service", ScopeRead), want: http.StatusNoContent},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			called = false
			req := httptest.NewRequest(tc.method, "/api/v1/sessions", nil)
			req.Header.Set("Authorization", tc.authorization)
			res := httptest.NewRecorder()
			handler.ServeHTTP(res, req)
			if res.Code != tc.want {
				t.Fatalf("status = %d, want %d; body=%s", res.Code, tc.want, res.Body.String())
			}
			if called != (tc.want == http.StatusNoContent) {
				t.Fatalf("called=%v", called)
			}
		})
	}
}

func TestStartupAuthenticationConfigurationFailsClosed(t *testing.T) {
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("AUTH_CORE_ISSUER", "")
	t.Setenv("BRIDGE_CORE_AUTH_AUDIENCE", Audience)
	if _, err := NewVerifierFromEnv(); err == nil {
		t.Fatal("missing trust configuration unexpectedly succeeded")
	}
	t.Setenv("BRIDGE_CORE_AUTH_AUDIENCE", "mp-bridge")
	if _, err := NewVerifierFromEnv(); err == nil {
		t.Fatal("wrong configured audience unexpectedly succeeded")
	}
}

func TestGRPCListenerExposesOnlyPublicStandardHealth(t *testing.T) {
	verifier, key := testAuth(t)
	interceptor := UnaryInterceptor(verifier)
	handler := func(context.Context, any) (any, error) { return "ok", nil }
	if _, err := interceptor(context.Background(), nil, &grpc.UnaryServerInfo{FullMethod: "/grpc.health.v1.Health/Check"}, handler); err != nil {
		t.Fatalf("standard health: %v", err)
	}
	ctx := metadata.NewIncomingContext(context.Background(), metadata.Pairs(
		"authorization", "Bearer "+token(t, key, Audience, "org-a", "user-a", "user"),
	))
	if _, err := interceptor(ctx, nil, &grpc.UnaryServerInfo{FullMethod: "/model_plane.v1.Future/Unsafe"}, handler); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("future method code = %v, want PermissionDenied", status.Code(err))
	}
}
