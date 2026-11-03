package http

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"log"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"sync"
	"testing"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
)

// WS05-S7: the SSO policy and provider routes. They share the harness, gate and authority of
// the domain routes (identity_handlers_test.go).

const (
	ssoPolicyPath    = "/orgs/" + idnTestOrg + "/sso/policy"
	ssoProvidersPath = "/orgs/" + idnTestOrg + "/sso/providers"
	ssoByDomainPath  = "/internal/orgs/by-domain/example.com/sso-policy"
	ssoTestSecret    = "client-secret-ssssssssssssssssssssssssss"
)

type fakeSSO struct {
	mu        sync.Mutex
	err       error
	calls     []string
	gotOrg    string
	gotActor  string
	gotRole   string
	gotPolicy orgcore.SSOPolicyInput
	gotProv   orgcore.SSOProviderInput
	gotID     string
	gotDomain string
}

func (f *fakeSSO) callCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.calls)
}

func (f *fakeSSO) GetSSOPolicy(_ context.Context, orgID, actorID string) (*orgcore.SSOPolicyView, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls, f.gotOrg, f.gotActor = append(f.calls, "get"), orgID, actorID
	if f.err != nil {
		return nil, f.err
	}
	return &orgcore.SSOPolicyView{
		Mode: "report", Revision: 3, Domains: []string{"example.com"}, BreakGlassUserIDs: []string{},
		Providers:  []orgcore.SSOProvider{{ProviderID: "acme-oidc", Domain: "example.com", Type: "oidc", DomainVerified: true}},
		CanEnforce: orgcore.SSOCanEnforce{OK: false, Reasons: []string{orgcore.SSOReasonNotProven}},
	}, nil
}

func (f *fakeSSO) PutSSOPolicy(_ context.Context, orgID, actorID, actorRole string, in orgcore.SSOPolicyInput) (*orgcore.SSOPolicyView, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls, f.gotOrg, f.gotActor, f.gotRole, f.gotPolicy = append(f.calls, "put"), orgID, actorID, actorRole, in
	if f.err != nil {
		return nil, f.err
	}
	return &orgcore.SSOPolicyView{Mode: in.Mode, Revision: in.ExpectedRevision + 1, Domains: []string{}, BreakGlassUserIDs: in.BreakGlassUserIDs, Providers: []orgcore.SSOProvider{}}, nil
}

func (f *fakeSSO) AddSSOProvider(_ context.Context, orgID, actorID, actorRole string, in orgcore.SSOProviderInput) (*orgcore.SSOProvider, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls, f.gotOrg, f.gotActor, f.gotRole, f.gotProv = append(f.calls, "add"), orgID, actorID, actorRole, in
	if f.err != nil {
		return nil, f.err
	}
	return &orgcore.SSOProvider{ProviderID: in.ProviderID, Domain: in.Domain, Issuer: in.Issuer, Type: in.Type, DomainVerified: true}, nil
}

func (f *fakeSSO) RemoveSSOProvider(_ context.Context, orgID, actorID, actorRole, providerID string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls, f.gotOrg, f.gotActor, f.gotRole, f.gotID = append(f.calls, "remove"), orgID, actorID, actorRole, providerID
	return f.err
}

func (f *fakeSSO) LookupSSOPolicyByDomain(_ context.Context, domain string) (*orgcore.SSOPolicyByDomain, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.calls, f.gotDomain = append(f.calls, "by-domain"), domain
	if f.err != nil {
		return nil, f.err
	}
	return &orgcore.SSOPolicyByDomain{OrgID: idnTestOrg, Domain: domain, Mode: "enforce", BreakGlassUserIDs: []string{"u-owner"}, Revision: 7}, nil
}

func ssoProviderBody() []byte {
	return []byte(fmt.Sprintf(`{"provider_id":"acme-oidc","domain":"example.com","type":"oidc","issuer":"https://idp.example.com","client_id":"client-1","client_secret":%q}`, ssoTestSecret))
}

func ssoRoutes() []idnRoute {
	return []idnRoute{
		{http.MethodGet, ssoPolicyPath, nil},
		{http.MethodPut, ssoPolicyPath, []byte(`{"mode":"report","break_glass_user_ids":[],"expected_revision":1}`)},
		{http.MethodPost, ssoProvidersPath, ssoProviderBody()},
		{http.MethodDelete, ssoProvidersPath + "/acme-oidc", nil},
	}
}

func TestSSOGateAnswers404ForEveryRouteWhenOff(t *testing.T) {
	h := newIdentityHarness(t, false, idnTestRoles)
	for _, tc := range ssoRoutes() {
		if response := h.asUser(tc.method, tc.path, "u-owner", "", tc.body); response.Code != http.StatusNotFound {
			t.Errorf("%s %s with the flag off: status=%d; want 404", tc.method, tc.path, response.Code)
		}
	}
	if response := h.asService("auth-core", idnTestAuthToken, ssoByDomainPath); response.Code != http.StatusNotFound {
		t.Errorf("by-domain sso-policy with the flag off: status=%d; want 404", response.Code)
	}
	if h.sso.callCount() != 0 {
		t.Fatalf("the SSO service was reached %d times with the flag off", h.sso.callCount())
	}
}

func TestOnlyOwnerAndAdminMayAdministerSSO(t *testing.T) {
	h := newIdentityHarness(t, true, idnTestRoles)
	for _, user := range []string{"u-member", "u-viewer", "u-stranger"} {
		for _, forwarded := range []string{"", "admin", "owner"} {
			for _, tc := range ssoRoutes() {
				response := h.asUser(tc.method, tc.path, user, forwarded, tc.body)
				if response.Code != http.StatusForbidden {
					t.Fatalf("%s %s as %s (forwarded %q): status=%d body=%s; want 403", tc.method, tc.path, user, forwarded, response.Code, response.Body.String())
				}
			}
		}
	}
	if h.sso.callCount() != 0 {
		t.Fatalf("a refused caller reached the SSO service %d times", h.sso.callCount())
	}
	if code, _ := envelopeCodeAndDetails(t, h.asUser(http.MethodGet, ssoPolicyPath, "u-member", "admin", nil)); code != "identity_admin_required" {
		t.Fatalf("member policy read: code %q, want identity_admin_required", code)
	}

	for _, user := range []string{"u-owner", "u-admin"} {
		for _, tc := range ssoRoutes() {
			response := h.asUser(tc.method, tc.path, user, "", tc.body)
			if response.Code >= 300 {
				t.Fatalf("%s %s as %s: status=%d body=%s", tc.method, tc.path, user, response.Code, response.Body.String())
			}
			if got := response.Header().Get("Cache-Control"); got != "no-store" {
				t.Errorf("%s %s Cache-Control = %q, want no-store", tc.method, tc.path, got)
			}
		}
		if h.sso.gotActor != user || h.sso.gotRole != idnTestRoles[user] || h.sso.gotOrg != idnTestOrg {
			t.Fatalf("service saw org=%q actor=%q role=%q for %s", h.sso.gotOrg, h.sso.gotActor, h.sso.gotRole, user)
		}
	}
}

func TestSSOPutPolicyNormalizesTheModeAndRequiresARevision(t *testing.T) {
	h := newIdentityHarness(t, true, idnTestRoles)
	response := h.asUser(http.MethodPut, ssoPolicyPath, "u-owner", "", []byte(`{"mode":"  ENFORCE ","break_glass_user_ids":["u-owner","u-admin"],"expected_revision":4}`))
	if response.Code != http.StatusOK {
		t.Fatalf("put: status=%d body=%s", response.Code, response.Body.String())
	}
	if h.sso.gotPolicy.Mode != "enforce" || h.sso.gotPolicy.ExpectedRevision != 4 || len(h.sso.gotPolicy.BreakGlassUserIDs) != 2 {
		t.Fatalf("service saw %+v", h.sso.gotPolicy)
	}
	data, _ := decodeBody(t, response)["data"].(map[string]any)
	if data["revision"] != float64(5) {
		t.Fatalf("view: %v", data)
	}

	// expected_revision 0 is a valid compare value; absent is not.
	if r := h.asUser(http.MethodPut, ssoPolicyPath, "u-owner", "", []byte(`{"mode":"off","expected_revision":0}`)); r.Code != http.StatusOK {
		t.Fatalf("revision 0: status=%d body=%s", r.Code, r.Body.String())
	}
	calls := h.sso.callCount()
	for name, body := range map[string][]byte{
		"missing revision": []byte(`{"mode":"report"}`),
		"unknown field":    []byte(`{"mode":"report","expected_revision":1,"org_id":"org-2"}`),
		"not json":         []byte(`mode=report`),
		"trailing data":    []byte(`{"mode":"report","expected_revision":1} {}`),
		"string revision":  []byte(`{"mode":"report","expected_revision":"1"}`),
	} {
		r := h.asUser(http.MethodPut, ssoPolicyPath, "u-owner", "", body)
		if r.Code != http.StatusBadRequest {
			t.Fatalf("%s: status=%d body=%s; want 400", name, r.Code, r.Body.String())
		}
		if code, _ := envelopeCodeAndDetails(t, r); code != "invalid_request" {
			t.Fatalf("%s: code %q", name, code)
		}
	}
	if h.sso.callCount() != calls {
		t.Fatal("a malformed policy body reached the service")
	}
}

func TestSSOErrorsAreTypedEnvelopes(t *testing.T) {
	cases := []struct {
		name    string
		err     error
		status  int
		code    string
		details map[string]any
	}{
		{"bad request", &orgcore.SSORequestError{Reason: "mode"}, http.StatusBadRequest, "invalid_request", map[string]any{"reason": "mode"}},
		{"enforce no provider", &orgcore.SSOEnforceError{Code: orgcore.SSOReasonNoProvider}, http.StatusUnprocessableEntity, "no_provider", nil},
		{"enforce not proven", &orgcore.SSOEnforceError{Code: orgcore.SSOReasonNotProven}, http.StatusUnprocessableEntity, "sso_not_proven", nil},
		{"enforce break glass 2fa", &orgcore.SSOEnforceError{Code: orgcore.SSOReasonBreakGlass2FA}, http.StatusUnprocessableEntity, "break_glass_requires_2fa", nil},
		{"enforce no domain", &orgcore.SSOEnforceError{Code: orgcore.SSOReasonNoVerifiedDomain}, http.StatusUnprocessableEntity, "no_verified_domain", nil},
		{"revision conflict", orgcore.ErrSSOPolicyRevisionConflict, http.StatusConflict, "policy_revision_conflict", nil},
		{"provider exists", orgcore.ErrSSOProviderExists, http.StatusConflict, "provider_exists", nil},
		{"invalid issuer", orgcore.ErrSSOInvalidIssuer, http.StatusBadRequest, "invalid_issuer", nil},
		{"domain not verified", orgcore.ErrSSODomainNotVerified, http.StatusUnprocessableEntity, "domain_not_verified", nil},
		{"last provider", orgcore.ErrSSOLastProviderEnforced, http.StatusConflict, "last_provider_enforced", nil},
		{"provider not found", orgcore.ErrSSOProviderNotFound, http.StatusNotFound, "provider_not_found", nil},
		{"sso unavailable", orgcore.ErrSSOUnavailable, http.StatusServiceUnavailable, "identity_unavailable", nil},
		{"domains unavailable", orgcore.ErrDomainVerificationUnavailable, http.StatusServiceUnavailable, "identity_unavailable", nil},
		{"org not found", orgcore.ErrNotFound, http.StatusNotFound, "organization_not_found", nil},
		{"wrapped conflict", fmt.Errorf("put: %w", orgcore.ErrSSOPolicyRevisionConflict), http.StatusConflict, "policy_revision_conflict", nil},
		{"unexpected", errors.New("pq: broken pipe secret=hunter2"), http.StatusInternalServerError, "internal_error", nil},
	}
	for _, tc := range cases {
		for _, route := range ssoRoutes() {
			h := newIdentityHarness(t, true, idnTestRoles)
			h.sso.err = tc.err
			response := h.asUser(route.method, route.path, "u-owner", "", route.body)
			if response.Code != tc.status {
				t.Fatalf("%s on %s %s: status=%d body=%s; want %d", tc.name, route.method, route.path, response.Code, response.Body.String(), tc.status)
			}
			code, details := envelopeCodeAndDetails(t, response)
			if code != tc.code {
				t.Fatalf("%s: code %q, want %q", tc.name, code, tc.code)
			}
			for key, want := range tc.details {
				if details[key] != want {
					t.Fatalf("%s: details[%s] = %v, want %v", tc.name, key, details[key], want)
				}
			}
			if strings.Contains(response.Body.String(), "hunter2") {
				t.Fatalf("%s: an unexpected error leaked its text", tc.name)
			}
		}
	}
}

func TestSSOProviderSecretIsWriteOnlyAndNeverLogged(t *testing.T) {
	var logs bytes.Buffer
	log.SetOutput(&logs)
	defer log.SetOutput(os.Stderr)

	h := newIdentityHarness(t, true, idnTestRoles)
	add := h.asUser(http.MethodPost, ssoProvidersPath, "u-owner", "", ssoProviderBody())
	if add.Code != http.StatusCreated {
		t.Fatalf("add: status=%d body=%s", add.Code, add.Body.String())
	}
	if h.sso.gotProv.ClientSecret != ssoTestSecret || h.sso.gotProv.ProviderID != "acme-oidc" || h.sso.gotProv.Issuer != "https://idp.example.com" {
		t.Fatalf("the service must receive the whole input: %+v", h.sso.gotProv)
	}
	remove := h.asUser(http.MethodDelete, ssoProvidersPath+"/Acme-OIDC", "u-owner", "", nil)
	get := h.asUser(http.MethodGet, ssoPolicyPath, "u-owner", "", nil)
	for name, response := range map[string]*httptest.ResponseRecorder{"add": add, "remove": remove, "get": get} {
		if strings.Contains(response.Body.String(), ssoTestSecret) {
			t.Fatalf("%s response carries the client secret: %s", name, response.Body.String())
		}
	}
	data, _ := decodeBody(t, add)["data"].(map[string]any)
	if _, present := data["client_secret"]; present {
		t.Fatalf("add response has a client_secret field: %v", data)
	}
	if strings.Contains(logs.String(), ssoTestSecret) {
		t.Fatalf("the request log carries the client secret:\n%s", logs.String())
	}
}

func TestSSOProviderBodyRefusesUnknownAndMalformedFields(t *testing.T) {
	h := newIdentityHarness(t, true, idnTestRoles)
	for name, body := range map[string][]byte{
		"unknown field": []byte(`{"provider_id":"acme-oidc","domain":"example.com","type":"oidc","issuer":"https://idp.example.com","client_id":"c","client_secret":"s","org_id":"org-2"}`),
		"not json":      []byte(`provider=acme`),
		"oversized":     []byte(`{"provider_id":"` + strings.Repeat("a", 5000) + `"}`),
	} {
		r := h.asUser(http.MethodPost, ssoProvidersPath, "u-owner", "", body)
		if r.Code != http.StatusBadRequest {
			t.Fatalf("%s: status=%d; want 400", name, r.Code)
		}
	}
	if h.sso.callCount() != 0 {
		t.Fatal("a malformed provider body reached the service")
	}
}

func TestSSOPolicyByDomainNeedsTheAuthCoreOnlyScope(t *testing.T) {
	h := newIdentityHarness(t, true, idnTestRoles)
	ok := h.asService("auth-core", idnTestAuthToken, ssoByDomainPath)
	data, _ := decodeBody(t, ok)["data"].(map[string]any)
	if ok.Code != http.StatusOK || data["mode"] != "enforce" || data["revision"] != float64(7) || data["org_id"] != idnTestOrg {
		t.Fatalf("auth-core: status=%d body=%s", ok.Code, ok.Body.String())
	}
	if got := ok.Header().Get("Cache-Control"); got != "no-store" {
		t.Fatalf("Cache-Control = %q", got)
	}
	// No member, provider or secret data on the enforcement read.
	for _, key := range []string{"providers", "members", "client_secret", "domains"} {
		if _, present := data[key]; present {
			t.Fatalf("by-domain policy carries %q", key)
		}
	}
	before := h.sso.callCount()
	if r := h.asService("billing-core", idnTestBillToken, ssoByDomainPath); r.Code != http.StatusForbidden {
		t.Fatalf("billing-core: status=%d, want 403", r.Code)
	}
	if r := h.asUser(http.MethodGet, ssoByDomainPath, "u-owner", "", nil); r.Code != http.StatusForbidden && r.Code != http.StatusUnauthorized {
		t.Fatalf("gateway-signed: status=%d, want 401/403", r.Code)
	}
	if h.sso.callCount() != before {
		t.Fatal("a refused by-domain read reached the service")
	}

	// "No verified owner" is a 404: nothing to enforce.
	h.sso.err = orgcore.ErrDomainNotFound
	nf := h.asService("auth-core", idnTestAuthToken, ssoByDomainPath)
	if code, _ := envelopeCodeAndDetails(t, nf); nf.Code != http.StatusNotFound || code != "domain_not_found" {
		t.Fatalf("unknown domain: status=%d code=%q", nf.Code, code)
	}
}
