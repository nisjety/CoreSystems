package controlplane

import (
	"crypto/rand"
	"crypto/rsa"
	"encoding/base64"
	"encoding/json"
	"math/big"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
	"github.com/golang-jwt/jwt/v5"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/config"
)

func TestAuthClientVerifyTokenUsesAuthCoreJWKSContract(t *testing.T) {
	var jwksCalls atomic.Int32
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		jwksCalls.Add(1)
		if r.URL.Path != "/api/convex-auth/jwks" {
			t.Fatalf("path = %s, want /api/convex-auth/jwks", r.URL.Path)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"keys": []map[string]any{{
			"kid": "test-key",
			"kty": "RSA",
			"use": "sig",
			"alg": "RS256",
			"n":   base64.RawURLEncoding.EncodeToString(privateKey.PublicKey.N.Bytes()),
			"e":   base64.RawURLEncoding.EncodeToString(big.NewInt(int64(privateKey.PublicKey.E)).Bytes()),
		}}})
	}))
	defer server.Close()
	now := time.Now().UTC()
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"iss":            server.URL + "/api/convex-auth",
		"aud":            "ingestion",
		"sub":            "user-1",
		"iat":            now.Unix(),
		"nbf":            now.Add(-time.Second).Unix(),
		"exp":            now.Add(5 * time.Minute).Unix(),
		"org_id":         "org-1",
		"user_id":        "user-1",
		"principal_type": "user",
		"scopes":         []string{"admin"},
	})
	token.Header["kid"] = "test-key"
	signed, err := token.SignedString(privateKey)
	if err != nil {
		t.Fatalf("SignedString: %v", err)
	}

	client := NewAuthClient(testControlPlaneConfig(server.URL), server.Client())
	principal, err := client.VerifyToken(t.Context(), signed)
	if err != nil {
		t.Fatalf("VerifyToken error: %v", err)
	}
	if principal.UserID != "user-1" || principal.OrganizationID != "org-1" || principal.Role != "admin" {
		t.Fatalf("principal = %#v", principal)
	}
	if _, err := client.VerifyToken(t.Context(), signed); err != nil {
		t.Fatalf("second VerifyToken error: %v", err)
	}
	if got := jwksCalls.Load(); got != 1 {
		t.Fatalf("JWKS calls = %d, want 1 cached fetch", got)
	}
}

func TestAuthClientVerifyTokenRefreshesJWKSOnceForRotatedSigningKey(t *testing.T) {
	oldKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("GenerateKey old: %v", err)
	}
	newKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("GenerateKey new: %v", err)
	}
	var jwksCalls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		call := jwksCalls.Add(1)
		keyID, publicKey := "old-key", &oldKey.PublicKey
		if call > 1 {
			keyID, publicKey = "new-key", &newKey.PublicKey
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"keys": []map[string]any{jwksTestKey(keyID, publicKey)}})
	}))
	defer server.Close()

	client := NewAuthClient(testControlPlaneConfig(server.URL), server.Client())
	if _, err := client.VerifyToken(t.Context(), signedAuthToken(t, server.URL, "old-key", oldKey)); err != nil {
		t.Fatalf("VerifyToken old key: %v", err)
	}
	principal, err := client.VerifyToken(t.Context(), signedAuthToken(t, server.URL, "new-key", newKey))
	if err != nil {
		t.Fatalf("VerifyToken rotated key: %v", err)
	}
	if principal.UserID != "user-1" || principal.OrganizationID != "org-1" {
		t.Fatalf("principal = %#v", principal)
	}
	if got := jwksCalls.Load(); got != 2 {
		t.Fatalf("JWKS calls = %d, want initial fetch plus one forced refresh", got)
	}
}

func TestAuthClientVerifyTokenRejectsUnknownKeyAfterOneRefresh(t *testing.T) {
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	var jwksCalls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		jwksCalls.Add(1)
		_ = json.NewEncoder(w).Encode(map[string]any{"keys": []map[string]any{jwksTestKey("known-key", &privateKey.PublicKey)}})
	}))
	defer server.Close()

	client := NewAuthClient(testControlPlaneConfig(server.URL), server.Client())
	_, err = client.VerifyToken(t.Context(), signedAuthToken(t, server.URL, "unknown-key", privateKey))
	if err == nil {
		t.Fatal("unknown signing key was accepted")
	}
	if got := jwksCalls.Load(); got != 2 {
		t.Fatalf("JWKS calls = %d, want initial fetch plus exactly one forced refresh", got)
	}
}

func TestAuthClientVerifyTokenFailsClosedWhenRotationRefreshIsUnavailable(t *testing.T) {
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	var jwksCalls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		if jwksCalls.Add(1) > 1 {
			w.WriteHeader(http.StatusServiceUnavailable)
			return
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"keys": []map[string]any{jwksTestKey("old-key", &privateKey.PublicKey)}})
	}))
	defer server.Close()

	client := NewAuthClient(testControlPlaneConfig(server.URL), server.Client())
	_, err = client.VerifyToken(t.Context(), signedAuthToken(t, server.URL, "new-key", privateKey))
	if err == nil {
		t.Fatal("token was accepted without refreshed verification key")
	}
}

func TestAuthClientVerifyTokenRejectsMissingRequiredIdentityTimeClaims(t *testing.T) {
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"keys": []map[string]any{jwksTestKey("test-key", &privateKey.PublicKey)}})
	}))
	defer server.Close()
	now := time.Now().UTC()
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"iss": server.URL + "/api/convex-auth", "aud": "ingestion", "sub": "user-1",
		"iat": now.Unix(), "exp": now.Add(5 * time.Minute).Unix(),
		"org_id": "org-1", "user_id": "user-1", "principal_type": "user",
	})
	token.Header["kid"] = "test-key"
	signed, err := token.SignedString(privateKey)
	if err != nil {
		t.Fatalf("SignedString: %v", err)
	}
	if _, err := NewAuthClient(testControlPlaneConfig(server.URL), server.Client()).VerifyToken(t.Context(), signed); err == nil {
		t.Fatal("token missing nbf was accepted")
	}
}

func TestProductionInternalOrBearerAcceptsCanonicalServicePrincipalAndRejectsLegacyTenantKey(t *testing.T) {
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"keys": []map[string]any{jwksTestKey("test-key", &privateKey.PublicKey)}})
	}))
	defer server.Close()
	now := time.Now().UTC()
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"iss": server.URL + "/api/convex-auth", "aud": "ingestion", "sub": "service:conversation-core",
		"iat": now.Unix(), "nbf": now.Add(-time.Second).Unix(), "exp": now.Add(5 * time.Minute).Unix(),
		"org_id": "org-1", "service_id": "service:conversation-core", "principal_type": "service",
		"scopes": []string{"integration:read", "integration:write"},
	})
	token.Header["kid"] = "test-key"
	signed, err := token.SignedString(privateKey)
	if err != nil {
		t.Fatalf("SignedString: %v", err)
	}
	principal, err := NewAuthClient(testControlPlaneConfig(server.URL), server.Client()).VerifyToken(t.Context(), signed)
	if err != nil {
		t.Fatalf("VerifyToken: %v", err)
	}
	if principal.PrincipalType != "service" || principal.UserID != "service:conversation-core" || !principal.HasScope("integration:write") {
		t.Fatalf("principal = %#v", principal)
	}

	app := fiber.New()
	app.Post("/tenant-action", auth.InternalOrBearer(auth.Config{
		APIKey:               "legacy-shared-key",
		TokenVerifier:        NewAuthClient(testControlPlaneConfig(server.URL), server.Client()),
		AllowLegacyTenantKey: false,
	}), func(c *fiber.Ctx) error {
		verified, ok := auth.PrincipalFromContext(c)
		if !ok || verified.PrincipalType != "service" || verified.OrganizationID != "org-1" || !verified.HasScope("integration:write") {
			return c.SendStatus(fiber.StatusForbidden)
		}
		return c.SendStatus(fiber.StatusNoContent)
	})

	bearerRequest := httptest.NewRequest(http.MethodPost, "/tenant-action", nil)
	bearerRequest.Header.Set("Authorization", "Bearer "+signed)
	bearerResponse, err := app.Test(bearerRequest)
	if err != nil || bearerResponse.StatusCode != fiber.StatusNoContent {
		t.Fatalf("service bearer response = %v status=%d", err, bearerResponse.StatusCode)
	}

	legacyRequest := httptest.NewRequest(http.MethodPost, "/tenant-action", nil)
	legacyRequest.Header.Set("X-Internal-API-Key", "legacy-shared-key")
	legacyRequest.Header.Set("X-Org-ID", "org-forged")
	legacyResponse, err := app.Test(legacyRequest)
	if err != nil || legacyResponse.StatusCode != fiber.StatusForbidden {
		t.Fatalf("legacy response = %v status=%d, want forbidden", err, legacyResponse.StatusCode)
	}
}

func TestAuthClientJWKSRejectsMalformedOrUnusableDocuments(t *testing.T) {
	tests := []struct {
		name   string
		body   string
		status int
	}{
		{name: "malformed json", body: `{`},
		{name: "no keys", body: `{"keys":[]}`},
		{name: "unsupported key", body: `{"keys":[{"kid":"bad","kty":"EC","use":"enc","alg":"ES256","n":"bad","e":"bad"}]}`},
		{name: "invalid rsa material", body: `{"keys":[{"kid":"bad","kty":"RSA","use":"sig","alg":"RS256","n":"bad","e":"bad"}]}`},
		{name: "upstream unavailable", status: http.StatusServiceUnavailable},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				if tt.status != 0 {
					w.WriteHeader(tt.status)
					return
				}
				_, _ = w.Write([]byte(tt.body))
			}))
			defer server.Close()
			client := NewAuthClient(testControlPlaneConfig(server.URL), server.Client())
			if _, _, err := client.fetchJWKS(t.Context(), false, 0); err == nil {
				t.Fatal("invalid JWKS document was accepted")
			}
		})
	}
}

func TestAuthClientFailsClosedWhenUnconfiguredOrAlgorithmIsUnsupported(t *testing.T) {
	client := NewAuthClient(config.Config{}, nil)
	if _, err := client.VerifyToken(t.Context(), "token"); err == nil {
		t.Fatal("unconfigured verifier accepted token")
	}
	token := jwt.NewWithClaims(jwt.SigningMethodHS256, jwt.MapClaims{"exp": time.Now().Add(time.Minute).Unix()})
	signed, err := token.SignedString([]byte("test-secret"))
	if err != nil {
		t.Fatalf("SignedString: %v", err)
	}
	client.audience = "ingestion"
	client.issuer = "issuer"
	if _, _, err := client.parseToken(signed, nil); err == nil {
		t.Fatal("unsupported signing algorithm was accepted")
	}
}

func TestAuthClientJWKSForcedRefreshReusesNewerGeneration(t *testing.T) {
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("GenerateKey: %v", err)
	}
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		_ = json.NewEncoder(w).Encode(map[string]any{"keys": []map[string]any{jwksTestKey("test-key", &privateKey.PublicKey)}})
	}))
	defer server.Close()
	client := NewAuthClient(testControlPlaneConfig(server.URL), server.Client())
	_, generation, err := client.fetchJWKS(t.Context(), false, 0)
	if err != nil {
		t.Fatalf("initial fetch: %v", err)
	}
	if _, reusedGeneration, err := client.fetchJWKS(t.Context(), true, generation-1); err != nil || reusedGeneration != generation {
		t.Fatalf("generation = %d, err = %v", reusedGeneration, err)
	}
	if got := calls.Load(); got != 1 {
		t.Fatalf("JWKS calls = %d, want newer generation reused", got)
	}
}

func TestJWTClaimHelpersRejectUnsupportedNumericTypes(t *testing.T) {
	if got := numericClaim(jwt.MapClaims{"value": json.Number("42")}, "value"); got != 42 {
		t.Fatalf("json.Number claim = %d", got)
	}
	if got := numericClaim(jwt.MapClaims{"value": "42"}, "value"); got != 0 {
		t.Fatalf("string numeric claim = %d, want rejected", got)
	}
}

func jwksTestKey(keyID string, publicKey *rsa.PublicKey) map[string]any {
	return map[string]any{
		"kid": keyID,
		"kty": "RSA",
		"use": "sig",
		"alg": "RS256",
		"n":   base64.RawURLEncoding.EncodeToString(publicKey.N.Bytes()),
		"e":   base64.RawURLEncoding.EncodeToString(big.NewInt(int64(publicKey.E)).Bytes()),
	}
}

func signedAuthToken(t *testing.T, issuerBaseURL, keyID string, privateKey *rsa.PrivateKey) string {
	t.Helper()
	now := time.Now().UTC()
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"iss":            issuerBaseURL + "/api/convex-auth",
		"aud":            "ingestion",
		"sub":            "user-1",
		"iat":            now.Unix(),
		"nbf":            now.Add(-time.Second).Unix(),
		"exp":            now.Add(5 * time.Minute).Unix(),
		"org_id":         "org-1",
		"user_id":        "user-1",
		"principal_type": "user",
		"scopes":         []string{"admin"},
	})
	token.Header["kid"] = keyID
	signed, err := token.SignedString(privateKey)
	if err != nil {
		t.Fatalf("SignedString: %v", err)
	}
	return signed
}

func TestOrgClientReadsOrgPlanFromOrgCore(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/orgs/org-1" {
			t.Fatalf("path = %s, want /orgs/org-1", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "internal-key" {
			t.Fatalf("internal key = %q, want internal-key", got)
		}
		if got := r.Header.Get("X-User-ID"); got != "user-1" {
			t.Fatalf("user header = %q, want user-1", got)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{
			"id":   "org-1",
			"plan": "advanced",
			"entitlements": []map[string]any{
				{"key": "integrations", "enabled": true},
			},
			"quotas": []map[string]any{
				{"key": "sources", "value": 1, "limit": 10, "reset_period": "month"},
			},
		})
	}))
	defer server.Close()

	cfg := testControlPlaneConfig("http://unused")
	cfg.OrgCoreURL = server.URL
	client := NewOrgClient(cfg, server.Client())
	plan, err := client.GetOrgPlan(t.Context(), "org-1", "user-1")
	if err != nil {
		t.Fatalf("GetOrgPlan error: %v", err)
	}
	if plan.Plan != "pro" || !plan.Entitlements["integrations"] || plan.Quotas["sources"].Limit != 10 {
		t.Fatalf("plan = %#v", plan)
	}
}

func TestBillingClientRecordsUsageThroughBillingCore(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/billing/orgs/org-1/usage" {
			t.Fatalf("path = %s, want /api/v1/billing/orgs/org-1/usage", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "internal-key" {
			t.Fatalf("internal key = %q, want internal-key", got)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["metric"] != "connect_session_created" || body["source"] != "integration-corev2" {
			t.Fatalf("body = %#v", body)
		}
		w.WriteHeader(http.StatusAccepted)
		_ = json.NewEncoder(w).Encode(map[string]string{"status": "usage recorded"})
	}))
	defer server.Close()

	cfg := testControlPlaneConfig("http://unused")
	cfg.BillingCoreURL = server.URL
	client := NewBillingClient(cfg, server.Client())
	if err := client.RecordUsage(t.Context(), "org-1", UsageEvent{Metric: "connect_session_created", Quantity: 1}); err != nil {
		t.Fatalf("RecordUsage error: %v", err)
	}
}

func TestAuditClientRecordsAuditThroughAuditCore(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/v1/audit" {
			t.Fatalf("path = %s, want /v1/audit", r.URL.Path)
		}
		if got := r.Header.Get("X-Internal-Api-Key"); got != "internal-key" {
			t.Fatalf("internal key = %q, want internal-key", got)
		}
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatalf("Decode body error: %v", err)
		}
		if body["org_id"] != "org-1" || body["plane"] != "integration-corev2" || body["event"] != "connection.action.executed" {
			t.Fatalf("body = %#v", body)
		}
		details, ok := body["details"].(map[string]any)
		if !ok || details["providerKey"] != "slack" {
			t.Fatalf("details = %#v, want provider key", body["details"])
		}
		w.WriteHeader(http.StatusAccepted)
	}))
	defer server.Close()

	cfg := testControlPlaneConfig("http://unused")
	cfg.AuditCoreURL = server.URL
	client := NewAuditClient(cfg, server.Client())
	if err := client.RecordAudit(t.Context(), AuditEvent{
		OrgID:      "org-1",
		UserID:     "user-1",
		Event:      "connection.action.executed",
		ResourceID: "conn-1",
		Details:    map[string]any{"providerKey": "slack"},
	}); err != nil {
		t.Fatalf("RecordAudit error: %v", err)
	}
}

func testControlPlaneConfig(authCoreURL string) config.Config {
	return config.Config{
		ServiceName:            "integration-corev2",
		InternalAPIKey:         "fallback-key",
		AuthCoreURL:            authCoreURL,
		AuthCoreInternalAPIKey: "internal-key",
		OrgCoreURL:             "http://org-core",
		BillingCoreURL:         "http://billing-core",
		AuditCoreURL:           "http://audit-core",
	}
}
