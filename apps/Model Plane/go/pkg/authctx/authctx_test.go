package authctx

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"math/big"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

const testIssuer = "https://auth.example.test/api/convex-auth"

type testClaims struct {
	OrgID         string   `json:"org_id"`
	UserID        string   `json:"user_id,omitempty"`
	ServiceID     string   `json:"service_id,omitempty"`
	PrincipalType string   `json:"principal_type"`
	Scopes        []string `json:"scopes,omitempty"`
	ZDR           *bool    `json:"zdr,omitempty"`
	jwt.RegisteredClaims
}

func testVerifier(t *testing.T) (*Verifier, *rsa.PrivateKey) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	publicDER, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	publicPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: publicDER})
	verifier, err := NewVerifier(Config{
		Audiences:    []string{"model-gateway"},
		Issuer:       testIssuer,
		PublicKeyPEM: publicPEM,
	})
	if err != nil {
		t.Fatal(err)
	}
	return verifier, key
}

func signToken(t *testing.T, key *rsa.PrivateKey, claims testClaims) string {
	t.Helper()
	if claims.Issuer == "" {
		claims.Issuer = testIssuer
	}
	if claims.Audience == nil {
		claims.Audience = jwt.ClaimStrings{"model-gateway"}
	}
	if claims.Subject == "" {
		if claims.PrincipalType == "service" {
			claims.Subject = claims.ServiceID
		} else {
			claims.Subject = claims.UserID
		}
	}
	if claims.IssuedAt == nil {
		claims.IssuedAt = jwt.NewNumericDate(time.Now().Add(-time.Minute))
	}
	if claims.NotBefore == nil {
		claims.NotBefore = jwt.NewNumericDate(time.Now().Add(-time.Minute))
	}
	if claims.ExpiresAt == nil {
		claims.ExpiresAt = jwt.NewNumericDate(time.Now().Add(time.Hour))
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	raw, err := token.SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func userToken(t *testing.T, key *rsa.PrivateKey) string {
	return signToken(t, key, testClaims{
		OrgID:         "org-a",
		UserID:        "user-a",
		PrincipalType: "user",
	})
}

func TestVerifyPreservesSignedRetentionPolicy(t *testing.T) {
	verifier, key := testVerifier(t)
	zdr := true
	principal, err := verifier.Verify(signToken(t, key, testClaims{
		OrgID: "org-a", UserID: "user-a", PrincipalType: "user", ZDR: &zdr,
	}))
	if err != nil {
		t.Fatal(err)
	}
	if !principal.RetentionPolicyPresent || !principal.ZeroDataRetention {
		t.Fatalf("retention policy = %+v", principal)
	}

	principal, err = verifier.Verify(userToken(t, key))
	if err != nil {
		t.Fatal(err)
	}
	if principal.RetentionPolicyPresent || principal.ZeroDataRetention {
		t.Fatalf("unspecified retention became authoritative: %+v", principal)
	}
}

func TestHTTPMiddlewareFailsClosedAndCanonicalizesIdentity(t *testing.T) {
	verifier, key := testVerifier(t)
	called := false
	handler := verifier.HTTPMiddleware(nil)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		called = true
		principal, ok := PrincipalFromContext(r.Context())
		if !ok || principal.OrganizationID != "org-a" || principal.ActorID != "user-a" {
			t.Fatalf("unexpected verified principal: %#v, ok=%v", principal, ok)
		}
		if got := r.Header.Get("X-Org-ID"); got != "org-a" {
			t.Fatalf("canonical org header = %q", got)
		}
		if got := r.Header.Get("X-User-ID"); got != "user-a" {
			t.Fatalf("canonical user header = %q", got)
		}
		w.WriteHeader(http.StatusNoContent)
	}))

	tests := []struct {
		name          string
		authorization string
		orgHeader     string
		userHeader    string
		wantStatus    int
	}{
		{name: "missing", wantStatus: http.StatusUnauthorized},
		{name: "malformed", authorization: "Bearer not-a-jwt", wantStatus: http.StatusUnauthorized},
		{name: "forged org", authorization: "Bearer " + userToken(t, key), orgHeader: "org-b", wantStatus: http.StatusForbidden},
		{name: "forged user", authorization: "Bearer " + userToken(t, key), userHeader: "user-b", wantStatus: http.StatusForbidden},
		{name: "valid", authorization: "Bearer " + userToken(t, key), wantStatus: http.StatusNoContent},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			called = false
			req := httptest.NewRequest(http.MethodGet, "/sensitive", nil)
			req.Header.Set("Authorization", tc.authorization)
			req.Header.Set("X-Org-ID", tc.orgHeader)
			req.Header.Set("X-User-ID", tc.userHeader)
			res := httptest.NewRecorder()
			handler.ServeHTTP(res, req)
			if res.Code != tc.wantStatus {
				t.Fatalf("status = %d, want %d; body=%s", res.Code, tc.wantStatus, res.Body.String())
			}
			if called != (tc.wantStatus == http.StatusNoContent) {
				t.Fatalf("downstream called = %v", called)
			}
		})
	}
}

func TestVerifierRejectsWrongAudienceExpiredAndAmbiguousIdentity(t *testing.T) {
	verifier, key := testVerifier(t)
	tests := []struct {
		name   string
		claims testClaims
	}{
		{
			name: "wrong audience",
			claims: testClaims{OrgID: "org-a", UserID: "user-a", PrincipalType: "user",
				RegisteredClaims: jwt.RegisteredClaims{Audience: jwt.ClaimStrings{"data-plane"}}},
		},
		{
			name: "expired",
			claims: testClaims{OrgID: "org-a", UserID: "user-a", PrincipalType: "user",
				RegisteredClaims: jwt.RegisteredClaims{ExpiresAt: jwt.NewNumericDate(time.Now().Add(-time.Hour))}},
		},
		{name: "missing organization", claims: testClaims{UserID: "user-a", PrincipalType: "user"}},
		{name: "ambiguous user", claims: testClaims{OrgID: "org-a", UserID: "user-a", ServiceID: "service-a", PrincipalType: "user"}},
		{name: "service without scopes", claims: testClaims{OrgID: "org-a", ServiceID: "service-a", PrincipalType: "service"}},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := verifier.Verify(signToken(t, key, tc.claims)); err == nil {
				t.Fatal("expected verification failure")
			}
		})
	}
}

func TestServicePrincipalAndScopeChecks(t *testing.T) {
	verifier, key := testVerifier(t)
	raw := signToken(t, key, testClaims{
		OrgID:         "org-a",
		ServiceID:     "service:model-gateway",
		PrincipalType: "service",
		Scopes:        []string{"cost:read"},
	})
	principal, err := verifier.Verify(raw)
	if err != nil {
		t.Fatal(err)
	}
	if principal.ActorID != "service:model-gateway" || !principal.HasScope("cost:read") || principal.HasScope("cost:write") {
		t.Fatalf("unexpected service principal: %#v", principal)
	}
}

func TestUnaryInterceptorUsesVerifiedMetadataAndRejectsForgedIdentity(t *testing.T) {
	verifier, key := testVerifier(t)
	interceptor := verifier.UnaryServerInterceptor(nil)
	info := &grpc.UnaryServerInfo{FullMethod: "/model_plane.v1.CapabilityCore/ListCapabilities"}
	handler := func(ctx context.Context, _ any) (any, error) {
		principal, ok := PrincipalFromContext(ctx)
		if !ok || principal.OrganizationID != "org-a" || principal.ActorID != "user-a" {
			t.Fatalf("unexpected principal: %#v, ok=%v", principal, ok)
		}
		return "ok", nil
	}

	if _, err := interceptor(context.Background(), nil, info, handler); status.Code(err) != codes.Unauthenticated {
		t.Fatalf("missing auth code = %v, want unauthenticated", status.Code(err))
	}

	forged := metadata.NewIncomingContext(context.Background(), metadata.Pairs(
		"authorization", "Bearer "+userToken(t, key),
		"x-org-id", "org-b",
	))
	if _, err := interceptor(forged, nil, info, handler); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("forged org code = %v, want permission denied", status.Code(err))
	}

	valid := metadata.NewIncomingContext(context.Background(), metadata.Pairs(
		"authorization", "Bearer "+userToken(t, key),
	))
	response, err := interceptor(valid, nil, info, handler)
	if err != nil || response != "ok" {
		t.Fatalf("valid response=%v err=%v", response, err)
	}
}

func TestNewVerifierRejectsIncompleteOrInvalidTrustConfiguration(t *testing.T) {
	tests := []Config{
		{},
		{Audiences: []string{"model-gateway"}, Issuer: testIssuer},
		{Audiences: []string{"model-gateway"}, Issuer: testIssuer, PublicKeyPEM: []byte("not a key")},
		{Audiences: []string{"model-gateway"}, Issuer: testIssuer, JWKSURL: "file:///tmp/key.json"},
	}
	for index, config := range tests {
		if _, err := NewVerifier(config); err == nil {
			t.Fatalf("config %d unexpectedly succeeded", index)
		}
	}
}

func TestJWKSLoadingAndKeyIDVerification(t *testing.T) {
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	exponent := big.NewInt(int64(key.PublicKey.E)).Bytes()
	document := map[string]any{
		"keys": []map[string]string{{
			"kid": "test-key",
			"kty": "RSA",
			"use": "sig",
			"alg": "RS256",
			"n":   base64.RawURLEncoding.EncodeToString(key.PublicKey.N.Bytes()),
			"e":   base64.RawURLEncoding.EncodeToString(exponent),
		}},
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(document)
	}))
	defer server.Close()

	verifier, err := NewVerifier(Config{
		Audiences: []string{"model-gateway"},
		Issuer:    testIssuer,
		JWKSURL:   server.URL,
	})
	if err != nil {
		t.Fatal(err)
	}
	claims := testClaims{OrgID: "org-a", UserID: "user-a", PrincipalType: "user"}
	claims.Issuer = testIssuer
	claims.Audience = jwt.ClaimStrings{"model-gateway"}
	claims.Subject = "user-a"
	claims.IssuedAt = jwt.NewNumericDate(time.Now().Add(-time.Minute))
	claims.NotBefore = jwt.NewNumericDate(time.Now().Add(-time.Minute))
	claims.ExpiresAt = jwt.NewNumericDate(time.Now().Add(time.Hour))
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	token.Header["kid"] = "test-key"
	raw, err := token.SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := verifier.Verify(raw); err != nil {
		t.Fatalf("JWKS-backed verification failed: %v", err)
	}
}

func TestJWKSLoaderRejectsRedirectStatusMalformedAndUnusableDocuments(t *testing.T) {
	server := httptest.NewServer(nil)
	defer server.Close()
	mux := http.NewServeMux()
	mux.HandleFunc("/redirect", func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "/valid", http.StatusFound)
	})
	mux.HandleFunc("/status", func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, "unavailable", http.StatusServiceUnavailable)
	})
	mux.HandleFunc("/malformed", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("{"))
	})
	mux.HandleFunc("/unusable", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"keys":[{"kid":"bad","kty":"EC","alg":"ES256"}]}`))
	})
	mux.HandleFunc("/valid", func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"keys":[]}`))
	})
	server.Config.Handler = mux

	for _, path := range []string{"/redirect", "/status", "/malformed", "/unusable"} {
		t.Run(path, func(t *testing.T) {
			_, err := NewVerifier(Config{
				Audiences: []string{"model-gateway"},
				Issuer:    testIssuer,
				JWKSURL:   server.URL + path,
			})
			if err == nil {
				t.Fatal("expected JWKS loading failure")
			}
		})
	}
}
