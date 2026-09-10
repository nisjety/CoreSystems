package http

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
)

const testServiceToken = "0123456789abcdef0123456789abcdef"

func signTestDelegation(request *http.Request, principal, userID, orgID, email, name, avatar string, body []byte, timestamp time.Time) {
	request.Header.Set("X-Service-Id", principal)
	request.Header.Set("X-Service-Token", testServiceToken)
	request.Header.Set("X-User-Id", userID)
	request.Header.Set("X-Org-Id", orgID)
	request.Header.Set("X-User-Email", email)
	request.Header.Set("X-User-Name", name)
	request.Header.Set("X-User-Avatar", avatar)
	request.Header.Set("X-Delegation-Timestamp", timestamp.UTC().Format(time.RFC3339))
	request.Header.Set("X-Delegation-Body-SHA256", serviceDelegationBodyDigest(body))
	request.Header.Set("X-Delegation-Signature", serviceDelegationSignature(testServiceToken, serviceDelegationClaims{
		Principal:  principal,
		Audience:   "user-core",
		Timestamp:  timestamp.UTC().Format(time.RFC3339),
		Method:     request.Method,
		URI:        request.URL.RequestURI(),
		UserID:     userID,
		OrgID:      orgID,
		Email:      email,
		Name:       name,
		Avatar:     avatar,
		BodySHA256: serviceDelegationBodyDigest(body),
	}))
}

func signTestAuthzDelegation(request *http.Request, principal, userID, orgID, operation, resourceType, resourceID, reason string, body []byte, timestamp time.Time) {
	bodyDigest := serviceDelegationBodyDigest(body)
	timestampValue := timestamp.UTC().Format(time.RFC3339)
	request.Header.Set("X-Service-Id", principal)
	request.Header.Set("X-Service-Token", testServiceToken)
	request.Header.Set("X-User-Id", userID)
	request.Header.Set("X-Org-Id", orgID)
	request.Header.Set("X-Delegation-Version", "v2")
	request.Header.Set("X-Delegation-Nonce", "test-nonce-1234567890")
	request.Header.Set("X-Delegation-Timestamp", timestampValue)
	request.Header.Set("X-Delegation-Operation", operation)
	request.Header.Set("X-Delegation-Resource-Type", resourceType)
	request.Header.Set("X-Delegation-Resource-Id", resourceID)
	request.Header.Set("X-Delegation-Reason", reason)
	request.Header.Set("X-Delegation-ZDR", "true")
	request.Header.Set("X-Delegation-Body-SHA256", bodyDigest)
	request.Header.Set("X-Delegation-Signature", serviceDelegationSignatureV2(testServiceToken, serviceDelegationV2Claims{
		Principal:    principal,
		Audience:     "user-core",
		Timestamp:    timestampValue,
		Method:       request.Method,
		URI:          request.URL.RequestURI(),
		UserID:       userID,
		OrgID:        orgID,
		Operation:    operation,
		ResourceType: resourceType,
		ResourceID:   resourceID,
		Reason:       reason,
		ZDR:          "true",
		Nonce:        "test-nonce-1234567890",
		BodySHA256:   bodyDigest,
	}))
}

func TestParseServiceCredentialsFailsClosed(t *testing.T) {
	tests := []struct {
		name    string
		raw     string
		wantLen int
		wantErr bool
	}{
		{name: "empty registry", raw: "", wantLen: 0},
		{name: "malformed json", raw: "{", wantErr: true},
		{name: "wrong audience", raw: `[{"principal":"svc","audience":"other","token":"0123456789abcdef0123456789abcdef","scopes":["users:read:any"]}]`, wantErr: true},
		{name: "empty scope", raw: `[{"principal":"svc","audience":"user-core","token":"0123456789abcdef0123456789abcdef","scopes":[""]}]`, wantErr: true},
		{name: "valid", raw: `[{"principal":"svc","audience":"user-core","token":"0123456789abcdef0123456789abcdef","scopes":["users:read:any"]}]`, wantLen: 1},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			credentials, err := parseServiceCredentials(test.raw)
			if test.wantErr {
				if err == nil {
					t.Fatal("expected validation error")
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if len(credentials) != test.wantLen {
				t.Fatalf("credential count = %d; want %d", len(credentials), test.wantLen)
			}
		})
	}
}

func TestValidateRequiredServiceCredentialRegistry(t *testing.T) {
	tests := []struct {
		name    string
		raw     string
		wantErr bool
	}{
		{name: "empty registry", raw: "", wantErr: true},
		{name: "malformed registry", raw: "{", wantErr: true},
		{name: "missing gateway", raw: `[{"principal":"session-core","audience":"user-core","token":"0123456789abcdef0123456789abcdef","scopes":["users:read:self"]}]`, wantErr: true},
		{name: "gateway missing write scope", raw: `[{"principal":"verevon-gateway","audience":"user-core","token":"0123456789abcdef0123456789abcdef","scopes":["users:read:self"]}]`, wantErr: true},
		{name: "gateway wrong audience", raw: `[{"principal":"verevon-gateway","audience":"other","token":"0123456789abcdef0123456789abcdef","scopes":["users:read:self","users:write:self"]}]`, wantErr: true},
		{name: "required gateway policy", raw: `[{"principal":"verevon-gateway","audience":"user-core","token":"0123456789abcdef0123456789abcdef","scopes":["users:read:self","users:write:self"]}]`},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			err := ValidateRequiredServiceCredentialRegistry(test.raw)
			if test.wantErr && err == nil {
				t.Fatal("expected validation error")
			}
			if !test.wantErr && err != nil {
				t.Fatalf("unexpected validation error: %v", err)
			}
		})
	}
}

func TestServiceScopeForRequest(t *testing.T) {
	tests := []struct {
		name   string
		method string
		path   string
		userID string
		want   string
	}{
		{name: "authz read", method: http.MethodGet, path: "/api/v1/internal/authz/visible", want: "authz:read"},
		{name: "authz write", method: http.MethodPost, path: "/api/v1/internal/authz/grant", want: "authz:write"},
		{name: "Space registration", method: http.MethodPost, path: "/api/v1/internal/spaces/register", want: "spaces:register"},
		{name: "Space deletion authorization", method: http.MethodPost, path: "/api/v1/internal/spaces/deletion-authorizations", want: "spaces:deletion:authorize"},
		{name: "Space deletion policy", method: http.MethodPut, path: "/api/v1/internal/spaces/deletion-policy", want: "spaces:policy:write"},
		{name: "Space legal hold", method: http.MethodPut, path: "/api/v1/internal/spaces/space-1/legal-hold", want: "spaces:policy:write"},
		{name: "Space recipient audience publication", method: http.MethodPost, path: "/api/v1/internal/spaces/recipient-audiences", want: "spaces:audience:publish"},
		{name: "Space membership resolution", method: http.MethodGet, path: "/api/v1/internal/spaces/space-1/membership", want: "spaces:resolve"},
		{name: "Space personal-thread decision", method: http.MethodPost, path: "/api/v1/internal/spaces/personal-thread-decision", want: "spaces:issue"},
		{name: "Space shared-thread decision", method: http.MethodPost, path: "/api/v1/internal/spaces/thread-decision", want: "spaces:issue"},
		{name: "Space thread append decision", method: http.MethodPost, path: "/api/v1/internal/spaces/thread-append-decision", want: "spaces:issue"},
		{name: "Space personal-retrieval decision", method: http.MethodPost, path: "/api/v1/internal/spaces/personal-retrieval-decision", want: "spaces:issue"},
		{name: "Space personal-import decision", method: http.MethodPost, path: "/api/v1/internal/spaces/personal-import-decision", want: "spaces:issue"},
		{name: "Space schedule-create decision", method: http.MethodPost, path: "/api/v1/internal/spaces/schedule-create-decision", want: "spaces:issue"},
		{name: "Space owner grant decision", method: http.MethodPost, path: "/api/v1/internal/spaces/owner-grant-decision", want: "spaces:issue"},
		{name: "Space import execution reauthorization", method: http.MethodPost, path: "/api/v1/internal/spaces/import-execution-decision", want: "spaces:import:reauthorize"},
		{name: "Space schedule fire reauthorization", method: http.MethodPost, path: "/api/v1/internal/spaces/schedule-fire-decision", want: "spaces:schedule:reauthorize"},
		{name: "Space scheduled run preparation", method: http.MethodPost, path: "/api/v1/internal/spaces/scheduled-run-decision", want: "spaces:schedule:reauthorize"},
		{name: "Space sandbox capability decision", method: http.MethodPost, path: "/api/v1/internal/spaces/sandbox-capability-decision", want: "spaces:sandbox:capability"},
		{name: "Space scheduled run execution", method: http.MethodPost, path: "/api/v1/internal/spaces/scheduled-run-execution-decision", want: "spaces:schedule:execute"},
		{name: "Space scheduled step execution", method: http.MethodPost, path: "/api/v1/internal/spaces/scheduled-step-decision", want: "spaces:schedule:step"},
		{name: "Space agent action model view", method: http.MethodPost, path: "/api/v1/internal/spaces/model-action-view", want: "spaces:agent-action:view"},
		{name: "Space agent action reauthorization", method: http.MethodPost, path: "/api/v1/internal/spaces/run-action-decision", want: "spaces:agent-action:reauthorize"},
		{name: "Space agent action current authority", method: http.MethodPost, path: "/api/v1/internal/spaces/run-action-authority-check", want: "spaces:agent-action:current-authority"},
		{name: "Owner effect reservation reserve", method: http.MethodPost, path: "/api/v1/internal/spaces/owner-effect-reservations/reserve", want: "spaces:agent-action:reservation"},
		{name: "Owner effect reservation commit", method: http.MethodPost, path: "/api/v1/internal/spaces/owner-effect-reservations/reservation_1/commit", want: "spaces:agent-action:reservation"},
		{name: "Owner effect reservation lookup", method: http.MethodGet, path: "/api/v1/internal/spaces/owner-effect-reservations/reservation_1", want: "spaces:agent-action:reservation"},
		{name: "Space effect policy", method: http.MethodPut, path: "/api/v1/internal/spaces/effect-policy", want: "spaces:policy:write"},
		{name: "membership write", method: http.MethodPost, path: "/api/v1/internal/memberships/ensure", want: "memberships:write"},
		{name: "user sync", method: http.MethodPost, path: "/api/v1/internal/users/enrich-from-provider", want: "users:sync"},
		{name: "read any", method: http.MethodGet, path: "/api/v1/users/id", want: "users:read:any"},
		{name: "read self requires delegation", method: http.MethodGet, path: "/api/v1/users/me", userID: "subject", want: "users:read:self"},
		{name: "write any", method: http.MethodPost, path: "/api/v1/users", want: "users:write:any"},
		{name: "write self requires delegation", method: http.MethodPatch, path: "/api/v1/users/me", userID: "subject", want: "users:write:self"},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			request := httptest.NewRequest(test.method, test.path, nil)
			if test.userID != "" {
				request.Header.Set("X-User-Id", test.userID)
			}
			if got := serviceScopeForRequest(request); got != test.want {
				t.Fatalf("scope = %q; want %q", got, test.want)
			}
		})
	}
}

func TestAuthenticateServicePrincipalRequiresBoundDelegation(t *testing.T) {
	credential := serviceCredential{
		Principal: "svc",
		Audience:  "user-core",
		Token:     testServiceToken,
		Scopes:    []string{"users:read:any", "users:read:self", "authz:read"},
	}

	tests := []struct {
		name           string
		principal      string
		token          string
		userID         string
		credentials    []serviceCredential
		wantPresent    bool
		wantAuthorized bool
	}{
		{name: "absent", credentials: []serviceCredential{credential}},
		{name: "partial credential", principal: "svc", credentials: []serviceCredential{credential}, wantPresent: true},
		{name: "unknown credential", principal: "other", token: testServiceToken, credentials: []serviceCredential{credential}, wantPresent: true},
		{name: "missing scope", principal: "svc", token: testServiceToken, credentials: []serviceCredential{{Principal: "svc", Token: testServiceToken, Scopes: []string{"users:write:any"}}}, wantPresent: true},
		{name: "unverified self delegation", principal: "svc", token: testServiceToken, userID: "victim", credentials: []serviceCredential{credential}, wantPresent: true},
		{name: "explicit non delegated scope", principal: "svc", token: testServiceToken, credentials: []serviceCredential{credential}, wantPresent: true, wantAuthorized: true},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			w := httptest.NewRecorder()
			c, _ := gin.CreateTestContext(w)
			c.Request = httptest.NewRequest(http.MethodGet, "/api/v1/users/id", nil)
			if test.principal != "" {
				c.Request.Header.Set("X-Service-Id", test.principal)
			}
			if test.token != "" {
				c.Request.Header.Set("X-Service-Token", test.token)
			}
			if test.userID != "" {
				c.Request.Header.Set("X-User-Id", test.userID)
			}

			present, authorized := authenticateServicePrincipal(c, test.credentials)
			if present != test.wantPresent || authorized != test.wantAuthorized {
				t.Fatalf("got present=%v authorized=%v; want present=%v authorized=%v", present, authorized, test.wantPresent, test.wantAuthorized)
			}
			if test.userID != "" && c.GetString("user_id") != "" {
				t.Fatal("unverified user header was copied into authenticated context")
			}
		})
	}
}

func TestAuthenticateServicePrincipalAcceptsOnlyValidSignedSelfDelegation(t *testing.T) {
	credential := serviceCredential{
		Principal: "verevon-gateway",
		Audience:  "user-core",
		Token:     testServiceToken,
		Scopes:    []string{"users:read:self", "users:write:self"},
	}

	t.Run("valid read binds verified subject and profile claims", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/users/me?view=profile", nil)
		signTestDelegation(request, credential.Principal, "user-1", "org-1", "verified@example.com", "Verified", "https://images.example.com/a.png", nil, time.Now())
		w := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(w)
		c.Request = request

		present, authorized := authenticateServicePrincipal(c, []serviceCredential{credential})
		if !present || !authorized {
			t.Fatalf("valid delegation got present=%v authorized=%v", present, authorized)
		}
		if c.GetString("user_id") != "user-1" || c.GetString("org_id") != "org-1" ||
			c.GetString("user_email") != "verified@example.com" || c.GetString("user_name") != "Verified" ||
			c.GetString("user_avatar") != "https://images.example.com/a.png" {
			t.Fatalf("signed claims were not bound to context")
		}
	})

	t.Run("body substitution is rejected", func(t *testing.T) {
		signedBody := []byte(`{"theme":"dark"}`)
		tamperedBody := []byte(`{"role":"admin"}`)
		request := httptest.NewRequest(http.MethodPatch, "/api/v1/preferences", bytes.NewReader(tamperedBody))
		signTestDelegation(request, credential.Principal, "user-1", "org-1", "verified@example.com", "Verified", "", signedBody, time.Now())
		w := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(w)
		c.Request = request

		present, authorized := authenticateServicePrincipal(c, []serviceCredential{credential})
		if !present || authorized {
			t.Fatalf("tampered body got present=%v authorized=%v", present, authorized)
		}
	})

	t.Run("expired signature is rejected", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/users/me", nil)
		signTestDelegation(request, credential.Principal, "user-1", "org-1", "verified@example.com", "Verified", "", nil, time.Now().Add(-2*time.Minute))
		w := httptest.NewRecorder()
		c, _ := gin.CreateTestContext(w)
		c.Request = request

		present, authorized := authenticateServicePrincipal(c, []serviceCredential{credential})
		if !present || authorized {
			t.Fatalf("expired delegation got present=%v authorized=%v", present, authorized)
		}
	})
}

func TestAuthenticateServicePrincipalAcceptsSignedAuthzReadDelegation(t *testing.T) {
	credential := serviceCredential{
		Principal: "retrieval-engine",
		Audience:  "user-core",
		Token:     testServiceToken,
		Scopes:    []string{"authz:read"},
	}
	request := httptest.NewRequest(http.MethodGet, "/api/v1/internal/authz/visible?org_id=org-1&resource_type=document&subject_id=user-1", nil)
	signTestAuthzDelegation(request, credential.Principal, "user-1", "org-1", "authz:visible", "document", "", "resolve explicit grants", nil, time.Now())
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = request

	present, authorized := authenticateServicePrincipal(c, []serviceCredential{credential})
	if !present || !authorized {
		t.Fatalf("valid authz delegation got present=%v authorized=%v", present, authorized)
	}
	if !c.GetBool("delegation_verified") || c.GetString("delegation_version") != "v2" || c.GetString("delegation_reason") != "resolve explicit grants" || c.GetString("user_id") != "user-1" || c.GetString("org_id") != "org-1" {
		t.Fatalf("verified authz claims were not pinned to context")
	}
}

func TestServiceDelegationCrossLanguageVector(t *testing.T) {
	body := []byte(`{"theme":"dark"}`)
	digest := serviceDelegationBodyDigest(body)
	if digest != "D0-H20VnIyp_F1aqFTTsExR3eznDv1IJ-Hz5c5Mhzdw" {
		t.Fatalf("body digest = %q", digest)
	}
	signature := serviceDelegationSignature(testServiceToken, serviceDelegationClaims{
		Principal:  "verevon-gateway",
		Audience:   "user-core",
		Timestamp:  "2026-07-11T02:00:00+00:00",
		Method:     http.MethodPatch,
		URI:        "/api/v1/preferences?view=all",
		UserID:     "user-1",
		OrgID:      "org-1",
		Email:      "verified@example.com",
		Name:       "Verified User",
		BodySHA256: digest,
	})
	if signature != "YscmqUN5kNSEAFIU2hafAF2UV87glPCdL_CnjII5V5k" {
		t.Fatalf("cross-language signature = %q", signature)
	}
}

func TestServiceDelegationV2CrossLanguageVector(t *testing.T) {
	digest := serviceDelegationBodyDigest(nil)
	signature := serviceDelegationSignatureV2(testServiceToken, serviceDelegationV2Claims{
		Principal:    "retrieval-engine",
		Audience:     "user-core",
		Timestamp:    "2026-07-11T02:00:00Z",
		Method:       http.MethodGet,
		URI:          "/api/v1/internal/authz/visible?org_id=org-1&subject_id=user-1&resource_type=document",
		UserID:       "user-1",
		OrgID:        "org-1",
		Operation:    "authz:visible",
		ResourceType: "document",
		Reason:       "resolve explicit document grants",
		ZDR:          "true",
		Nonce:        "test-nonce-1234567890",
		BodySHA256:   digest,
	})
	if signature != "ryYDD6IcyZ8bHaQcAuFrzXHj9cjbQ9O3qFA2Yni0cFM" {
		t.Fatalf("v2 cross-language signature = %q", signature)
	}
}

func TestServiceCredentialHelpers(t *testing.T) {
	credential := serviceCredential{Token: testServiceToken, Scopes: []string{" users:read:any "}}
	if !secureServiceTokenEqual(credential.Token, testServiceToken) || secureServiceTokenEqual(credential.Token, "wrong") {
		t.Fatal("constant-time token comparison returned an unexpected result")
	}
	if !credentialHasScope(credential, "users:read:any") || credentialHasScope(credential, "users:write:any") {
		t.Fatal("scope matching returned an unexpected result")
	}

	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	if hasServiceScope(c, "users:read:any") {
		t.Fatal("missing context scope must not authorize")
	}
	c.Set("service_scopes", "wrong-type")
	if hasServiceScope(c, "users:read:any") {
		t.Fatal("malformed context scope must not authorize")
	}
	c.Set("service_scopes", []string{"users:read:any"})
	if !hasServiceScope(c, "users:read:any") || hasServiceScope(c, "users:write:any") {
		t.Fatal("context scope matching returned an unexpected result")
	}
}
