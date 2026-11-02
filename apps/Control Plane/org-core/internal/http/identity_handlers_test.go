package http

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

// WS05-S6: the identity domain routes behind the REAL service-auth middleware, the REAL
// membership guard and the REAL gate, in the route shape setupRoutes uses. The route table
// itself is pinned by TestIdentityRoutesAreMountedAndGated. The same harness serves the SSO
// routes (sso_handlers_test.go).

const (
	idnTestOrg       = "org-idn-1"
	idnTestAuthToken = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	idnTestBillToken = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	idnTestRegistry  = `[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:identity:admin:self"]},
  {"principal":"auth-core","audience":"org-core","token":%q,"scopes":["org:domain:read:any"]},
  {"principal":"billing-core","audience":"org-core","token":%q,"scopes":["org:read:any"]}
]`
)

var idnTestRoles = map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member", "u-viewer": "viewer"}

func idnTestRegistryJSON() string {
	return fmt.Sprintf(idnTestRegistry, modelPolicyGatewayToken, idnTestAuthToken, idnTestBillToken)
}

// fakeIdentity is an identityDomainService that records calls and answers from canned values.
type fakeIdentity struct {
	mu       sync.Mutex
	err      error
	status   string // status of the domain returned by add
	calls    []string
	gotOrg   string
	gotActor string
	gotRole  string
	gotInput string
}

func (f *fakeIdentity) record(call, org, actor, role, input string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls = append(f.calls, call)
	f.gotOrg, f.gotActor, f.gotRole, f.gotInput = org, actor, role, input
}

func (f *fakeIdentity) callCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.calls)
}

func (f *fakeIdentity) ListIdentityDomains(_ context.Context, orgID string) ([]orgcore.IdentityDomain, error) {
	f.record("list", orgID, "", "", "")
	if f.err != nil {
		return nil, f.err
	}
	return []orgcore.IdentityDomain{{Domain: "example.com", Status: "pending"}}, nil
}

func (f *fakeIdentity) AddIdentityDomain(_ context.Context, orgID, actorID, actorRole, domain string) (*orgcore.IdentityDomain, error) {
	f.record("add", orgID, actorID, actorRole, domain)
	if f.err != nil {
		return nil, f.err
	}
	status := f.status
	if status == "" {
		status = "pending"
	}
	return &orgcore.IdentityDomain{Domain: strings.ToLower(domain), Status: status}, nil
}

func (f *fakeIdentity) VerifyIdentityDomain(_ context.Context, orgID, actorID, actorRole, domain string) (*orgcore.DomainVerifyResult, error) {
	f.record("verify", orgID, actorID, actorRole, domain)
	if f.err != nil {
		return nil, f.err
	}
	return &orgcore.DomainVerifyResult{Domain: domain, Status: "pending", Reason: orgcore.DomainReasonTXTNotFound}, nil
}

func (f *fakeIdentity) RevokeIdentityDomain(_ context.Context, orgID, actorID, actorRole, domain string) error {
	f.record("revoke", orgID, actorID, actorRole, domain)
	return f.err
}

func (f *fakeIdentity) LookupVerifiedDomainOwner(_ context.Context, domain string) (*orgcore.VerifiedDomainOwner, error) {
	f.record("lookup", "", "", "", domain)
	if f.err != nil {
		return nil, f.err
	}
	return &orgcore.VerifiedDomainOwner{OrgID: idnTestOrg, Domain: domain, Status: "verified", VerifiedAt: time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)}, nil
}

type identityHarness struct {
	router *gin.Engine
	fake   *fakeIdentity
	sso    *fakeSSO
}

func newIdentityHarness(t *testing.T, enabled bool, roles map[string]string) *identityHarness {
	t.Helper()
	credentials, err := parseServiceCredentials(idnTestRegistryJSON())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	fake := &fakeIdentity{}
	sso := &fakeSSO{}
	server := &Server{callerRoles: fakeCallerRoles{roles: roles}, identityEnabled: enabled}
	server.SetIdentityDomains(fake)
	server.SetIdentitySSO(sso)

	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	guard := requireActiveMembership(modelPolicyMembers(roles))
	gate := server.requireIdentityEnabled()
	router.GET("/orgs/:id/domains", gate, server.listIdentityDomains)
	router.POST("/orgs/:id/domains", gate, guard, server.addIdentityDomain)
	router.POST("/orgs/:id/domains/:domain/verify", gate, guard, server.verifyIdentityDomain)
	router.DELETE("/orgs/:id/domains/:domain", gate, guard, server.revokeIdentityDomain)
	router.GET("/orgs/:id/sso/policy", gate, server.getSSOPolicy)
	router.PUT("/orgs/:id/sso/policy", gate, guard, server.putSSOPolicy)
	router.POST("/orgs/:id/sso/providers", gate, guard, server.addSSOProvider)
	router.DELETE("/orgs/:id/sso/providers/:providerId", gate, guard, server.removeSSOProvider)
	router.GET("/internal/orgs/by-domain/:domain", gate, server.getVerifiedDomainOwnerInternal)
	router.GET("/internal/orgs/by-domain/:domain/sso-policy", gate, server.getSSOPolicyByDomainInternal)
	return &identityHarness{router: router, fake: fake, sso: sso}
}

func (h *identityHarness) asUser(method, path, userID, platformRole string, body []byte) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	if len(body) > 0 {
		request.Header.Set("Content-Type", "application/json")
	}
	signModelPolicyDelegation(request, body, userID, idnTestOrg, platformRole)
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

func (h *identityHarness) asService(principal, token, path string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodGet, path, nil)
	request.Header.Set("X-Service-Id", principal)
	request.Header.Set("X-Service-Token", token)
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

const (
	idnDomainsPath = "/orgs/" + idnTestOrg + "/domains"
	idnByDomain    = "/internal/orgs/by-domain/example.com"
)

type idnRoute struct {
	method, path string
	body         []byte
}

func idnDomainRoutes() []idnRoute {
	return []idnRoute{
		{http.MethodGet, idnDomainsPath, nil},
		{http.MethodPost, idnDomainsPath, []byte(`{"domain":"example.com"}`)},
		{http.MethodPost, idnDomainsPath + "/example.com/verify", nil},
		{http.MethodDelete, idnDomainsPath + "/example.com", nil},
	}
}

func envelopeCodeAndDetails(t *testing.T, response *httptest.ResponseRecorder) (string, map[string]any) {
	t.Helper()
	failure, _ := decodeBody(t, response)["error"].(map[string]any)
	if failure == nil {
		t.Fatalf("no typed error envelope in %s", response.Body.String())
	}
	code, _ := failure["code"].(string)
	if msg, _ := failure["message"].(string); msg == "" {
		t.Fatalf("envelope has no message: %v", failure)
	}
	details, ok := failure["details"].(map[string]any)
	if !ok {
		t.Fatalf("envelope has no details object: %v", failure)
	}
	return code, details
}

func TestIdentityFlagIsStrict(t *testing.T) {
	for value, want := range map[string]bool{
		"1": true, "true": true,
		"": false, "0": false, "false": false, "TRUE": false, "True": false, "yes": false, "on": false, " 1": false, "1 ": false, "2": false,
	} {
		if got := domainVerificationEnabled(func(string) string { return value }); got != want {
			t.Errorf("flag %q = %v, want %v", value, got, want)
		}
	}
	if resolvers := domainDNSResolvers(func(string) string { return "" }); resolvers != nil {
		t.Fatalf("unset resolvers must be nil (system resolvers), got %v", resolvers)
	}
	if resolvers := domainDNSResolvers(func(string) string { return "1.1.1.1, 9.9.9.9:53" }); len(resolvers) != 2 {
		t.Fatalf("resolvers = %v", resolvers)
	}
	if got := authCoreIdentityURL(func(string) string { return "" }); got != "http://auth-core:3011" {
		t.Fatalf("default auth-core url = %q", got)
	}
	if got := authCoreIdentityURL(func(string) string { return " http://x:1 " }); got != "http://x:1" {
		t.Fatalf("auth-core url = %q", got)
	}
}

func TestIdentityRoutesAreMountedAndGated(t *testing.T) {
	t.Setenv(serviceCredentialEnv, idnTestRegistryJSON())
	t.Setenv(domainVerificationFlagEnv, "")
	off := NewServer(0, nil, nil, "", "")
	if off.identityEnabled {
		t.Fatal("the identity routes are on by default")
	}
	want := map[string]bool{
		"GET /orgs/:id/domains":                           false,
		"POST /orgs/:id/domains":                          false,
		"POST /orgs/:id/domains/:domain/verify":           false,
		"DELETE /orgs/:id/domains/:domain":                false,
		"GET /orgs/:id/sso/policy":                        false,
		"PUT /orgs/:id/sso/policy":                        false,
		"POST /orgs/:id/sso/providers":                    false,
		"DELETE /orgs/:id/sso/providers/:providerId":      false,
		"GET /internal/orgs/by-domain/:domain":            false,
		"GET /internal/orgs/by-domain/:domain/sso-policy": false,
	}
	for _, route := range off.router.Routes() {
		if _, ok := want[route.Method+" "+route.Path]; ok {
			want[route.Method+" "+route.Path] = true
		}
	}
	for key, mounted := range want {
		if !mounted {
			t.Errorf("%s is not registered", key)
		}
	}

	// Off: even a caller holding the scope gets 404 on the by-domain read.
	request := httptest.NewRequest(http.MethodGet, idnByDomain, nil)
	request.Header.Set("X-Service-Id", "auth-core")
	request.Header.Set("X-Service-Token", idnTestAuthToken)
	response := httptest.NewRecorder()
	off.router.ServeHTTP(response, request)
	if response.Code != http.StatusNotFound {
		t.Fatalf("flag off, by-domain read: status=%d body=%s; want 404", response.Code, response.Body.String())
	}

	t.Setenv(domainVerificationFlagEnv, "1")
	if !NewServer(0, nil, nil, "", "").identityEnabled {
		t.Fatal("ORG_DOMAIN_VERIFICATION_ENABLED=1 did not switch the routes on")
	}
}

func TestIdentityGateAnswers404ForEveryRouteWhenOff(t *testing.T) {
	h := newIdentityHarness(t, false, idnTestRoles)
	for _, tc := range idnDomainRoutes() {
		response := h.asUser(tc.method, tc.path, "u-owner", "", tc.body)
		if response.Code != http.StatusNotFound {
			t.Errorf("%s %s with the flag off: status=%d; want 404", tc.method, tc.path, response.Code)
		}
	}
	if response := h.asService("auth-core", idnTestAuthToken, idnByDomain); response.Code != http.StatusNotFound {
		t.Errorf("by-domain read with the flag off: status=%d; want 404", response.Code)
	}
	if h.fake.callCount() != 0 {
		t.Fatalf("the service was reached %d times with the flag off", h.fake.callCount())
	}
}

func TestOnlyOwnerAndAdminMayAdministerIdentityDomains(t *testing.T) {
	h := newIdentityHarness(t, true, idnTestRoles)

	// A member, a viewer and a stranger are refused whatever role the gateway forwarded,
	// and a platform admin who is only a member of this org is not an org admin.
	for _, user := range []string{"u-member", "u-viewer", "u-stranger"} {
		for _, forwarded := range []string{"", "admin", "owner", "superadmin"} {
			for _, tc := range idnDomainRoutes() {
				response := h.asUser(tc.method, tc.path, user, forwarded, tc.body)
				if response.Code != http.StatusForbidden {
					t.Fatalf("%s %s as %s (forwarded %q): status=%d body=%s; want 403", tc.method, tc.path, user, forwarded, response.Code, response.Body.String())
				}
			}
		}
	}
	if h.fake.callCount() != 0 {
		t.Fatalf("a refused caller reached the service %d times", h.fake.callCount())
	}

	// The role the handler checks is from org-core's table, and the typed code names the gap.
	response := h.asUser(http.MethodGet, idnDomainsPath, "u-member", "admin", nil)
	if code, _ := envelopeCodeAndDetails(t, response); code != "identity_admin_required" {
		t.Fatalf("member read: code %q, want identity_admin_required", code)
	}

	for _, user := range []string{"u-owner", "u-admin"} {
		for _, tc := range idnDomainRoutes() {
			response := h.asUser(tc.method, tc.path, user, "", tc.body)
			if response.Code >= 300 {
				t.Fatalf("%s %s as %s: status=%d body=%s", tc.method, tc.path, user, response.Code, response.Body.String())
			}
			if got := response.Header().Get("Cache-Control"); got != "no-store" {
				t.Errorf("%s %s Cache-Control = %q, want no-store", tc.method, tc.path, got)
			}
			if h.fake.gotActor != "" && (h.fake.gotActor != user || h.fake.gotRole != idnTestRoles[user]) {
				t.Fatalf("service saw actor=%q role=%q; want the membership-table identity of %s", h.fake.gotActor, h.fake.gotRole, user)
			}
			if h.fake.gotOrg != idnTestOrg {
				t.Fatalf("service saw org %q", h.fake.gotOrg)
			}
		}
	}
}

func TestIdentityDomainSuccessShapes(t *testing.T) {
	h := newIdentityHarness(t, true, idnTestRoles)

	list := h.asUser(http.MethodGet, idnDomainsPath, "u-owner", "", nil)
	data, _ := decodeBody(t, list)["data"].(map[string]any)
	if domains, _ := data["domains"].([]any); len(domains) != 1 {
		t.Fatalf("list data: %v", data)
	}

	add := h.asUser(http.MethodPost, idnDomainsPath, "u-admin", "", []byte(`{"domain":"Example.COM"}`))
	if add.Code != http.StatusCreated {
		t.Fatalf("add pending: status=%d", add.Code)
	}
	if h.fake.gotInput != "Example.COM" {
		t.Fatalf("the raw domain goes to the service, which normalizes: %q", h.fake.gotInput)
	}
	h.fake.status = "verified"
	if again := h.asUser(http.MethodPost, idnDomainsPath, "u-admin", "", []byte(`{"domain":"example.com"}`)); again.Code != http.StatusOK {
		t.Fatalf("add of an already verified domain: status=%d, want 200", again.Code)
	}

	verify := h.asUser(http.MethodPost, idnDomainsPath+"/example.com/verify", "u-admin", "", nil)
	vdata, _ := decodeBody(t, verify)["data"].(map[string]any)
	if verify.Code != http.StatusOK || vdata["status"] != "pending" || vdata["reason"] != orgcore.DomainReasonTXTNotFound {
		t.Fatalf("verify: status=%d data=%v", verify.Code, vdata)
	}

	revoke := h.asUser(http.MethodDelete, idnDomainsPath+"/example.com", "u-admin", "", nil)
	rdata, _ := decodeBody(t, revoke)["data"].(map[string]any)
	if revoke.Code != http.StatusOK || rdata["status"] != "revoked" {
		t.Fatalf("revoke: status=%d data=%v", revoke.Code, rdata)
	}
}

func TestIdentityDomainErrorsAreTypedEnvelopes(t *testing.T) {
	cases := []struct {
		name       string
		err        error
		status     int
		code       string
		detailKeys map[string]any
	}{
		{"invalid domain", &orgcore.DomainValidationError{Reason: "free_mail"}, http.StatusBadRequest, "invalid_domain", map[string]any{"reason": "free_mail"}},
		{"verified elsewhere", orgcore.ErrDomainVerifiedElsewhere, http.StatusConflict, "domain_verified_elsewhere", nil},
		{"limit reached", orgcore.ErrDomainLimitReached, http.StatusConflict, "domain_limit_reached", map[string]any{"max": float64(orgcore.MaxIdentityDomainsPerOrg)}},
		{"in use", orgcore.ErrDomainInUse, http.StatusConflict, "domain_in_use", nil},
		{"not found", orgcore.ErrDomainNotFound, http.StatusNotFound, "domain_not_found", nil},
		{"unavailable", orgcore.ErrDomainVerificationUnavailable, http.StatusServiceUnavailable, "identity_unavailable", nil},
		{"org not found", orgcore.ErrNotFound, http.StatusNotFound, "organization_not_found", nil},
		{"wrapped", fmt.Errorf("add: %w", orgcore.ErrDomainInUse), http.StatusConflict, "domain_in_use", nil},
		{"unexpected", errors.New("pq: connection reset by peer, password=hunter2"), http.StatusInternalServerError, "internal_error", nil},
	}
	for _, tc := range cases {
		for _, route := range idnDomainRoutes() {
			h := newIdentityHarness(t, true, idnTestRoles)
			h.fake.err = tc.err
			response := h.asUser(route.method, route.path, "u-owner", "", route.body)
			if response.Code != tc.status {
				t.Fatalf("%s on %s %s: status=%d body=%s; want %d", tc.name, route.method, route.path, response.Code, response.Body.String(), tc.status)
			}
			code, details := envelopeCodeAndDetails(t, response)
			if code != tc.code {
				t.Fatalf("%s: code %q, want %q", tc.name, code, tc.code)
			}
			for key, want := range tc.detailKeys {
				if details[key] != want {
					t.Fatalf("%s: details[%s] = %v, want %v", tc.name, key, details[key], want)
				}
			}
			if strings.Contains(response.Body.String(), "hunter2") {
				t.Fatalf("%s: an unexpected error leaked its text: %s", tc.name, response.Body.String())
			}
		}
	}
}

func TestIdentityAddRefusesMalformedBodiesBeforeTheService(t *testing.T) {
	h := newIdentityHarness(t, true, idnTestRoles)
	huge := []byte(`{"domain":"` + strings.Repeat("a", 5000) + `.com"}`)
	for name, body := range map[string][]byte{
		"unknown field": []byte(`{"domain":"example.com","org_id":"org-2"}`),
		"trailing data": []byte(`{"domain":"example.com"} {}`),
		"not json":      []byte(`domain=example.com`),
		"array":         []byte(`["example.com"]`),
		"oversized":     huge,
	} {
		response := h.asUser(http.MethodPost, idnDomainsPath, "u-owner", "", body)
		if response.Code != http.StatusBadRequest {
			t.Fatalf("%s: status=%d body=%s; want 400", name, response.Code, response.Body.String())
		}
		if code, _ := envelopeCodeAndDetails(t, response); code != "invalid_request" {
			t.Fatalf("%s: code %q", name, code)
		}
	}
	if h.fake.callCount() != 0 {
		t.Fatalf("a malformed body reached the service %d times", h.fake.callCount())
	}
}

func TestIdentityRoutesFailClosedWhenTheRoleCannotBeResolved(t *testing.T) {
	credentials, err := parseServiceCredentials(idnTestRegistryJSON())
	if err != nil {
		t.Fatal(err)
	}
	for name, server := range map[string]*Server{
		"role lookup error": {callerRoles: fakeCallerRoles{err: errors.New("db down")}, identityEnabled: true},
		"no role reader":    {identityEnabled: true},
	} {
		server.SetIdentityDomains(&fakeIdentity{})
		router := gin.New()
		router.Use(serviceAuthMiddleware(credentials, time.Now))
		router.GET("/orgs/:id/domains", server.requireIdentityEnabled(), server.listIdentityDomains)
		request := httptest.NewRequest(http.MethodGet, idnDomainsPath, nil)
		signModelPolicyDelegation(request, nil, "u-owner", idnTestOrg, "")
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		if response.Code != http.StatusInternalServerError && response.Code != http.StatusServiceUnavailable {
			t.Fatalf("%s: status=%d body=%s; want 500/503, never 200", name, response.Code, response.Body.String())
		}
	}

	// A server wired without the domain service answers 503 rather than serving nothing.
	server := &Server{callerRoles: fakeCallerRoles{roles: idnTestRoles}, identityEnabled: true}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	router.GET("/orgs/:id/domains", server.requireIdentityEnabled(), server.listIdentityDomains)
	request := httptest.NewRequest(http.MethodGet, idnDomainsPath, nil)
	signModelPolicyDelegation(request, nil, "u-owner", idnTestOrg, "")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("no service: status=%d, want 503", response.Code)
	}
}

func TestByDomainReadNeedsTheAuthCoreOnlyScope(t *testing.T) {
	h := newIdentityHarness(t, true, idnTestRoles)

	// auth-core holds org:domain:read:any.
	ok := h.asService("auth-core", idnTestAuthToken, idnByDomain)
	data, _ := decodeBody(t, ok)["data"].(map[string]any)
	if ok.Code != http.StatusOK || data["org_id"] != idnTestOrg || data["status"] != "verified" {
		t.Fatalf("auth-core by-domain: status=%d body=%s", ok.Code, ok.Body.String())
	}
	if got := ok.Header().Get("Cache-Control"); got != "no-store" {
		t.Fatalf("Cache-Control = %q, want no-store", got)
	}
	if h.fake.gotInput != "example.com" {
		t.Fatalf("service saw domain %q", h.fake.gotInput)
	}
	before := h.fake.callCount()

	// org:read:any does not reach it, and neither does the gateway's signed-user scope.
	if r := h.asService("billing-core", idnTestBillToken, idnByDomain); r.Code != http.StatusForbidden {
		t.Fatalf("billing-core by-domain: status=%d, want 403", r.Code)
	}
	if r := h.asUser(http.MethodGet, idnByDomain, "u-owner", "", nil); r.Code != http.StatusForbidden && r.Code != http.StatusUnauthorized {
		t.Fatalf("gateway-signed by-domain: status=%d, want 401/403", r.Code)
	}
	if r := h.asService("auth-core", "wrong-token-wrong-token-wrong-token-wrong", idnByDomain); r.Code != http.StatusUnauthorized && r.Code != http.StatusForbidden {
		t.Fatalf("wrong token: status=%d", r.Code)
	}
	if h.fake.callCount() != before {
		t.Fatal("a refused by-domain read reached the service")
	}

	// A claim that is not verified is a 404 typed envelope; nothing about other orgs leaks.
	h.fake.err = orgcore.ErrDomainNotFound
	notFound := h.asService("auth-core", idnTestAuthToken, idnByDomain)
	if code, _ := envelopeCodeAndDetails(t, notFound); notFound.Code != http.StatusNotFound || code != "domain_not_found" {
		t.Fatalf("not verified: status=%d code=%q", notFound.Code, code)
	}
}
