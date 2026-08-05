package http

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
)

const billingServiceTestToken = "abcdef0123456789abcdef0123456789"

func billingServiceTestRegistry(scopes ...string) string {
	raw := `[{"principal":"verevon-gateway","audience":"billing-core","token":"` + billingServiceTestToken + `","scopes":[`
	for index, scope := range scopes {
		if index > 0 {
			raw += ","
		}
		raw += `"` + scope + `"`
	}
	return raw + `]}]`
}

func billingRequiredServiceTestRegistry(gatewayScopes, authScopes []string) string {
	raw := billingServiceTestRegistry(gatewayScopes...)
	raw = raw[:len(raw)-1] + `,{"principal":"auth-core","audience":"billing-core","token":"0123456789abcdef0123456789abcdef","scopes":[`
	for index, scope := range authScopes {
		if index > 0 {
			raw += ","
		}
		raw += `"` + scope + `"`
	}
	return raw + `]}]`
}

func signBillingDelegation(request *http.Request, body []byte, timestamp time.Time, nonce string) {
	digest := serviceDelegationBodyDigest(body)
	claims := serviceDelegationClaims{
		Principal:  "verevon-gateway",
		Audience:   "billing-core",
		Timestamp:  timestamp.UTC().Format(time.RFC3339),
		Nonce:      nonce,
		Method:     request.Method,
		URI:        request.URL.RequestURI(),
		UserID:     "user-1",
		OrgID:      "org-1",
		UserRole:   "owner",
		BodySHA256: digest,
	}
	request.Header.Set("X-Service-Id", claims.Principal)
	request.Header.Set("X-Service-Token", billingServiceTestToken)
	request.Header.Set("X-User-Id", claims.UserID)
	request.Header.Set("X-Org-Id", claims.OrgID)
	request.Header.Set("X-User-Role", claims.UserRole)
	request.Header.Set("X-Delegation-Version", "v3")
	request.Header.Set("X-Delegation-Timestamp", claims.Timestamp)
	request.Header.Set("X-Delegation-Nonce", claims.Nonce)
	request.Header.Set("X-Delegation-Body-SHA256", digest)
	request.Header.Set("X-Delegation-Signature", serviceDelegationSignature(billingServiceTestToken, claims))
}

func billingServiceAuthTestRouter(t *testing.T, raw string, now time.Time) *gin.Engine {
	t.Helper()
	credentials, err := parseServiceCredentials(raw)
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, func() time.Time { return now }))
	router.GET("/health", func(c *gin.Context) { c.Status(http.StatusNoContent) })
	router.GET("/api/v1/billing/orgs/:orgId/account", func(c *gin.Context) { c.Status(http.StatusNoContent) })
	router.POST("/api/v1/billing/orgs/:orgId/checkout-session", func(c *gin.Context) { c.Status(http.StatusNoContent) })
	router.POST(nexiWebhookPath, func(c *gin.Context) { c.Status(http.StatusAccepted) })
	return router
}

func TestParseBillingServiceCredentialsFailsClosed(t *testing.T) {
	valid := billingServiceTestRegistry(
		"billing:account:read:self",
		"billing:entitlement:read:self",
		"billing:quota:read:self",
		"billing:checkout:create:self",
		"billing:checkout:confirm:self",
	)
	tests := []struct {
		name string
		raw  string
	}{
		{name: "empty", raw: ""},
		{name: "malformed", raw: "{"},
		{name: "unknown field", raw: `[{"principal":"verevon-gateway","audience":"billing-core","token":"abcdef0123456789abcdef0123456789","scopes":["billing:account:read:self"],"extra":true}]`},
		{name: "wrong audience", raw: `[{"principal":"verevon-gateway","audience":"org-core","token":"abcdef0123456789abcdef0123456789","scopes":["billing:account:read:self"]}]`},
		{name: "short token", raw: `[{"principal":"verevon-gateway","audience":"billing-core","token":"short","scopes":["billing:account:read:self"]}]`},
		{name: "test token", raw: `[{"principal":"verevon-gateway","audience":"billing-core","token":"test-generated-secret-value-at-least-32-bytes","scopes":["billing:account:read:self"]}]`},
		{name: "placeholder token", raw: `[{"principal":"verevon-gateway","audience":"billing-core","token":"placeholder-generated-secret-at-least-32-bytes","scopes":["billing:account:read:self"]}]`},
		{name: "change-me token", raw: `[{"principal":"verevon-gateway","audience":"billing-core","token":"change-me-generated-secret-at-least-32-bytes","scopes":["billing:account:read:self"]}]`},
		{name: "replace-with token", raw: `[{"principal":"verevon-gateway","audience":"billing-core","token":"replace-with-generated-secret-at-least-32-bytes","scopes":["billing:account:read:self"]}]`},
		{name: "unknown scope", raw: `[{"principal":"verevon-gateway","audience":"billing-core","token":"abcdef0123456789abcdef0123456789","scopes":["billing:root"]}]`},
		{name: "duplicate principal", raw: valid[:len(valid)-1] + `,{"principal":"verevon-gateway","audience":"billing-core","token":"0123456789abcdef0123456789abcdef","scopes":["billing:account:read:self"]}]`},
		{name: "duplicate token", raw: `[{"principal":"machine-a","audience":"billing-core","token":"abcdef0123456789abcdef0123456789","scopes":["billing:account:read:any"]},{"principal":"machine-b","audience":"billing-core","token":"abcdef0123456789abcdef0123456789","scopes":["billing:account:read:any"]}]`},
		{name: "duplicate scope", raw: billingServiceTestRegistry("billing:account:read:self", "billing:account:read:self")},
		{name: "self scope on machine", raw: `[{"principal":"machine-a","audience":"billing-core","token":"abcdef0123456789abcdef0123456789","scopes":["billing:account:read:self"]}]`},
		{name: "any scope on gateway", raw: billingServiceTestRegistry("billing:account:read:any")},
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

func TestValidateRequiredBillingServiceCredentialRegistry(t *testing.T) {
	if err := ValidateRequiredServiceCredentialRegistry(billingRequiredServiceTestRegistry(
		[]string{"billing:account:read:self"}, []string{"billing:organization:deactivate:any"},
	)); err == nil {
		t.Fatal("expected incomplete gateway policy to fail startup validation")
	}
	completeGateway := []string{
		"billing:account:read:self", "billing:entitlement:read:self", "billing:quota:read:self",
		"billing:checkout:create:self", "billing:checkout:confirm:self",
	}
	if err := ValidateRequiredServiceCredentialRegistry(billingServiceTestRegistry(completeGateway...)); err == nil {
		t.Fatal("expected missing auth-core principal to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(billingRequiredServiceTestRegistry(completeGateway, []string{"billing:account:read:any"})); err == nil {
		t.Fatal("expected missing auth-core deactivate scope to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(billingRequiredServiceTestRegistry(
		completeGateway, []string{"billing:organization:deactivate:any"},
	)); err != nil {
		t.Fatalf("valid required registry rejected: %v", err)
	}
}

func TestBillingServiceScopeForRequest(t *testing.T) {
	tests := []struct {
		method string
		path   string
		want   []string
	}{
		{method: http.MethodGet, path: "/api/v1/billing/orgs/org-1/account", want: []string{"billing:account:read:self", "billing:account:read:any"}},
		{method: http.MethodPut, path: "/api/v1/billing/orgs/org-1/account", want: []string{"billing:account:write:any"}},
		{method: http.MethodPost, path: "/api/v1/billing/orgs/org-1/usage", want: []string{"billing:usage:write:any"}},
		{method: http.MethodGet, path: "/api/v1/billing/orgs/org-1/entitlements/feature", want: []string{"billing:entitlement:read:self", "billing:entitlement:read:any"}},
		{method: http.MethodPost, path: "/api/v1/billing/orgs/org-1/deactivate", want: []string{"billing:organization:deactivate:any"}},
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

func TestBillingServiceAuthRejectsLegacyPartialInvalidAndUnscopedCredentials(t *testing.T) {
	now := time.Date(2026, 7, 14, 10, 0, 0, 0, time.UTC)
	router := billingServiceAuthTestRouter(t, billingServiceTestRegistry("billing:checkout:create:self"), now)
	tests := []struct {
		name    string
		headers map[string]string
		want    int
	}{
		{name: "missing", want: http.StatusUnauthorized},
		{name: "legacy key", headers: map[string]string{"X-Internal-Api-Key": billingServiceTestToken}, want: http.StatusUnauthorized},
		{name: "partial", headers: map[string]string{"X-Service-Id": "verevon-gateway"}, want: http.StatusUnauthorized},
		{name: "wrong token", headers: map[string]string{"X-Service-Id": "verevon-gateway", "X-Service-Token": "wrong"}, want: http.StatusUnauthorized},
		{name: "missing route scope", headers: map[string]string{"X-Service-Id": "verevon-gateway", "X-Service-Token": billingServiceTestToken}, want: http.StatusForbidden},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			request := httptest.NewRequest(http.MethodGet, "/api/v1/billing/orgs/org-1/account", nil)
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

	webhook := httptest.NewRequest(http.MethodPost, nexiWebhookPath, nil)
	webhookResponse := httptest.NewRecorder()
	router.ServeHTTP(webhookResponse, webhook)
	if webhookResponse.Code != http.StatusAccepted {
		t.Fatalf("Nexi webhook must remain separately authenticated; status=%d", webhookResponse.Code)
	}
}

func TestBillingGatewaySelfScopeRequiresFreshBoundNonReplayableDelegation(t *testing.T) {
	now := time.Date(2026, 7, 14, 10, 0, 0, 0, time.UTC)
	router := billingServiceAuthTestRouter(t, billingServiceTestRegistry(
		"billing:account:read:self", "billing:entitlement:read:self", "billing:quota:read:self",
		"billing:checkout:create:self", "billing:checkout:confirm:self",
	), now)
	body := []byte(`{"plan":"pro"}`)
	request := httptest.NewRequest(http.MethodPost, "/api/v1/billing/orgs/org-1/checkout-session?return=app", bytes.NewReader(body))
	signBillingDelegation(request, body, now, "nonce-0123456789abcdef")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusNoContent {
		t.Fatalf("valid delegation status=%d body=%s", response.Code, response.Body.String())
	}

	replay := httptest.NewRequest(http.MethodPost, "/api/v1/billing/orgs/org-1/checkout-session?return=app", bytes.NewReader(body))
	signBillingDelegation(replay, body, now, "nonce-0123456789abcdef")
	replayResponse := httptest.NewRecorder()
	router.ServeHTTP(replayResponse, replay)
	if replayResponse.Code != http.StatusForbidden {
		t.Fatalf("replay status=%d want=%d", replayResponse.Code, http.StatusForbidden)
	}

	tamperCases := []struct {
		name   string
		tamper func(*http.Request)
	}{
		{name: "stale timestamp", tamper: func(r *http.Request) {
			r.Header.Set("X-Delegation-Timestamp", now.Add(-31*time.Second).Format(time.RFC3339))
		}},
		{name: "future timestamp", tamper: func(r *http.Request) {
			r.Header.Set("X-Delegation-Timestamp", now.Add(6*time.Second).Format(time.RFC3339))
		}},
		{name: "method", tamper: func(r *http.Request) { r.Method = http.MethodPut }},
		{name: "query", tamper: func(r *http.Request) { r.URL.RawQuery = "return=evil" }},
		{name: "user", tamper: func(r *http.Request) { r.Header.Set("X-User-Id", "victim") }},
		{name: "org", tamper: func(r *http.Request) { r.Header.Set("X-Org-Id", "org-2") }},
		{name: "role", tamper: func(r *http.Request) { r.Header.Set("X-User-Role", "admin") }},
		{name: "body", tamper: func(r *http.Request) { r.Body = ioNopCloserForTest([]byte(`{"plan":"enterprise"}`)) }},
	}
	for index, test := range tamperCases {
		t.Run(test.name, func(t *testing.T) {
			candidate := httptest.NewRequest(http.MethodPost, "/api/v1/billing/orgs/org-1/checkout-session?return=app", bytes.NewReader(body))
			signBillingDelegation(candidate, body, now, "unique-nonce-01234567"+string(rune('a'+index)))
			test.tamper(candidate)
			candidateResponse := httptest.NewRecorder()
			router.ServeHTTP(candidateResponse, candidate)
			if candidateResponse.Code != http.StatusForbidden {
				t.Fatalf("tampered request status=%d want=%d", candidateResponse.Code, http.StatusForbidden)
			}
		})
	}
}

func TestBillingMachineScopeDoesNotTrustCallerIdentityHeaders(t *testing.T) {
	credentials, err := parseServiceCredentials(`[{"principal":"org-core","audience":"billing-core","token":"0123456789abcdef0123456789abcdef","scopes":["billing:account:write:any"]}]`)
	if err != nil {
		t.Fatalf("parse machine credential: %v", err)
	}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	router.PUT("/api/v1/billing/orgs/:orgId/account", func(c *gin.Context) {
		if c.GetHeader("X-User-Id") != "" || c.GetHeader("X-Org-Id") != "" || c.GetHeader("X-User-Role") != "" {
			c.Status(http.StatusInternalServerError)
			return
		}
		c.Status(http.StatusNoContent)
	})
	request := httptest.NewRequest(http.MethodPut, "/api/v1/billing/orgs/org-1/account", nil)
	request.Header.Set("X-Service-Id", "org-core")
	request.Header.Set("X-Service-Token", "0123456789abcdef0123456789abcdef")
	request.Header.Set("X-User-Id", "victim")
	request.Header.Set("X-Org-Id", "org-2")
	request.Header.Set("X-User-Role", "owner")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusNoContent {
		t.Fatalf("machine request status=%d want=%d", response.Code, http.StatusNoContent)
	}
}

func TestBillingServiceDelegationV3CrossLanguageVector(t *testing.T) {
	bodyDigest := serviceDelegationBodyDigest([]byte(`{"plan":"pro"}`))
	if bodyDigest != "ApxA0uXOJFNQhvraZ-s-yFofgWfVqZ6reRfsBXYSbpk" {
		t.Fatalf("body digest=%q", bodyDigest)
	}
	signature := serviceDelegationSignature(billingServiceTestToken, serviceDelegationClaims{
		Principal:  "verevon-gateway",
		Audience:   "billing-core",
		Timestamp:  "2026-07-14T10:00:00Z",
		Nonce:      "nonce-0123456789abcdef",
		Method:     http.MethodPost,
		URI:        "/api/v1/billing/orgs/org-1/checkout-session?return=app",
		UserID:     "user-1",
		OrgID:      "org-1",
		UserRole:   "owner",
		BodySHA256: bodyDigest,
	})
	if signature != "3_0BRigS-8q6IIt_l4UV8OOEFtOG-1DCbMQrqnKeLMk" {
		t.Fatalf("cross-language signature=%q", signature)
	}
}

func ioNopCloserForTest(body []byte) *testReadCloser {
	return &testReadCloser{Reader: bytes.NewReader(body)}
}

type testReadCloser struct{ *bytes.Reader }

func (c *testReadCloser) Close() error { return nil }
