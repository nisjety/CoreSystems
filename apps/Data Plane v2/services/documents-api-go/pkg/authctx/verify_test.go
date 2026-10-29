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
		"user_id":        "user_123",
		"org_id":         "org_abc",
		"aud":            testAudience,
		"iss":            testIssuer,
		"sub":            "user_123",
		"principal_type": "user",
		"zdr":            true,
		"iat":            now.Add(-1 * time.Minute).Unix(),
		"nbf":            now.Add(-1 * time.Minute).Unix(),
		"exp":            now.Add(1 * time.Hour).Unix(),
	}
}

func TestVerifyRequiresBooleanZDRPosture(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)

	missing := validClaims()
	delete(missing, "zdr")
	if _, err := v.Verify(signToken(t, key, missing)); err == nil {
		t.Fatal("token without signed zdr posture was accepted")
	}

	malformed := validClaims()
	malformed["zdr"] = "true"
	if _, err := v.Verify(signToken(t, key, malformed)); err == nil {
		t.Fatal("token with non-boolean zdr posture was accepted")
	}
}

func TestVerifyPreservesSignedZDRPosture(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)

	claims, err := v.Verify(signToken(t, key, validClaims()))
	if err != nil {
		t.Fatalf("verify restrictive token: %v", err)
	}
	if !claims.ZDR {
		t.Fatal("verified restrictive zdr posture was lost")
	}
}

func TestVerifyAcceptsCanonicalScopedServicePrincipal(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)
	c := validClaims()
	delete(c, "user_id")
	c["sub"] = "service:documents-worker"
	c["service_id"] = "service:documents-worker"
	c["principal_type"] = "service"
	c["scopes"] = []string{"documents:read"}

	claims, err := v.Verify(signToken(t, key, c))
	if err != nil {
		t.Fatalf("expected valid service principal, got %v", err)
	}
	if !claims.IsService() || claims.ServiceID != "service:documents-worker" {
		t.Fatalf("unexpected service claims: %+v", claims)
	}
}

func TestVerifyRejectsMixedUserAndServiceIdentity(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)
	c := validClaims()
	c["service_id"] = "service:documents-worker"
	c["principal_type"] = "service"
	if _, err := v.Verify(signToken(t, key, c)); err == nil {
		t.Fatal("expected mixed user/service identity to be rejected")
	}
}

func TestVerifyRejectsServicePrincipalWithoutUsableScope(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)
	c := validClaims()
	delete(c, "user_id")
	c["sub"] = "service:documents-worker"
	c["service_id"] = "service:documents-worker"
	c["principal_type"] = "service"
	c["scopes"] = []string{"  "}

	if _, err := v.Verify(signToken(t, key, c)); err == nil {
		t.Fatal("expected service principal without a usable scope to be rejected")
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

func TestVerifyRejectsUnsignedToken(t *testing.T) {
	writeTestKey(t)
	v := newTestVerifier(t)

	unsigned := jwt.NewWithClaims(jwt.SigningMethodNone, validClaims())
	token, err := unsigned.SignedString(jwt.UnsafeAllowNoneSignatureType)
	if err != nil {
		t.Fatalf("sign unsigned token: %v", err)
	}
	if _, err := v.Verify(token); err == nil {
		t.Fatal("expected alg=none token to be rejected")
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

func TestVerifyRejectsMissingExpiration(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)
	c := validClaims()
	delete(c, "exp")
	if _, err := v.Verify(signToken(t, key, c)); err == nil {
		t.Fatal("expected token without exp to be rejected")
	}
}

func TestVerifyRejectsMissingOrAmbiguousIdentity(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)

	for _, mutate := range []func(jwt.MapClaims){
		func(c jwt.MapClaims) { delete(c, "user_id") },
		func(c jwt.MapClaims) { c["sub"] = "different-user" },
	} {
		c := validClaims()
		mutate(c)
		if _, err := v.Verify(signToken(t, key, c)); err == nil {
			t.Fatal("expected identity-less or ambiguous token to be rejected")
		}
	}
}

func TestVerifyRejectsMissingOrMalformedStandardAndIdentityClaims(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)
	tests := []struct {
		name   string
		mutate func(jwt.MapClaims)
	}{
		{name: "missing audience", mutate: func(c jwt.MapClaims) { delete(c, "aud") }},
		{name: "missing issuer", mutate: func(c jwt.MapClaims) { delete(c, "iss") }},
		{name: "missing subject", mutate: func(c jwt.MapClaims) { delete(c, "sub") }},
		{name: "missing issued at", mutate: func(c jwt.MapClaims) { delete(c, "iat") }},
		{name: "missing not before", mutate: func(c jwt.MapClaims) { delete(c, "nbf") }},
		{name: "missing principal type", mutate: func(c jwt.MapClaims) { delete(c, "principal_type") }},
		{name: "future issued at", mutate: func(c jwt.MapClaims) { c["iat"] = time.Now().Add(5 * time.Minute).Unix() }},
		{name: "future not before", mutate: func(c jwt.MapClaims) { c["nbf"] = time.Now().Add(5 * time.Minute).Unix() }},
		{name: "malformed expiration", mutate: func(c jwt.MapClaims) { c["exp"] = "not-a-timestamp" }},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			claims := validClaims()
			tc.mutate(claims)
			if _, err := v.Verify(signToken(t, key, claims)); err == nil {
				t.Fatal("malformed token was accepted")
			}
		})
	}
}

func TestVerifyRejectsNonRS256AndMalformedTokens(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)
	token := jwt.NewWithClaims(jwt.SigningMethodRS384, validClaims())
	signed, err := token.SignedString(key)
	if err != nil {
		t.Fatalf("sign RS384 token: %v", err)
	}
	for _, candidate := range []string{signed, "not-a-jwt"} {
		if _, err := v.Verify(candidate); err == nil {
			t.Fatal("non-RS256 or malformed token was accepted")
		}
	}
}

func TestVerifyAcceptsAudienceArrayAndExpirationInsideLeeway(t *testing.T) {
	key := writeTestKey(t)
	v := newTestVerifier(t)
	c := validClaims()
	c["aud"] = []string{"model-plane", testAudience}
	c["exp"] = time.Now().Add(-10 * time.Second).Unix()
	claims, err := v.Verify(signToken(t, key, c))
	if err != nil {
		t.Fatalf("expected standards-compliant audience array and expiration leeway: %v", err)
	}
	if !claims.Verified {
		t.Fatal("verified token was not marked verified")
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

func TestEnforcementDefaultsOn(t *testing.T) {
	t.Setenv("AUTHCTX_ENFORCE", "")
	if !((&Config{}).resolveEnforce()) {
		t.Fatal("JWT verification must default to enforced")
	}
}

func TestValidateRejectsMalformedJWKSURL(t *testing.T) {
	t.Setenv("JWT_PUBLIC_KEY_FILE", "")
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_JWKS_URL", "")
	enforce := true
	err := Validate(Config{
		Audience:       testAudience,
		ExpectedIssuer: testIssuer,
		JWKSURL:        "://not-a-valid-jwks-url",
		Enforce:        &enforce,
	})
	if err == nil {
		t.Fatal("production startup must reject a malformed JWKS URL")
	}
}

func TestRequireServiceScopePreservesUsersAndBoundsServices(t *testing.T) {
	next := RequireServiceScope("documents:read")(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	}))
	tests := []struct {
		name   string
		claims *Claims
		want   int
	}{
		{name: "user", claims: &Claims{UserID: "user-1", PrincipalType: "user", Verified: true}, want: http.StatusNoContent},
		{name: "scoped service", claims: &Claims{ServiceID: "service:reader", PrincipalType: "service", Scopes: []string{"documents:read"}, Verified: true}, want: http.StatusNoContent},
		{name: "wrong service scope", claims: &Claims{ServiceID: "service:graph", PrincipalType: "service", Scopes: []string{"graph:read"}, Verified: true}, want: http.StatusForbidden},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodGet, "/v1/documents", nil)
			req = req.WithContext(IntoContext(req.Context(), tc.claims))
			response := httptest.NewRecorder()
			next.ServeHTTP(response, req)
			if response.Code != tc.want {
				t.Fatalf("status = %d, want %d", response.Code, tc.want)
			}
		})
	}
}
