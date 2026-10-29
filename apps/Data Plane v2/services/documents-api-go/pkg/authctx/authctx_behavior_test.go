package authctx

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestValidateProductionAndDevelopmentPostures(t *testing.T) {
	t.Run("observe requires explicit insecure development gate", func(t *testing.T) {
		observe := false
		t.Setenv("ALLOW_INSECURE_DEV_DEFAULTS", "")
		if err := Validate(Config{Enforce: &observe}); err == nil {
			t.Fatal("observe mode started without ALLOW_INSECURE_DEV_DEFAULTS=1")
		}
		t.Setenv("ALLOW_INSECURE_DEV_DEFAULTS", "1")
		if err := Validate(Config{Enforce: &observe}); err != nil {
			t.Fatalf("explicitly gated observe mode: %v", err)
		}
	})

	t.Run("enforce requires audience", func(t *testing.T) {
		key := writeTestKey(t)
		_ = key
		enforce := true
		if err := Validate(Config{ExpectedIssuer: testIssuer, Enforce: &enforce}); err == nil {
			t.Fatal("enforce mode started without an audience")
		}
	})

	t.Run("enforce requires issuer", func(t *testing.T) {
		key := writeTestKey(t)
		_ = key
		enforce := true
		if err := Validate(Config{Audience: testAudience, Enforce: &enforce}); err == nil {
			t.Fatal("enforce mode started without an issuer")
		}
	})

	t.Run("enforce requires verification material", func(t *testing.T) {
		t.Setenv("JWT_PUBLIC_KEY_FILE", "")
		t.Setenv("AUTH_CORE_JWKS_URL", "")
		t.Setenv("JWT_JWKS_URL", "")
		enforce := true
		if err := Validate(Config{Audience: testAudience, ExpectedIssuer: testIssuer, Enforce: &enforce}); err == nil {
			t.Fatal("enforce mode started without a static key or JWKS URL")
		}
	})

	t.Run("enforce accepts valid static key", func(t *testing.T) {
		writeTestKey(t)
		enforce := true
		if err := Validate(Config{Audience: testAudience, ExpectedIssuer: testIssuer, Enforce: &enforce}); err != nil {
			t.Fatalf("valid static verification key: %v", err)
		}
	})

	t.Run("enforce rejects unreadable static key", func(t *testing.T) {
		t.Setenv("JWT_PUBLIC_KEY_FILE", filepath.Join(t.TempDir(), "missing.pem"))
		t.Setenv("AUTH_CORE_JWKS_URL", "")
		t.Setenv("JWT_JWKS_URL", "")
		enforce := true
		if err := Validate(Config{Audience: testAudience, ExpectedIssuer: testIssuer, Enforce: &enforce}); err == nil {
			t.Fatal("enforce mode accepted an unreadable static key")
		}
	})

	t.Run("enforce rejects invalid static key", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "invalid.pem")
		if err := os.WriteFile(path, []byte("not a public key"), 0o600); err != nil {
			t.Fatalf("write invalid key: %v", err)
		}
		t.Setenv("JWT_PUBLIC_KEY_FILE", path)
		t.Setenv("AUTH_CORE_JWKS_URL", "")
		t.Setenv("JWT_JWKS_URL", "")
		enforce := true
		if err := Validate(Config{Audience: testAudience, ExpectedIssuer: testIssuer, Enforce: &enforce}); err == nil {
			t.Fatal("enforce mode accepted an invalid static key")
		}
	})

	t.Run("enforce accepts configured HTTPS JWKS", func(t *testing.T) {
		t.Setenv("JWT_PUBLIC_KEY_FILE", "")
		t.Setenv("AUTH_CORE_JWKS_URL", "")
		t.Setenv("JWT_JWKS_URL", "")
		enforce := true
		if err := Validate(Config{
			Audience:       testAudience,
			ExpectedIssuer: testIssuer,
			JWKSURL:        "https://auth.example.test/.well-known/jwks.json",
			Enforce:        &enforce,
		}); err != nil {
			t.Fatalf("configured HTTPS JWKS: %v", err)
		}
	})
}

func TestResolveEnforceOnlyAllowsExplicitFalseValues(t *testing.T) {
	for _, value := range []string{"0", "false", "FALSE", "no", "off", " Off "} {
		t.Run(value, func(t *testing.T) {
			t.Setenv("AUTHCTX_ENFORCE", value)
			if (&Config{}).resolveEnforce() {
				t.Fatalf("AUTHCTX_ENFORCE=%q resolved to enforced", value)
			}
		})
	}
	for _, value := range []string{"", "1", "true", "unexpected"} {
		t.Run("enforced_"+value, func(t *testing.T) {
			t.Setenv("AUTHCTX_ENFORCE", value)
			if !((&Config{}).resolveEnforce()) {
				t.Fatalf("AUTHCTX_ENFORCE=%q did not fail closed", value)
			}
		})
	}
	forceObserve := false
	if (&Config{Enforce: &forceObserve}).resolveEnforce() {
		t.Fatal("explicit test override was ignored")
	}
}

func TestObserveMiddlewareDecodesOnlyForTelemetry(t *testing.T) {
	observe := false
	var got *Claims
	handler := Middleware(Config{Audience: testAudience, Enforce: &observe})(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		got, _ = FromContext(r.Context())
		w.WriteHeader(http.StatusNoContent)
	}))

	payload, err := json.Marshal(validClaims())
	if err != nil {
		t.Fatalf("marshal claims: %v", err)
	}
	// Padded URL encoding exercises compatibility with older JWT encoders.
	token := "header." + base64.URLEncoding.EncodeToString(payload) + ".signature"
	req := httptest.NewRequest(http.MethodGet, "/v1/documents", nil)
	req.Header.Set("Authorization", "bearer "+token)
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, req)

	if response.Code != http.StatusNoContent {
		t.Fatalf("observe response = %d, want 204", response.Code)
	}
	if got == nil || got.UserID != "user_123" || got.OrgID != "org_abc" {
		t.Fatalf("observe claims = %+v", got)
	}
	if got.Verified || got.PrincipalID() != "" || got.HasScope("documents:read") {
		t.Fatal("observe-mode claims must never become an authorization principal")
	}
}

func TestObserveMiddlewarePassesMalformedOrMissingCredentialsWithoutClaims(t *testing.T) {
	observe := false
	tests := []struct {
		name   string
		header string
	}{
		{name: "missing"},
		{name: "wrong scheme", header: "Basic credentials"},
		{name: "empty bearer", header: "Bearer    "},
		{name: "wrong segment count", header: "Bearer not-a-jwt"},
		{name: "invalid base64", header: "Bearer a.%%%.c"},
		{name: "invalid JSON", header: "Bearer a.e25vdC1qc29ufQ.c"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			gotClaims := false
			handler := Middleware(Config{Audience: testAudience, Enforce: &observe})(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				_, gotClaims = FromContext(r.Context())
				w.WriteHeader(http.StatusNoContent)
			}))
			req := httptest.NewRequest(http.MethodGet, "/v1/documents", nil)
			if tc.header != "" {
				req.Header.Set("Authorization", tc.header)
			}
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, req)
			if response.Code != http.StatusNoContent || gotClaims {
				t.Fatalf("response=%d gotClaims=%v, want 204/false", response.Code, gotClaims)
			}
		})
	}
}

func TestEnforceMiddlewarePinsClaimsAndTenantHeader(t *testing.T) {
	key := writeTestKey(t)
	enforce := true
	handler := Middleware(Config{Audience: testAudience, ExpectedIssuer: testIssuer, Enforce: &enforce})(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		claims, ok := FromContext(r.Context())
		if !ok || !claims.Verified || claims.PrincipalID() != "user_123" {
			http.Error(w, "missing verified claims", http.StatusInternalServerError)
			return
		}
		if r.Header.Get("X-Org-ID") != "org_abc" {
			http.Error(w, "tenant was not pinned", http.StatusInternalServerError)
			return
		}
		w.WriteHeader(http.StatusNoContent)
	}))

	req := httptest.NewRequest(http.MethodGet, "/v1/documents", nil)
	req.Header.Set("Authorization", "bEaReR "+signToken(t, key, validClaims()))
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, req)
	if response.Code != http.StatusNoContent {
		t.Fatalf("response = %d, want 204", response.Code)
	}
}

func TestEnforceMiddlewareRejectsMalformedAndBadSignatureTokens(t *testing.T) {
	writeTestKey(t)
	enforce := true
	handler := Middleware(Config{Audience: testAudience, ExpectedIssuer: testIssuer, Enforce: &enforce})(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	}))
	otherKey := generateRSAKey(t)
	tests := []string{
		"not-a-jwt",
		signToken(t, otherKey, validClaims()),
	}
	for _, token := range tests {
		req := httptest.NewRequest(http.MethodGet, "/v1/documents", nil)
		req.Header.Set("Authorization", "Bearer "+token)
		response := httptest.NewRecorder()
		handler.ServeHTTP(response, req)
		if response.Code != http.StatusUnauthorized {
			t.Fatalf("token was not rejected: status=%d", response.Code)
		}
	}
}

func TestClaimsAuthorizationHelpersFailClosed(t *testing.T) {
	var nilClaims *Claims
	if nilClaims.IsService() || nilClaims.PrincipalID() != "" || nilClaims.HasScope("documents:read") {
		t.Fatal("nil claims must not authorize")
	}
	unverified := &Claims{UserID: "user-1", PrincipalType: "user", Scopes: []string{"documents:read"}}
	if unverified.PrincipalID() != "" || unverified.HasScope("documents:read") {
		t.Fatal("unverified claims must not authorize")
	}
	user := &Claims{UserID: "user-1", PrincipalType: "user", Verified: true}
	if user.PrincipalID() != "user-1" || user.IsService() {
		t.Fatalf("unexpected user identity: %+v", user)
	}
	service := &Claims{ServiceID: "service:reader", PrincipalType: "service", Scopes: []string{"documents:read"}, Verified: true}
	if service.PrincipalID() != "service:reader" || !service.IsService() || !service.HasScope("documents:read") || service.HasScope("documents:write") {
		t.Fatalf("unexpected service identity: %+v", service)
	}
	unknown := &Claims{UserID: "opaque", PrincipalType: "unknown", Verified: true}
	if unknown.PrincipalID() != "" {
		t.Fatal("unknown verified principal type must not authorize")
	}
}

func TestClaimsExpirationUsesThirtySecondLeeway(t *testing.T) {
	now := time.Unix(10_000, 0)
	tests := []struct {
		name   string
		claims *Claims
		want   bool
	}{
		{name: "nil", claims: nil, want: true},
		{name: "missing", claims: &Claims{}, want: true},
		{name: "future", claims: &Claims{ExpiresAt: now.Add(time.Minute).Unix()}, want: false},
		{name: "inside leeway", claims: &Claims{ExpiresAt: now.Add(-20 * time.Second).Unix()}, want: false},
		{name: "past leeway", claims: &Claims{ExpiresAt: now.Add(-31 * time.Second).Unix()}, want: true},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			if got := tc.claims.IsExpired(now); got != tc.want {
				t.Fatalf("IsExpired() = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestContextAndServiceScopeRejectMissingOrUnknownPrincipal(t *testing.T) {
	if _, ok := FromContext(context.Background()); ok {
		t.Fatal("empty context unexpectedly contained claims")
	}
	handler := RequireServiceScope("documents:read")(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNoContent)
	}))
	tests := []struct {
		name   string
		claims *Claims
	}{
		{name: "missing"},
		{name: "unverified", claims: &Claims{ServiceID: "service:reader", PrincipalType: "service", Scopes: []string{"documents:read"}}},
		{name: "unknown", claims: &Claims{UserID: "opaque", PrincipalType: "unknown", Verified: true}},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodGet, "/v1/documents", nil)
			if tc.claims != nil {
				req = req.WithContext(IntoContext(req.Context(), tc.claims))
			}
			response := httptest.NewRecorder()
			handler.ServeHTTP(response, req)
			if response.Code != http.StatusUnauthorized {
				t.Fatalf("response = %d, want 401", response.Code)
			}
		})
	}
}
