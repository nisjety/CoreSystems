package authctx

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

const (
	testAudience = "data-plane"
	testIssuer   = "http://localhost:3011/api/convex-auth"
)

// writeTestKey mints an RSA keypair, writes the PUBLIC key to a temp PEM file,
// points JWT_PUBLIC_KEY_FILE at it, and returns the private key for signing.
func writeTestKey(t *testing.T) *rsa.PrivateKey {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate key: %v", err)
	}
	pubDER, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	pubPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: pubDER})
	path := filepath.Join(t.TempDir(), "convex-auth.pub")
	if err := os.WriteFile(path, pubPEM, 0o600); err != nil {
		t.Fatalf("write pub key: %v", err)
	}
	t.Setenv("JWT_PUBLIC_KEY_FILE", path)
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_JWKS_URL", "")
	return key
}

func signToken(t *testing.T, key *rsa.PrivateKey, claims jwt.MapClaims) string {
	t.Helper()
	tok := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	signed, err := tok.SignedString(key)
	if err != nil {
		t.Fatalf("sign token: %v", err)
	}
	return signed
}

func validClaims() jwt.MapClaims {
	now := time.Now()
	return jwt.MapClaims{
		"user_id": "user_123",
		"org_id":  "org_abc",
		"aud":     testAudience,
		"iss":     testIssuer,
		"sub":     "user_123",
		"iat":     now.Add(-1 * time.Minute).Unix(),
		"exp":     now.Add(1 * time.Hour).Unix(),
	}
}

func newTestVerifier(t *testing.T) *verifier {
	t.Helper()
	v, err := newVerifier(Config{Audience: testAudience, ExpectedIssuer: testIssuer})
	if err != nil {
		t.Fatalf("newVerifier: %v", err)
	}
	return v
}

func TestVerifyAcceptsValidToken(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)

	claims, err := v.Verify(signToken(t, key, validClaims()))
	if err != nil {
		t.Fatalf("expected valid token, got error: %v", err)
	}
	if !claims.Verified || claims.OrgID != "org_abc" || claims.UserID != "user_123" {
		t.Fatalf("unexpected claims: %+v", claims)
	}
}

func TestVerifyRejectsWrongAudience(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)

	c := validClaims()
	c["aud"] = "model-plane"
	if _, err := v.Verify(signToken(t, key, c)); err == nil {
		t.Fatal("expected audience mismatch to be rejected")
	}
}

func TestVerifyRejectsWrongIssuer(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)

	c := validClaims()
	c["iss"] = "http://evil.example/api"
	if _, err := v.Verify(signToken(t, key, c)); err == nil {
		t.Fatal("expected issuer mismatch to be rejected")
	}
}

func TestVerifyRejectsExpiredToken(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)

	c := validClaims()
	c["exp"] = time.Now().Add(-5 * time.Minute).Unix()
	if _, err := v.Verify(signToken(t, key, c)); err == nil {
		t.Fatal("expected expired token to be rejected")
	}
}

func TestVerifyRejectsWrongKey(t *testing.T) {
	writeTestKey(t) // sets JWT_PUBLIC_KEY_FILE to key A's public part
	v := newTestVerifier(t)

	// Sign with a DIFFERENT private key — signature must not verify.
	other, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate other key: %v", err)
	}
	if _, err := v.Verify(signToken(t, other, validClaims())); err == nil {
		t.Fatal("expected token signed by a different key to be rejected")
	}
}

func TestVerifyRejectsMissingOrgID(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)

	c := validClaims()
	delete(c, "org_id")
	if _, err := v.Verify(signToken(t, key, c)); err == nil {
		t.Fatal("expected token without org_id to be rejected")
	}
}

// TestMiddlewareEnforceRejectsUnverified proves the guarded HTTP surface fails
// closed: no token → 401, cross-tenant header → 403, valid token → 200.
func TestMiddlewareEnforceRejectsUnverified(t *testing.T) {
	key := writeTestKey(t)
	enforce := true
	mw := Middleware(Config{Audience: testAudience, ExpectedIssuer: testIssuer, Enforce: &enforce})

	ok := http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusOK) })
	handler := mw(ok)

	// No token → 401.
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, httptest.NewRequest(http.MethodGet, "/v1/documents", nil))
	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("no token: got %d, want 401", rec.Code)
	}

	// Valid token, matching (or absent) header → 200.
	token := signToken(t, key, validClaims())
	rec = httptest.NewRecorder()
	req := httptest.NewRequest(http.MethodGet, "/v1/documents", nil)
	req.Header.Set("Authorization", "Bearer "+token)
	handler.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("valid token: got %d, want 200", rec.Code)
	}

	// Valid token but a conflicting X-Org-ID → 403 (cross-tenant guard).
	rec = httptest.NewRecorder()
	req = httptest.NewRequest(http.MethodGet, "/v1/documents", nil)
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("X-Org-ID", "org_other")
	handler.ServeHTTP(rec, req)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("cross-tenant header: got %d, want 403", rec.Code)
	}
}

// TestMiddlewareEnforceMisconfiguredFailsClosed proves that flipping enforce
// on without any verification key returns 503, not open access.
func TestMiddlewareEnforceMisconfiguredFailsClosed(t *testing.T) {
	t.Setenv("JWT_PUBLIC_KEY_FILE", "")
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_JWKS_URL", "")
	enforce := true
	mw := Middleware(Config{Audience: testAudience, ExpectedIssuer: testIssuer, Enforce: &enforce})
	handler := mw(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusOK) }))

	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, httptest.NewRequest(http.MethodGet, "/v1/documents", nil))
	if rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("misconfigured enforce: got %d, want 503", rec.Code)
	}
}
