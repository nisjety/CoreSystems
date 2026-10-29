package auth

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
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

const (
	testIssuer   = "https://auth.test/api/convex-auth"
	testAudience = "ingestion"
)

func TestMiddlewareRejectsMissingBearer(t *testing.T) {
	middleware, _, _ := newTestMiddleware(t)

	response := serve(middleware(noContentHandler()), http.MethodGet, "/api/carriers", nil)

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusUnauthorized)
	}
}

func TestMiddlewareRejectsWrongAudience(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, map[string]any{
		"aud": "data-plane",
	})

	response := serve(middleware(noContentHandler()), http.MethodGet, "/api/carriers", map[string]string{
		"Authorization": "Bearer " + token,
	})

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusUnauthorized)
	}
}

func TestMiddlewareRejectsConflictingTenantHeader(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, nil)

	response := serve(middleware(noContentHandler()), http.MethodGet, "/api/carriers", map[string]string{
		"Authorization": "Bearer " + token,
		"X-Org-ID":      "org-other",
	})

	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusForbidden)
	}
}

func TestMiddlewareRejectsConflictingActorHeader(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, nil)

	response := serve(middleware(noContentHandler()), http.MethodGet, "/api/carriers", map[string]string{
		"Authorization": "Bearer " + token,
		"X-User-ID":     "user-other",
	})

	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusForbidden)
	}
}

func TestMiddlewareRejectsExpiredToken(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, map[string]any{
		"exp": time.Now().Add(-2 * time.Minute).Unix(),
	})

	response := serve(middleware(noContentHandler()), http.MethodGet, "/api/carriers", map[string]string{
		"Authorization": "Bearer " + token,
	})

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusUnauthorized)
	}
}

func TestMiddlewareRejectsMissingOrganization(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, map[string]any{"org_id": nil})

	response := serve(middleware(noContentHandler()), http.MethodGet, "/api/carriers", map[string]string{
		"Authorization": "Bearer " + token,
	})

	if response.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusUnauthorized)
	}
}

func TestMiddlewareUsesVerifiedIdentity(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, nil)

	next := middleware(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		principal, ok := PrincipalFromContext(r.Context())
		if !ok {
			t.Fatal("verified principal missing from request context")
		}
		if principal.OrganizationID != "org-test" {
			t.Fatalf("organization = %q, want org-test", principal.OrganizationID)
		}
		if principal.ActorID != "user-test" {
			t.Fatalf("actor = %q, want user-test", principal.ActorID)
		}
		if got := r.Header.Get("X-Org-ID"); got != "org-test" {
			t.Fatalf("restamped X-Org-ID = %q, want org-test", got)
		}
		if got := r.Header.Get("X-User-ID"); got != "user-test" {
			t.Fatalf("restamped X-User-ID = %q, want user-test", got)
		}
		w.WriteHeader(http.StatusNoContent)
	}))

	response := serve(next, http.MethodGet, "/api/carriers", map[string]string{
		"Authorization": "Bearer " + token,
	})

	if response.Code != http.StatusNoContent {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusNoContent)
	}
}

func TestMiddlewareRequiresExplicitServiceScope(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, map[string]any{
		"sub":            "service:model-execution",
		"user_id":        nil,
		"service_id":     "service:model-execution",
		"principal_type": "service",
		"scopes":         []string{"documents:read"},
	})

	response := serve(middleware(noContentHandler()), http.MethodPost, "/api/bookings", map[string]string{
		"Authorization": "Bearer " + token,
	})

	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusForbidden)
	}
}

func TestMiddlewareAcceptsScopedServicePrincipal(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, map[string]any{
		"sub":            "service:model-execution",
		"user_id":        nil,
		"service_id":     "service:model-execution",
		"principal_type": "service",
		"scopes":         []string{"shipping:write"},
	})

	response := serve(middleware(noContentHandler()), http.MethodPost, "/api/bookings", map[string]string{
		"Authorization": "Bearer " + token,
	})

	if response.Code != http.StatusNoContent {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusNoContent)
	}
}

func TestMiddlewareAcceptsReadScopedServicePrincipalForQuotes(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, map[string]any{
		"sub":            "service:execution-core",
		"user_id":        nil,
		"service_id":     "service:execution-core",
		"principal_type": "service",
		"scopes":         []string{"shipping:read"},
	})

	for _, path := range []string{"/api/quotes", "/api/quotes/recommend"} {
		response := serve(middleware(noContentHandler()), http.MethodPost, path, map[string]string{
			"Authorization": "Bearer " + token,
		})
		if response.Code != http.StatusNoContent {
			t.Errorf("path %s: status = %d, want %d (a read-scoped principal must reach a read-shaped POST route)", path, response.Code, http.StatusNoContent)
		}
	}
}

func TestMiddlewareRejectsReadScopedServicePrincipalForBookings(t *testing.T) {
	middleware, privateKey, keyID := newTestMiddleware(t)
	token := signTestToken(t, privateKey, keyID, map[string]any{
		"sub":            "service:execution-core",
		"user_id":        nil,
		"service_id":     "service:execution-core",
		"principal_type": "service",
		"scopes":         []string{"shipping:read"},
	})

	response := serve(middleware(noContentHandler()), http.MethodPost, "/api/bookings", map[string]string{
		"Authorization": "Bearer " + token,
	})

	if response.Code != http.StatusForbidden {
		t.Fatalf("status = %d, want %d (shipping:read must never be enough to create a booking)", response.Code, http.StatusForbidden)
	}
}

func TestNewMiddlewareRejectsMissingVerificationMaterial(t *testing.T) {
	_, err := NewMiddleware(Config{Audience: testAudience, Issuer: testIssuer})
	if err == nil {
		t.Fatal("expected missing verification material to fail closed")
	}
}

func TestNewMiddlewareLoadsAuthCoreJWKS(t *testing.T) {
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate RSA key: %v", err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"keys": []map[string]any{{
			"kid": "jwks-key",
			"kty": "RSA",
			"use": "sig",
			"alg": "RS256",
			"n":   base64.RawURLEncoding.EncodeToString(privateKey.PublicKey.N.Bytes()),
			"e":   base64.RawURLEncoding.EncodeToString(big.NewInt(int64(privateKey.PublicKey.E)).Bytes()),
		}}})
	}))
	defer server.Close()

	middleware, err := NewMiddleware(Config{
		Audience: testAudience,
		Issuer:   testIssuer,
		JWKSURL:  server.URL,
	})
	if err != nil {
		t.Fatalf("build middleware from JWKS: %v", err)
	}
	token := signTestToken(t, privateKey, "jwks-key", nil)
	response := serve(middleware(noContentHandler()), http.MethodGet, "/api/carriers", map[string]string{
		"Authorization": "Bearer " + token,
	})
	if response.Code != http.StatusNoContent {
		t.Fatalf("status = %d, want %d", response.Code, http.StatusNoContent)
	}
}

func TestNewMiddlewareRejectsUnsafeOrEmptyJWKS(t *testing.T) {
	if _, err := NewMiddleware(Config{Audience: testAudience, Issuer: testIssuer, JWKSURL: "file:///tmp/jwks"}); err == nil {
		t.Fatal("expected non-http JWKS URL to be rejected")
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"keys":[]}`))
	}))
	defer server.Close()
	if _, err := NewMiddleware(Config{Audience: testAudience, Issuer: testIssuer, JWKSURL: server.URL}); err == nil {
		t.Fatal("expected JWKS without usable keys to be rejected")
	}
}

func TestConfigFromEnvLoadsStaticPublicKey(t *testing.T) {
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate RSA key: %v", err)
	}
	publicDER, err := x509.MarshalPKIXPublicKey(&privateKey.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	path := filepath.Join(t.TempDir(), "public.pem")
	if err := os.WriteFile(path, pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: publicDER}), 0o600); err != nil {
		t.Fatalf("write public key: %v", err)
	}
	t.Setenv("JWT_PUBLIC_KEY_FILE", path)
	t.Setenv("INGESTION_AUTH_AUDIENCE", "ingestion-test")
	t.Setenv("PLANE_TOKEN_ISSUER", "https://issuer.test")
	t.Setenv("AUTH_CORE_JWKS_URL", "")

	config, err := ConfigFromEnv()
	if err != nil {
		t.Fatalf("ConfigFromEnv: %v", err)
	}
	if config.Audience != "ingestion-test" || config.Issuer != "https://issuer.test" || len(config.PublicKeyPEM) == 0 {
		t.Fatalf("unexpected config: audience=%q issuer=%q public_key=%d bytes", config.Audience, config.Issuer, len(config.PublicKeyPEM))
	}
}

func newTestMiddleware(t *testing.T) (func(http.Handler) http.Handler, *rsa.PrivateKey, string) {
	t.Helper()
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate RSA key: %v", err)
	}
	publicDER, err := x509.MarshalPKIXPublicKey(&privateKey.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	publicPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: publicDER})
	middleware, err := NewMiddleware(Config{
		Audience:     testAudience,
		Issuer:       testIssuer,
		PublicKeyPEM: publicPEM,
	})
	if err != nil {
		t.Fatalf("build middleware: %v", err)
	}
	return middleware, privateKey, "test-key"
}

func signTestToken(t *testing.T, privateKey *rsa.PrivateKey, keyID string, overrides map[string]any) string {
	t.Helper()
	now := time.Now().UTC()
	claims := jwt.MapClaims{
		"iss":            testIssuer,
		"aud":            testAudience,
		"sub":            "user-test",
		"iat":            now.Unix(),
		"nbf":            now.Add(-time.Second).Unix(),
		"exp":            now.Add(5 * time.Minute).Unix(),
		"org_id":         "org-test",
		"user_id":        "user-test",
		"principal_type": "user",
		"scopes":         []string{"documents:read"},
	}
	for key, value := range overrides {
		if value == nil {
			delete(claims, key)
			continue
		}
		claims[key] = value
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	token.Header["kid"] = keyID
	signed, err := token.SignedString(privateKey)
	if err != nil {
		t.Fatalf("sign token: %v", err)
	}
	return signed
}

func serve(handler http.Handler, method, path string, headers map[string]string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, nil)
	for key, value := range headers {
		request.Header.Set(key, value)
	}
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}

func noContentHandler() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	})
}
