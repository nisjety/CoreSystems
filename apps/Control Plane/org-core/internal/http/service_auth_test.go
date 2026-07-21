package http

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
)

const orgServiceTestToken = "0123456789abcdef0123456789abcdef"

func orgServiceTestRegistry(scopes ...string) string {
	raw := `[{"principal":"velion-gateway","audience":"org-core","token":"` + orgServiceTestToken + `","scopes":[`
	for index, scope := range scopes {
		if index > 0 {
			raw += ","
		}
		raw += `"` + scope + `"`
	}
	return raw + `]}]`
}

func orgRequiredServiceTestRegistry(gatewayScopes, authScopes []string) string {
	raw := orgServiceTestRegistry(gatewayScopes...)
	raw = raw[:len(raw)-1] + `,{"principal":"auth-core","audience":"org-core","token":"fedcba9876543210fedcba9876543210","scopes":[`
	for index, scope := range authScopes {
		if index > 0 {
			raw += ","
		}
		raw += `"` + scope + `"`
	}
	return raw + `]}]`
}

func signOrgDelegation(request *http.Request, body []byte, timestamp time.Time, nonce string) {
	digest := serviceDelegationBodyDigest(body)
	claims := serviceDelegationClaims{
		Principal:  "velion-gateway",
		Audience:   "org-core",
		Timestamp:  timestamp.UTC().Format(time.RFC3339),
		Nonce:      nonce,
		Method:     request.Method,
		URI:        request.URL.RequestURI(),
		UserID:     "user-1",
		OrgID:      "org-1",
		UserRole:   "admin",
		BodySHA256: digest,
	}
	request.Header.Set("X-Service-Id", claims.Principal)
	request.Header.Set("X-Service-Token", orgServiceTestToken)
	request.Header.Set("X-User-Id", claims.UserID)
	request.Header.Set("X-Org-Id", claims.OrgID)
	request.Header.Set("X-User-Role", claims.UserRole)
	request.Header.Set("X-Delegation-Version", "v3")
	request.Header.Set("X-Delegation-Timestamp", claims.Timestamp)
	request.Header.Set("X-Delegation-Nonce", claims.Nonce)
	request.Header.Set("X-Delegation-Body-SHA256", digest)
	request.Header.Set("X-Delegation-Signature", serviceDelegationSignature(orgServiceTestToken, claims))
}

func orgServiceAuthTestRouter(t *testing.T, raw string, now time.Time) *gin.Engine {
	t.Helper()
	credentials, err := parseServiceCredentials(raw)
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, func() time.Time { return now }))
	router.GET("/health", func(c *gin.Context) { c.Status(http.StatusNoContent) })
	router.GET("/orgs/:id", func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"user": c.GetHeader("X-User-Id"),
			"org":  c.GetHeader("X-Org-Id"),
			"role": c.GetHeader("X-User-Role"),
		})
	})
	router.POST("/orgs/:id/plan", func(c *gin.Context) { c.Status(http.StatusNoContent) })
	router.POST("/internal/orgs/:id/onboarding/state", func(c *gin.Context) { c.Status(http.StatusNoContent) })
	return router
}

func TestParseOrgServiceCredentialsFailsClosed(t *testing.T) {
	valid := orgServiceTestRegistry("org:read:self", "org:provision:self", "org:settings:write:self")
	tests := []struct {
		name string
		raw  string
	}{
		{name: "empty", raw: ""},
		{name: "malformed", raw: "{"},
		{name: "unknown field", raw: `[{"principal":"velion-gateway","audience":"org-core","token":"0123456789abcdef0123456789abcdef","scopes":["org:read:self"],"extra":true}]`},
		{name: "wrong audience", raw: `[{"principal":"velion-gateway","audience":"billing-core","token":"0123456789abcdef0123456789abcdef","scopes":["org:read:self"]}]`},
		{name: "short token", raw: `[{"principal":"velion-gateway","audience":"org-core","token":"short","scopes":["org:read:self"]}]`},
		{name: "test token", raw: `[{"principal":"velion-gateway","audience":"org-core","token":"test-generated-secret-value-at-least-32-bytes","scopes":["org:read:self"]}]`},
		{name: "placeholder token", raw: `[{"principal":"velion-gateway","audience":"org-core","token":"placeholder-generated-secret-at-least-32-bytes","scopes":["org:read:self"]}]`},
		{name: "change-me token", raw: `[{"principal":"velion-gateway","audience":"org-core","token":"change-me-generated-secret-at-least-32-bytes","scopes":["org:read:self"]}]`},
		{name: "replace-with token", raw: `[{"principal":"velion-gateway","audience":"org-core","token":"replace-with-generated-secret-at-least-32-bytes","scopes":["org:read:self"]}]`},
		{name: "unknown scope", raw: `[{"principal":"velion-gateway","audience":"org-core","token":"0123456789abcdef0123456789abcdef","scopes":["org:root"]}]`},
		{name: "duplicate principal", raw: valid[:len(valid)-1] + `,{"principal":"velion-gateway","audience":"org-core","token":"abcdef0123456789abcdef0123456789","scopes":["org:read:self"]}]`},
		{name: "duplicate token", raw: `[{"principal":"machine-a","audience":"org-core","token":"0123456789abcdef0123456789abcdef","scopes":["org:read:any"]},{"principal":"machine-b","audience":"org-core","token":"0123456789abcdef0123456789abcdef","scopes":["org:read:any"]}]`},
		{name: "duplicate scope", raw: orgServiceTestRegistry("org:read:self", "org:read:self")},
		{name: "self scope on machine", raw: `[{"principal":"machine-a","audience":"org-core","token":"0123456789abcdef0123456789abcdef","scopes":["org:read:self"]}]`},
		{name: "any scope on gateway", raw: orgServiceTestRegistry("org:read:any")},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if _, err := parseServiceCredentials(test.raw); err == nil {
				t.Fatal("expected registry validation error")
			}
		})
	}
	if credentials, err := parseServiceCredentials(valid); err != nil || len(credentials) != 1 {
		t.Fatalf("valid registry rejected: credentials=%d err=%v", len(credentials), err)
	}
}

func TestValidateRequiredOrgServiceCredentialRegistry(t *testing.T) {
	if err := ValidateRequiredServiceCredentialRegistry(orgRequiredServiceTestRegistry(
		[]string{"org:read:self", "org:provision:self", "org:settings:write:self"},
		[]string{"org:projection:write:any", "org:projection:delete:any"},
	)); err == nil {
		t.Fatal("expected missing gateway onboarding scope to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(orgRequiredServiceTestRegistry(
		[]string{"org:read:self", "org:provision:self"},
		[]string{"org:projection:write:any", "org:projection:delete:any"},
	)); err == nil {
		t.Fatal("expected missing gateway settings scope to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(orgServiceTestRegistry("org:read:self", "org:provision:self", "org:settings:write:self", "org:onboarding:write:self")); err == nil {
		t.Fatal("expected missing auth-core principal to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(orgRequiredServiceTestRegistry(
		[]string{"org:read:self", "org:provision:self", "org:settings:write:self", "org:onboarding:write:self"},
		[]string{"org:projection:write:any"},
	)); err == nil {
		t.Fatal("expected missing auth-core projection delete scope to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(orgRequiredServiceTestRegistry(
		[]string{"org:read:self", "org:provision:self", "org:settings:write:self", "org:onboarding:write:self"},
		[]string{"org:projection:write:any", "org:projection:delete:any"},
	)); err != nil {
		t.Fatalf("valid required registry rejected: %v", err)
	}
}

func TestOrgServiceScopeForRequest(t *testing.T) {
	tests := []struct {
		method string
		path   string
		want   []string
	}{
		{method: http.MethodPost, path: "/api/v1/auth/login", want: []string{"org:auth:proxy"}},
		{method: http.MethodGet, path: "/api/v1/users/me", want: []string{"org:read:self"}},
		{method: http.MethodGet, path: "/api/v1/organizations", want: []string{"org:read:self"}},
		{method: http.MethodPost, path: "/api/v1/organizations", want: []string{"org:provision:self"}},
		{method: http.MethodGet, path: "/api/v1/organizations/org-1", want: []string{"org:read:self", "org:read:any"}},
		{method: http.MethodGet, path: "/api/v1/organizations/org-1/entitlements", want: []string{"org:read:self", "org:read:any"}},
		{method: http.MethodGet, path: "/api/v1/organizations/org-1/members/search", want: []string{"org:read:self", "org:read:any"}},
		{method: http.MethodPost, path: "/api/v1/organizations/org-1/plan", want: []string{"org:settings:write:self"}},
		{method: http.MethodPatch, path: "/api/v1/organizations/org-1/settings", want: []string{"org:settings:write:self"}},
		{method: http.MethodPatch, path: "/api/v1/organizations/org-1/brreg", want: []string{"org:settings:write:self"}},
		{method: http.MethodGet, path: "/orgs/org-1", want: []string{"org:read:self", "org:read:any"}},
		{method: http.MethodGet, path: "/orgs", want: []string{"org:read:self"}},
		{method: http.MethodPost, path: "/orgs", want: []string{"org:provision:self"}},
		{method: http.MethodGet, path: "/orgs/me", want: []string{"org:read:self"}},
		{method: http.MethodGet, path: "/orgs/org-1/entitlements", want: []string{"org:read:self", "org:read:any"}},
		{method: http.MethodGet, path: "/orgs/org-1/members", want: []string{"org:read:self", "org:read:any"}},
		{method: http.MethodGet, path: "/orgs/org-1/roles", want: []string{"org:read:self", "org:read:any"}},
		{method: http.MethodGet, path: "/orgs/org-1/members/search", want: []string{"org:read:self", "org:read:any"}},
		{method: http.MethodGet, path: "/orgs/org-1/roles/catalog", want: []string{"org:read:self", "org:read:any"}},
		{method: http.MethodPost, path: "/orgs/org-1/plan", want: []string{"org:settings:write:self"}},
		{method: http.MethodPatch, path: "/orgs/org-1/capabilities", want: []string{"org:settings:write:self"}},
		{method: http.MethodPatch, path: "/orgs/org-1/settings", want: []string{"org:settings:write:self"}},
		{method: http.MethodDelete, path: "/orgs/org-1/gdpr/erase", want: []string{"org:erase:self"}},
		{method: http.MethodDelete, path: "/orgs/org-1/gdpr/soft-delete", want: []string{"org:erase:self"}},
		{method: http.MethodGet, path: "/api/v1/brreg/search", want: []string{"org:brreg:read"}},
		{method: http.MethodGet, path: "/api/v1/brreg/123456789", want: []string{"org:brreg:read"}},
		{method: http.MethodGet, path: "/internal/orgs/by-tenant", want: []string{"org:tenant:read:any"}},
		{method: http.MethodPost, path: "/internal/orgs/ensure-from-tenant", want: []string{"org:tenant:write:any"}},
		{method: http.MethodPost, path: "/internal/orgs/org-1/onboarding/state", want: []string{"org:onboarding:write:self", "org:onboarding:write:any"}},
		{method: http.MethodPost, path: "/internal/orgs/org-1/reconcile", want: []string{"org:projection:write:any"}},
		{method: http.MethodPost, path: "/internal/orgs/org-1/members/reconcile", want: []string{"org:projection:write:any"}},
		{method: http.MethodPost, path: "/internal/orgs/org-1/reconcile-delete", want: []string{"org:projection:delete:any"}},
		{method: http.MethodPost, path: "/internal/orgs/org-1/members/user-1/succession", want: []string{"org:membership:succession:any"}},
		{method: http.MethodGet, path: "/orgs//org-1", want: nil},
		{method: http.MethodPost, path: "/orgs/org-1/members/invite", want: nil},
	}
	for _, test := range tests {
		request := httptest.NewRequest(test.method, test.path, nil)
		got := serviceScopesForRequest(request)
		if len(got) != len(test.want) {
			t.Fatalf("%s %s scopes=%v want=%v", test.method, test.path, got, test.want)
		}
		for index := range got {
			if got[index] != test.want[index] {
				t.Fatalf("%s %s scopes=%v want=%v", test.method, test.path, got, test.want)
			}
		}
	}
}

func TestOrgServiceAuthRejectsLegacyPartialInvalidAndUnscopedCredentials(t *testing.T) {
	now := time.Date(2026, 7, 14, 10, 0, 0, 0, time.UTC)
	router := orgServiceAuthTestRouter(t, orgServiceTestRegistry("org:settings:write:self"), now)
	tests := []struct {
		name    string
		headers map[string]string
		want    int
	}{
		{name: "missing", want: http.StatusUnauthorized},
		{name: "legacy key", headers: map[string]string{"X-Internal-Api-Key": orgServiceTestToken}, want: http.StatusUnauthorized},
		{name: "partial", headers: map[string]string{"X-Service-Id": "velion-gateway"}, want: http.StatusUnauthorized},
		{name: "wrong token", headers: map[string]string{"X-Service-Id": "velion-gateway", "X-Service-Token": "wrong"}, want: http.StatusUnauthorized},
		{name: "missing route scope", headers: map[string]string{"X-Service-Id": "velion-gateway", "X-Service-Token": orgServiceTestToken}, want: http.StatusForbidden},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			request := httptest.NewRequest(http.MethodGet, "/orgs/org-1", nil)
			for name, value := range test.headers {
				request.Header.Set(name, value)
			}
			response := httptest.NewRecorder()
			router.ServeHTTP(response, request)
			if response.Code != test.want {
				t.Fatalf("status=%d want=%d body=%s", response.Code, test.want, response.Body.String())
			}
		})
	}
}

func TestOrgGatewaySelfScopeRequiresFreshBoundNonReplayableDelegation(t *testing.T) {
	now := time.Date(2026, 7, 14, 10, 0, 0, 0, time.UTC)
	router := orgServiceAuthTestRouter(t, orgServiceTestRegistry("org:read:self", "org:provision:self", "org:settings:write:self"), now)

	validRequest := func() *http.Request {
		request := httptest.NewRequest(http.MethodGet, "/orgs/org-1?include=plan", nil)
		signOrgDelegation(request, nil, now, "nonce-0123456789abcdef")
		return request
	}

	request := validRequest()
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusOK || !bytes.Contains(response.Body.Bytes(), []byte(`"role":"admin"`)) {
		t.Fatalf("valid delegation status=%d body=%s", response.Code, response.Body.String())
	}

	replay := validRequest()
	replayResponse := httptest.NewRecorder()
	router.ServeHTTP(replayResponse, replay)
	if replayResponse.Code != http.StatusForbidden {
		t.Fatalf("replay status=%d want=%d", replayResponse.Code, http.StatusForbidden)
	}

	tamperCases := []struct {
		name   string
		tamper func(*http.Request)
	}{
		{name: "version", tamper: func(r *http.Request) { r.Header.Set("X-Delegation-Version", "v2") }},
		{name: "timestamp", tamper: func(r *http.Request) {
			r.Header.Set("X-Delegation-Timestamp", now.Add(-31*time.Second).Format(time.RFC3339))
		}},
		{name: "nonce", tamper: func(r *http.Request) { r.Header.Set("X-Delegation-Nonce", "nonce-fedcba9876543210") }},
		{name: "method", tamper: func(r *http.Request) { r.Method = http.MethodHead }},
		{name: "query", tamper: func(r *http.Request) { r.URL.RawQuery = "include=members" }},
		{name: "user", tamper: func(r *http.Request) { r.Header.Set("X-User-Id", "victim") }},
		{name: "org", tamper: func(r *http.Request) { r.Header.Set("X-Org-Id", "org-2") }},
		{name: "role", tamper: func(r *http.Request) { r.Header.Set("X-User-Role", "owner") }},
	}
	for index, test := range tamperCases {
		t.Run(test.name, func(t *testing.T) {
			candidate := httptest.NewRequest(http.MethodGet, "/orgs/org-1?include=plan", nil)
			signOrgDelegation(candidate, nil, now, "unique-nonce-01234567"+string(rune('a'+index)))
			test.tamper(candidate)
			candidateResponse := httptest.NewRecorder()
			router.ServeHTTP(candidateResponse, candidate)
			if candidateResponse.Code != http.StatusForbidden {
				t.Fatalf("tampered request status=%d want=%d", candidateResponse.Code, http.StatusForbidden)
			}
		})
	}

	body := []byte(`{"plan":"pro"}`)
	bodyRequest := httptest.NewRequest(http.MethodPost, "/orgs/org-1/plan", bytes.NewReader([]byte(`{"plan":"enterprise"}`)))
	signOrgDelegation(bodyRequest, body, now, "body-nonce-0123456789")
	bodyResponse := httptest.NewRecorder()
	router.ServeHTTP(bodyResponse, bodyRequest)
	if bodyResponse.Code != http.StatusForbidden {
		t.Fatalf("tampered body status=%d want=%d", bodyResponse.Code, http.StatusForbidden)
	}
}

func TestOrgMachineScopeDoesNotTrustCallerIdentityHeaders(t *testing.T) {
	credentials, err := parseServiceCredentials(`[{"principal":"session-core","audience":"org-core","token":"abcdef0123456789abcdef0123456789","scopes":["org:read:any"]}]`)
	if err != nil {
		t.Fatalf("parse machine credential: %v", err)
	}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	router.GET("/orgs/:id", func(c *gin.Context) {
		if c.GetHeader("X-User-Id") != "" || c.GetHeader("X-Org-Id") != "" || c.GetHeader("X-User-Role") != "" {
			c.Status(http.StatusInternalServerError)
			return
		}
		c.Status(http.StatusNoContent)
	})
	request := httptest.NewRequest(http.MethodGet, "/orgs/org-1", nil)
	request.Header.Set("X-Service-Id", "session-core")
	request.Header.Set("X-Service-Token", "abcdef0123456789abcdef0123456789")
	request.Header.Set("X-User-Id", "victim")
	request.Header.Set("X-Org-Id", "org-2")
	request.Header.Set("X-User-Role", "owner")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusNoContent {
		t.Fatalf("machine request status=%d want=%d", response.Code, http.StatusNoContent)
	}
}

func TestOrgOnboardingSelfScopeBindsDelegatedOrgToInternalPath(t *testing.T) {
	now := time.Date(2026, 7, 14, 10, 0, 0, 0, time.UTC)
	router := orgServiceAuthTestRouter(t, orgServiceTestRegistry("org:onboarding:write:self"), now)

	tampered := httptest.NewRequest(http.MethodPost, "/internal/orgs/org-2/onboarding/state", nil)
	signOrgDelegation(tampered, nil, now, "onboarding-path-tamper-01")
	tamperedResponse := httptest.NewRecorder()
	router.ServeHTTP(tamperedResponse, tampered)
	if tamperedResponse.Code != http.StatusForbidden {
		t.Fatalf("path-tampered onboarding status=%d want=%d", tamperedResponse.Code, http.StatusForbidden)
	}
}

func TestOrgServiceDelegationV3CrossLanguageVector(t *testing.T) {
	bodyDigest := serviceDelegationBodyDigest([]byte(`{"plan":"pro"}`))
	if bodyDigest != "ApxA0uXOJFNQhvraZ-s-yFofgWfVqZ6reRfsBXYSbpk" {
		t.Fatalf("body digest=%q", bodyDigest)
	}
	signature := serviceDelegationSignature(orgServiceTestToken, serviceDelegationClaims{
		Principal:  "velion-gateway",
		Audience:   "org-core",
		Timestamp:  "2026-07-14T10:00:00Z",
		Nonce:      "nonce-0123456789abcdef",
		Method:     http.MethodPost,
		URI:        "/orgs/org-1/plan?dry_run=true",
		UserID:     "user-1",
		OrgID:      "org-1",
		UserRole:   "admin",
		BodySHA256: bodyDigest,
	})
	if signature != "wtNSej6lIRXbLkJzzkTODsGcMXLoIi8zCBzssQ1bIr0" {
		t.Fatalf("cross-language signature=%q", signature)
	}
}
