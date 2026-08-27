package authctx

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"math/big"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

const (
	testAudience = "data-plane"
	testIssuer   = "https://auth.test/api/convex-auth"
	testOrgID    = "org_authorized"
	testUserID   = "user_authorized"
)

func TestMiddlewareFourShapeTenantMatrix(t *testing.T) {
	key, publicPEM := testKey(t)
	verifier, err := NewVerifier(Config{
		Audience:     testAudience,
		Issuer:       testIssuer,
		PublicKeyPEM: publicPEM,
	})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}

	token := signToken(t, key, validClaims())
	next := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		claims, ok := FromContext(r.Context())
		if !ok {
			http.Error(w, "missing verified claims", http.StatusInternalServerError)
			return
		}
		authorization, ok := AuthorizationHeader(r.Context())
		if !ok || authorization != "Bearer "+token {
			http.Error(w, "missing forwarded authorization", http.StatusInternalServerError)
			return
		}
		w.Header().Set("X-Seen-Org-ID", claims.OrgID)
		w.WriteHeader(http.StatusNoContent)
	})
	handler := Middleware(verifier)(next)

	tests := []struct {
		name       string
		bearer     string
		headerOrg  string
		wantStatus int
		wantOrg    string
	}{
		{name: "no authentication", wantStatus: http.StatusUnauthorized},
		{name: "forged organization header", headerOrg: "org_victim", wantStatus: http.StatusUnauthorized},
		{name: "valid verified identity", bearer: token, wantStatus: http.StatusNoContent, wantOrg: testOrgID},
		{name: "valid identity plus spoofed organization", bearer: token, headerOrg: "org_victim", wantStatus: http.StatusForbidden},
		{name: "valid identity plus matching organization", bearer: token, headerOrg: testOrgID, wantStatus: http.StatusNoContent, wantOrg: testOrgID},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodGet, "/protected", nil)
			if tt.bearer != "" {
				req.Header.Set("Authorization", "Bearer "+tt.bearer)
			}
			if tt.headerOrg != "" {
				req.Header.Set("X-Org-ID", tt.headerOrg)
			}
			response := httptest.NewRecorder()

			handler.ServeHTTP(response, req)

			if response.Code != tt.wantStatus {
				t.Fatalf("status = %d, want %d; body=%s", response.Code, tt.wantStatus, response.Body.String())
			}
			if got := response.Header().Get("X-Seen-Org-ID"); got != tt.wantOrg {
				t.Fatalf("seen org = %q, want %q", got, tt.wantOrg)
			}
		})
	}
}

func TestVerifierRejectsInvalidIdentityTokens(t *testing.T) {
	key, publicPEM := testKey(t)
	verifier, err := NewVerifier(Config{
		Audience:     testAudience,
		Issuer:       testIssuer,
		PublicKeyPEM: publicPEM,
	})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}

	tests := []struct {
		name   string
		claims jwt.MapClaims
	}{
		{name: "wrong audience", claims: mergeClaims(validClaims(), jwt.MapClaims{"aud": "model-gateway"})},
		{name: "wrong issuer", claims: mergeClaims(validClaims(), jwt.MapClaims{"iss": "https://evil.test"})},
		{name: "expired", claims: mergeClaims(validClaims(), jwt.MapClaims{"exp": time.Now().Add(-5 * time.Minute).Unix()})},
		{name: "missing issued-at", claims: withoutClaim(validClaims(), "iat")},
		{name: "missing not-before", claims: withoutClaim(validClaims(), "nbf")},
		{name: "missing organization", claims: withoutClaim(validClaims(), "org_id")},
		{name: "missing user", claims: withoutClaim(validClaims(), "user_id")},
		{name: "ambiguous subject", claims: mergeClaims(validClaims(), jwt.MapClaims{"sub": "different-user"})},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := verifier.Verify(signToken(t, key, tt.claims)); err == nil {
				t.Fatal("expected token to be rejected")
			}
		})
	}

	unsigned := jwt.NewWithClaims(jwt.SigningMethodNone, validClaims())
	unsignedToken, err := unsigned.SignedString(jwt.UnsafeAllowNoneSignatureType)
	if err != nil {
		t.Fatalf("sign unsigned token: %v", err)
	}
	if _, err := verifier.Verify(unsignedToken); err == nil {
		t.Fatal("expected unsigned token to be rejected")
	}

	wrongKey, _ := testKey(t)
	if _, err := verifier.Verify(signToken(t, wrongKey, validClaims())); err == nil {
		t.Fatal("expected self-signed token to be rejected")
	}
}

func TestVerifierFailsClosedWithoutVerificationMaterial(t *testing.T) {
	if _, err := NewVerifier(Config{Audience: testAudience, Issuer: testIssuer}); err == nil {
		t.Fatal("expected missing verification material to fail construction")
	}
}

func TestVerifierUsesJWKSAndCachesKey(t *testing.T) {
	key, _ := testKey(t)
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		requests.Add(1)
		writeJWKS(t, w, "test-key", &key.PublicKey)
	}))
	t.Cleanup(server.Close)

	verifier, err := NewVerifier(Config{Audience: testAudience, Issuer: testIssuer, JWKSURL: server.URL})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	token := signTokenWithKid(t, key, "test-key", validClaims())
	for range 2 {
		claims, err := verifier.Verify(token)
		if err != nil || claims.OrgID != testOrgID {
			t.Fatalf("Verify with JWKS = (%+v, %v)", claims, err)
		}
	}
	if got := requests.Load(); got != 1 {
		t.Fatalf("JWKS requests = %d, want 1", got)
	}
}

func TestMiddlewareFailsClosedWhenJWKSIsUnavailable(t *testing.T) {
	key, _ := testKey(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, "unavailable", http.StatusServiceUnavailable)
	}))
	t.Cleanup(server.Close)

	verifier, err := NewVerifier(Config{Audience: testAudience, Issuer: testIssuer, JWKSURL: server.URL})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	req := httptest.NewRequest(http.MethodGet, "/protected", nil)
	req.Header.Set("Authorization", "Bearer "+signTokenWithKid(t, key, "unavailable-key", validClaims()))
	response := httptest.NewRecorder()

	Middleware(verifier)(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Fatal("unavailable JWKS must not reach the handler")
	})).ServeHTTP(response, req)

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want 401", response.Code)
	}
}

func TestVerifierRejectsMissingOrUnknownJWKSKeyID(t *testing.T) {
	key, _ := testKey(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		writeJWKS(t, w, "known-key", &key.PublicKey)
	}))
	t.Cleanup(server.Close)
	verifier, err := NewVerifier(Config{Audience: testAudience, Issuer: testIssuer, JWKSURL: server.URL})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}

	if _, err := verifier.Verify(signToken(t, key, validClaims())); err == nil {
		t.Fatal("expected token without kid to be rejected")
	}
	unknown := signTokenWithKid(t, key, "unknown-key", validClaims())
	if _, err := verifier.Verify(unknown); err == nil {
		t.Fatal("expected unknown kid to be rejected")
	}
	if _, err := verifier.Verify(unknown); err == nil {
		t.Fatal("expected cached unknown kid to remain rejected")
	}
}

func TestVerifierLoadsStaticPublicKeyFile(t *testing.T) {
	key, publicPEM := testKey(t)
	path := filepath.Join(t.TempDir(), "data-plane.pub")
	if err := os.WriteFile(path, publicPEM, 0o600); err != nil {
		t.Fatalf("write public key: %v", err)
	}
	verifier, err := NewVerifier(Config{Audience: testAudience, Issuer: testIssuer, PublicKeyFile: path})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	if _, err := verifier.Verify(signToken(t, key, validClaims())); err != nil {
		t.Fatalf("Verify: %v", err)
	}
}

func TestNewVerifierRejectsMalformedConfiguration(t *testing.T) {
	_, publicPEM := testKey(t)
	tests := []struct {
		name string
		cfg  Config
	}{
		{name: "missing audience", cfg: Config{Issuer: testIssuer, PublicKeyPEM: publicPEM}},
		{name: "missing issuer", cfg: Config{Audience: testAudience, PublicKeyPEM: publicPEM}},
		{name: "unreadable public key", cfg: Config{Audience: testAudience, Issuer: testIssuer, PublicKeyFile: "/does/not/exist"}},
		{name: "malformed public key", cfg: Config{Audience: testAudience, Issuer: testIssuer, PublicKeyPEM: []byte("not a PEM")}},
		{name: "malformed JWKS URL", cfg: Config{Audience: testAudience, Issuer: testIssuer, JWKSURL: "://not-a-url"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := NewVerifier(tt.cfg); err == nil {
				t.Fatal("expected verifier construction to fail")
			}
		})
	}
}

func testKey(t *testing.T) (*rsa.PrivateKey, []byte) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate RSA key: %v", err)
	}
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	return key, pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der})
}

func validClaims() jwt.MapClaims {
	now := time.Now()
	return jwt.MapClaims{
		"iss":     testIssuer,
		"aud":     testAudience,
		"sub":     testUserID,
		"org_id":  testOrgID,
		"user_id": testUserID,
		"iat":     now.Unix(),
		"nbf":     now.Add(-time.Second).Unix(),
		"exp":     now.Add(5 * time.Minute).Unix(),
	}
}

func signToken(t *testing.T, key *rsa.PrivateKey, claims jwt.MapClaims) string {
	t.Helper()
	token, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatalf("sign token: %v", err)
	}
	return token
}

func signTokenWithKid(t *testing.T, key *rsa.PrivateKey, kid string, claims jwt.MapClaims) string {
	t.Helper()
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	token.Header["kid"] = kid
	signed, err := token.SignedString(key)
	if err != nil {
		t.Fatalf("sign token: %v", err)
	}
	return signed
}

func writeJWKS(t *testing.T, w http.ResponseWriter, kid string, key *rsa.PublicKey) {
	t.Helper()
	exponent := base64.RawURLEncoding.EncodeToString(big.NewInt(int64(key.E)).Bytes())
	modulus := base64.RawURLEncoding.EncodeToString(key.N.Bytes())
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(map[string]any{"keys": []map[string]string{{
		"kid": kid,
		"kty": "RSA",
		"use": "sig",
		"alg": "RS256",
		"n":   modulus,
		"e":   exponent,
	}}}); err != nil {
		t.Errorf("encode JWKS: %v", err)
	}
}

func mergeClaims(base, overrides jwt.MapClaims) jwt.MapClaims {
	merged := make(jwt.MapClaims, len(base)+len(overrides))
	for key, value := range base {
		merged[key] = value
	}
	for key, value := range overrides {
		merged[key] = value
	}
	return merged
}

func withoutClaim(base jwt.MapClaims, key string) jwt.MapClaims {
	result := mergeClaims(base, nil)
	delete(result, key)
	return result
}

// A real auth-core service-principal token must verify.
//
// Regression: this verifier used to require `user_id == sub` unconditionally,
// and auth-core's `issuePlaneToken` emits `sub` + `service_id` +
// `principal_type: service` with NO `user_id` for service principals. Every
// service token therefore got a silent 401 "invalid credentials", which made
// this service's own evaluation harness unreachable by any
// automated caller — `eval_golden_judgments` and `quality_eval_runs` sat at
// zero rows while the golden set was scored out-of-band by a local script.
func TestVerifierAcceptsServicePrincipalTokens(t *testing.T) {
	key, publicPEM := testKey(t)
	verifier, err := NewVerifier(Config{
		Audience:     testAudience,
		Issuer:       testIssuer,
		PublicKeyPEM: publicPEM,
	})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}

	claims, err := verifier.Verify(signToken(t, key, serviceClaims()))
	if err != nil {
		t.Fatalf("service token must verify: %v", err)
	}
	if !claims.IsService() {
		t.Fatal("expected IsService() to report a service principal")
	}
	if got := claims.PrincipalID(); got != testServiceID {
		t.Fatalf("PrincipalID() = %q, want %q", got, testServiceID)
	}
	if claims.UserID != "" {
		t.Fatalf("service token must not carry a user identity, got %q", claims.UserID)
	}
}

// The service branch is STRICTER than the old user-only check, not looser: an
// internally inconsistent service identity must still fail closed.
func TestVerifierRejectsMalformedServiceIdentities(t *testing.T) {
	key, publicPEM := testKey(t)
	verifier, err := NewVerifier(Config{
		Audience:     testAudience,
		Issuer:       testIssuer,
		PublicKeyPEM: publicPEM,
	})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}

	tests := []struct {
		name   string
		claims jwt.MapClaims
	}{
		{name: "service_id without principal_type", claims: withoutClaim(serviceClaims(), "principal_type")},
		{name: "principal_type without service_id", claims: withoutClaim(serviceClaims(), "service_id")},
		{name: "sub does not match service_id", claims: mergeClaims(serviceClaims(), jwt.MapClaims{"sub": "service:other"})},
		{name: "carries both identities", claims: mergeClaims(serviceClaims(), jwt.MapClaims{"user_id": testUserID})},
		{name: "no scopes", claims: withoutClaim(serviceClaims(), "scopes")},
		{name: "user_id with service principal_type", claims: mergeClaims(validClaims(), jwt.MapClaims{"principal_type": "service"})},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := verifier.Verify(signToken(t, key, tt.claims)); err == nil {
				t.Fatal("expected malformed service identity to be rejected")
			}
		})
	}
}

const testServiceID = "service:corpus-seeder"

// serviceClaims mirrors the exact shape auth-core mints for a service
// principal: sub == service_id, principal_type "service", no user_id.
func serviceClaims() jwt.MapClaims {
	now := time.Now()
	return jwt.MapClaims{
		"iss":            testIssuer,
		"aud":            testAudience,
		"sub":            testServiceID,
		"org_id":         testOrgID,
		"service_id":     testServiceID,
		"principal_type": "service",
		"scopes":         []string{"data:read"},
		"iat":            now.Unix(),
		"nbf":            now.Add(-time.Second).Unix(),
		"exp":            now.Add(5 * time.Minute).Unix(),
	}
}
