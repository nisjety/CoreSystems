package http

import (
	"bytes"
	"context"
	"fmt"
	"log"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

const (
	siemTestOrg        = "org-siem-1"
	siemTestAuditToken = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	siemTestBillToken  = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	siemTestSecret     = "ggggggggggggggggggggggggggggggggggg"
	siemTestRegistry   = `[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:provision:self","org:settings:write:self","org:onboarding:write:self","org:siem:manage:self"]},
  {"principal":"audit-core","audience":"org-core","token":%q,"scopes":["org:siem:read:any"]},
  {"principal":"billing-core","audience":"org-core","token":%q,"scopes":["org:read:any"]}
]`
)

func siemTestRegistryJSON() string {
	return fmt.Sprintf(siemTestRegistry, modelPolicyGatewayToken, siemTestAuditToken, siemTestBillToken)
}

// fakeSIEM is a siemDestinationService that records what the routes asked for
// and answers from a canned result or error.
type fakeSIEM struct {
	mu        sync.Mutex
	err       error
	created   []orgcore.CreateSIEMDestinationRequest
	updated   []orgcore.UpdateSIEMDestinationRequest
	deleted   []string
	listed    int
	exported  int
	gotSince  *time.Time
	gotActor  string
	gotRole   string
	exportRow []orgcore.SIEMExporterDestination
}

func (f *fakeSIEM) destination(id string, revision int64) *orgcore.SIEMDestination {
	return &orgcore.SIEMDestination{
		ID: id, OrgID: siemTestOrg, Kind: orgcore.SIEMKindNDJSONWebhook, URL: "https://siem.example.com/ingest",
		SecretSet: true, Enabled: true, Revision: revision, CreatedBy: "u-owner",
		CreatedAt: time.Date(2026, 10, 23, 9, 0, 0, 0, time.UTC), UpdatedAt: time.Date(2026, 10, 23, 9, 0, 0, 0, time.UTC),
	}
}

func (f *fakeSIEM) ListSIEMDestinations(context.Context, string) ([]orgcore.SIEMDestination, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.listed++
	if f.err != nil {
		return nil, f.err
	}
	return []orgcore.SIEMDestination{*f.destination("d-1", 1)}, nil
}

func (f *fakeSIEM) CreateSIEMDestination(_ context.Context, _, actorID, actorRole string, req orgcore.CreateSIEMDestinationRequest) (*orgcore.SIEMDestination, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.created = append(f.created, req)
	f.gotActor, f.gotRole = actorID, actorRole
	if f.err != nil {
		return nil, f.err
	}
	return f.destination("d-new", 1), nil
}

func (f *fakeSIEM) UpdateSIEMDestination(_ context.Context, _, id, actorID, actorRole string, req orgcore.UpdateSIEMDestinationRequest) (*orgcore.SIEMDestination, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.updated = append(f.updated, req)
	f.gotActor, f.gotRole = actorID, actorRole
	if f.err != nil {
		return nil, f.err
	}
	return f.destination(id, req.ExpectedRevision+1), nil
}

func (f *fakeSIEM) DeleteSIEMDestination(_ context.Context, _, id, actorID, actorRole string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.deleted = append(f.deleted, id)
	f.gotActor, f.gotRole = actorID, actorRole
	return f.err
}

func (f *fakeSIEM) ListSIEMDestinationsForExport(_ context.Context, since *time.Time) ([]orgcore.SIEMExporterDestination, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.exported++
	f.gotSince = since
	if f.err != nil {
		return nil, f.err
	}
	return f.exportRow, nil
}

func (f *fakeSIEM) calls() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.listed + len(f.created) + len(f.updated) + len(f.deleted) + f.exported
}

// siemHarness mounts the SIEM handlers behind the REAL service-auth middleware,
// the REAL membership guard and the REAL gate, in the route shape setupRoutes
// uses. NewServer's own router cannot run the guarded routes without a database
// (the guard reads org-core's membership table), so the route table itself is
// pinned by TestSIEMRoutesAreMountedAndGated.
type siemHarness struct {
	router *gin.Engine
	fake   *fakeSIEM
}

func newSIEMHarness(t *testing.T, enabled bool, roles map[string]string) *siemHarness {
	t.Helper()
	credentials, err := parseServiceCredentials(siemTestRegistryJSON())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	fake := &fakeSIEM{}
	server := &Server{callerRoles: fakeCallerRoles{roles: roles}, siemEnabled: enabled}
	server.SetSIEMDestinations(fake)

	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	guard := requireActiveMembership(modelPolicyMembers(roles))
	gate := server.requireSIEMEnabled()
	router.GET("/orgs/:id/siem-destinations", gate, server.listSIEMDestinations)
	router.POST("/orgs/:id/siem-destinations", gate, guard, server.createSIEMDestination)
	router.PATCH("/orgs/:id/siem-destinations/:destinationId", gate, guard, server.updateSIEMDestination)
	router.DELETE("/orgs/:id/siem-destinations/:destinationId", gate, guard, server.deleteSIEMDestination)
	router.GET("/internal/orgs/siem-destinations", gate, server.listSIEMDestinationsInternal)
	return &siemHarness{router: router, fake: fake}
}

// asUser sends a request as the gateway on behalf of userID.
func (h *siemHarness) asUser(method, path, userID, platformRole string, body []byte) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	if len(body) > 0 {
		request.Header.Set("Content-Type", "application/json")
	}
	signModelPolicyDelegation(request, body, userID, siemTestOrg, platformRole)
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

func (h *siemHarness) asService(principal, token, path string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodGet, path, nil)
	request.Header.Set("X-Service-Id", principal)
	request.Header.Set("X-Service-Token", token)
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

var siemTestRoles = map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member", "u-viewer": "viewer"}

const siemCollectionPath = "/orgs/" + siemTestOrg + "/siem-destinations"

func siemCreateBody() []byte {
	return []byte(fmt.Sprintf(`{"kind":"ndjson_webhook","url":"https://siem.example.com/ingest?x=1","secret":%q,"event_filter":{"events":["tool.call"]}}`, siemTestSecret))
}

func TestSIEMDestinationsFlagIsStrict(t *testing.T) {
	for value, want := range map[string]bool{
		"1": true, "true": true,
		"": false, "0": false, "false": false, "TRUE": false, "True": false, "yes": false, "on": false, " 1": false, "1 ": false, "2": false,
	} {
		got := siemDestinationsEnabled(func(string) string { return value })
		if got != want {
			t.Errorf("flag %q = %v, want %v", value, got, want)
		}
	}
}

func TestSIEMRoutesAreMountedAndGated(t *testing.T) {
	t.Setenv(serviceCredentialEnv, siemTestRegistryJSON())
	t.Setenv(siemDestinationsFlagEnv, "")
	off := NewServer(0, nil, nil, "", "")
	if off.siemEnabled {
		t.Fatalf("the SIEM routes are on by default")
	}
	want := map[string]bool{
		"GET /orgs/:id/siem-destinations":                   false,
		"POST /orgs/:id/siem-destinations":                  false,
		"PATCH /orgs/:id/siem-destinations/:destinationId":  false,
		"DELETE /orgs/:id/siem-destinations/:destinationId": false,
		"GET /internal/orgs/siem-destinations":              false,
	}
	for _, route := range off.router.Routes() {
		key := route.Method + " " + route.Path
		if _, ok := want[key]; ok {
			want[key] = true
		}
		if strings.Contains(route.Path, "siem") && !strings.HasPrefix(route.Path, "/orgs/:id/siem-destinations") && route.Path != "/internal/orgs/siem-destinations" {
			t.Errorf("unexpected SIEM route %s", key)
		}
	}
	for key, mounted := range want {
		if !mounted {
			t.Errorf("%s is not registered", key)
		}
	}

	// Off: every path is 404 for a caller that holds the scope, and the
	// internal path never falls through to /orgs/:orgId.
	for _, tc := range []struct{ principal, token string }{{"audit-core", siemTestAuditToken}} {
		request := httptest.NewRequest(http.MethodGet, "/internal/orgs/siem-destinations", nil)
		request.Header.Set("X-Service-Id", tc.principal)
		request.Header.Set("X-Service-Token", tc.token)
		response := httptest.NewRecorder()
		off.router.ServeHTTP(response, request)
		if response.Code != http.StatusNotFound {
			t.Fatalf("flag off, exporter read: status=%d body=%s; want 404", response.Code, response.Body.String())
		}
	}
	request := httptest.NewRequest(http.MethodGet, siemCollectionPath, nil)
	signModelPolicyDelegation(request, nil, "u-owner", siemTestOrg, "")
	response := httptest.NewRecorder()
	off.router.ServeHTTP(response, request)
	if response.Code != http.StatusNotFound {
		t.Fatalf("flag off, admin list: status=%d body=%s; want 404", response.Code, response.Body.String())
	}

	t.Setenv(siemDestinationsFlagEnv, "1")
	if !NewServer(0, nil, nil, "", "").siemEnabled {
		t.Fatalf("ORG_SIEM_DESTINATIONS_ENABLED=1 did not switch the routes on")
	}
}

func TestSIEMGateAnswers404ForEveryRouteWhenOff(t *testing.T) {
	h := newSIEMHarness(t, false, siemTestRoles)
	for _, tc := range []struct {
		method, path string
		body         []byte
	}{
		{http.MethodGet, siemCollectionPath, nil},
		{http.MethodPost, siemCollectionPath, siemCreateBody()},
		{http.MethodPatch, siemCollectionPath + "/d-1", []byte(`{"expected_revision":1,"enabled":false}`)},
		{http.MethodDelete, siemCollectionPath + "/d-1", nil},
	} {
		if response := h.asUser(tc.method, tc.path, "u-owner", "", tc.body); response.Code != http.StatusNotFound {
			t.Errorf("%s %s with the flag off: status=%d; want 404", tc.method, tc.path, response.Code)
		}
	}
	if response := h.asService("audit-core", siemTestAuditToken, "/internal/orgs/siem-destinations"); response.Code != http.StatusNotFound {
		t.Errorf("exporter read with the flag off: status=%d; want 404", response.Code)
	}
	if h.fake.calls() != 0 {
		t.Fatalf("the service was reached %d times with the flag off", h.fake.calls())
	}
}

func TestOnlyOwnerAndAdminMayAdministerSIEMDestinations(t *testing.T) {
	h := newSIEMHarness(t, true, siemTestRoles)

	// A member, a viewer and a stranger are refused whatever role the gateway
	// forwarded: the header is not authority, the membership table is.
	for _, user := range []string{"u-member", "u-viewer", "u-stranger"} {
		for _, forwarded := range []string{"", "admin", "owner", "superadmin"} {
			for _, tc := range []struct {
				method, path string
				body         []byte
			}{
				{http.MethodGet, siemCollectionPath, nil},
				{http.MethodPost, siemCollectionPath, siemCreateBody()},
				{http.MethodPatch, siemCollectionPath + "/d-1", []byte(`{"expected_revision":1,"enabled":false}`)},
				{http.MethodDelete, siemCollectionPath + "/d-1", nil},
			} {
				response := h.asUser(tc.method, tc.path, user, forwarded, tc.body)
				if response.Code != http.StatusForbidden {
					t.Fatalf("%s %s as %s (forwarded role %q): status=%d body=%s; want 403", tc.method, tc.path, user, forwarded, response.Code, response.Body.String())
				}
			}
		}
	}
	if h.fake.calls() != 0 {
		t.Fatalf("a refused caller reached the service %d times", h.fake.calls())
	}

	for _, user := range []string{"u-owner", "u-admin"} {
		if response := h.asUser(http.MethodGet, siemCollectionPath, user, "", nil); response.Code != http.StatusOK {
			t.Fatalf("list as %s: status=%d body=%s", user, response.Code, response.Body.String())
		}
		response := h.asUser(http.MethodPost, siemCollectionPath, user, "", siemCreateBody())
		if response.Code != http.StatusCreated {
			t.Fatalf("create as %s: status=%d body=%s", user, response.Code, response.Body.String())
		}
		if h.fake.gotActor != user || h.fake.gotRole != roleOf(user) {
			t.Fatalf("service saw actor=%q role=%q; want the membership-table identity of %s", h.fake.gotActor, h.fake.gotRole, user)
		}
	}
}

func roleOf(user string) string { return siemTestRoles[user] }

func TestSIEMSecretIsWriteOnlyAndNeverLogged(t *testing.T) {
	var logs bytes.Buffer
	log.SetOutput(&logs)
	defer log.SetOutput(os.Stderr)

	h := newSIEMHarness(t, true, siemTestRoles)
	create := h.asUser(http.MethodPost, siemCollectionPath, "u-owner", "", siemCreateBody())
	list := h.asUser(http.MethodGet, siemCollectionPath, "u-owner", "", nil)
	update := h.asUser(http.MethodPatch, siemCollectionPath+"/d-1", "u-owner", "", []byte(fmt.Sprintf(`{"expected_revision":1,"secret":%q}`, siemTestSecret)))

	for name, response := range map[string]*httptest.ResponseRecorder{"create": create, "list": list, "update": update} {
		if response.Code >= 300 {
			t.Fatalf("%s: status=%d body=%s", name, response.Code, response.Body.String())
		}
		if strings.Contains(response.Body.String(), siemTestSecret) {
			t.Fatalf("%s response carries the secret: %s", name, response.Body.String())
		}
		if response.Header().Get("Cache-Control") != "no-store" {
			t.Errorf("%s Cache-Control = %q, want no-store", name, response.Header().Get("Cache-Control"))
		}
	}
	data, _ := decodeBody(t, create)["data"].(map[string]any)
	if data["secret_set"] != true {
		t.Fatalf("create does not report secret_set: %v", data)
	}
	if _, present := data["secret"]; present {
		t.Fatalf("create response has a secret field: %v", data)
	}
	for _, leaked := range []string{siemTestSecret, "siem.example.com/ingest?x=1"} {
		if strings.Contains(logs.String(), leaked) {
			t.Fatalf("the request log carries %q:\n%s", leaked, logs.String())
		}
	}
}

func TestSIEMErrorsAreTypedAndNeverEchoTheRequest(t *testing.T) {
	h := newSIEMHarness(t, true, siemTestRoles)
	for name, tc := range map[string]struct {
		err    error
		status int
		code   string
	}{
		"validation": {&orgcore.SIEMValidationError{Errors: []string{"url must use https"}}, http.StatusBadRequest, "invalid_siem_destination"},
		"conflict":   {&orgcore.SIEMRevisionConflictError{Current: 7}, http.StatusConflict, "revision_conflict"},
		"limit":      {orgcore.ErrSIEMDestinationLimit, http.StatusConflict, "siem_destination_limit"},
		"not found":  {orgcore.ErrSIEMDestinationNotFound, http.StatusNotFound, "siem_destination_not_found"},
		"plan":       {orgcore.ErrPlanUpgradeRequired, http.StatusPaymentRequired, ""},
		"org":        {orgcore.ErrNotFound, http.StatusNotFound, ""},
		"internal":   {fmt.Errorf("pq: connection refused for postgres://user:%s@db/x", siemTestSecret), http.StatusInternalServerError, ""},
	} {
		h.fake.err = tc.err
		response := h.asUser(http.MethodPost, siemCollectionPath, "u-admin", "", siemCreateBody())
		if response.Code != tc.status {
			t.Fatalf("%s: status=%d body=%s; want %d", name, response.Code, response.Body.String(), tc.status)
		}
		body := decodeBody(t, response)
		if tc.code != "" && body["code"] != tc.code {
			t.Fatalf("%s: code=%v want %s", name, body["code"], tc.code)
		}
		if tc.status == http.StatusConflict && name == "conflict" && body["current_revision"] != float64(7) {
			t.Fatalf("conflict does not carry the current revision: %v", body)
		}
		if strings.Contains(response.Body.String(), siemTestSecret) || strings.Contains(response.Body.String(), "connection refused") {
			t.Fatalf("%s: the response echoes a request value or an internal error: %s", name, response.Body.String())
		}
	}
}

func TestSIEMBodiesAreStrict(t *testing.T) {
	h := newSIEMHarness(t, true, siemTestRoles)
	for name, tc := range map[string]struct {
		body   string
		status int
	}{
		"unknown field":  {`{"kind":"splunk_hec","url":"https://a.example.com","secret":"aaaaaaaaaaaaaaaa","org_id":"other"}`, http.StatusBadRequest},
		"trailing value": {`{"kind":"splunk_hec"} {"kind":"splunk_hec"}`, http.StatusBadRequest},
		"not json":       {`kind=splunk_hec`, http.StatusBadRequest},
		"too large":      {`{"kind":"splunk_hec","secret":"` + strings.Repeat("a", siemMaxBodyBytes) + `"}`, http.StatusRequestEntityTooLarge},
	} {
		response := h.asUser(http.MethodPost, siemCollectionPath, "u-owner", "", []byte(tc.body))
		if response.Code != tc.status {
			t.Errorf("%s: status=%d body=%s; want %d", name, response.Code, response.Body.String(), tc.status)
		}
	}
	if len(h.fake.created) != 0 {
		t.Fatalf("a rejected body reached the service")
	}
}

func TestSIEMUpdateAndDeleteReachTheService(t *testing.T) {
	h := newSIEMHarness(t, true, siemTestRoles)
	update := h.asUser(http.MethodPatch, siemCollectionPath+"/d-9", "u-admin", "", []byte(`{"expected_revision":4,"enabled":false}`))
	if update.Code != http.StatusOK {
		t.Fatalf("update: status=%d body=%s", update.Code, update.Body.String())
	}
	data, _ := decodeBody(t, update)["data"].(map[string]any)
	if data["id"] != "d-9" || data["revision"] != float64(5) {
		t.Fatalf("update body = %v", data)
	}
	if len(h.fake.updated) != 1 || h.fake.updated[0].ExpectedRevision != 4 || h.fake.updated[0].Enabled == nil || *h.fake.updated[0].Enabled {
		t.Fatalf("update request = %+v", h.fake.updated)
	}
	del := h.asUser(http.MethodDelete, siemCollectionPath+"/d-9", "u-owner", "", nil)
	if del.Code != http.StatusNoContent || del.Body.Len() != 0 {
		t.Fatalf("delete: status=%d body=%q", del.Code, del.Body.String())
	}
	if len(h.fake.deleted) != 1 || h.fake.deleted[0] != "d-9" {
		t.Fatalf("deleted = %v", h.fake.deleted)
	}
}

func TestSIEMExporterReadIsForAuditCoreAlone(t *testing.T) {
	h := newSIEMHarness(t, true, siemTestRoles)
	h.fake.exportRow = []orgcore.SIEMExporterDestination{{
		ID: "d-1", OrgID: siemTestOrg, Kind: orgcore.SIEMKindSplunkHEC, URL: "https://hec.example.com/services/collector",
		Secret: siemTestSecret, Revision: 2, UpdatedAt: time.Date(2026, 10, 23, 9, 15, 0, 0, time.UTC),
	}}

	response := h.asService("audit-core", siemTestAuditToken, "/internal/orgs/siem-destinations")
	if response.Code != http.StatusOK || response.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("exporter read: status=%d cache=%q body=%s", response.Code, response.Header().Get("Cache-Control"), response.Body.String())
	}
	data, _ := decodeBody(t, response)["data"].([]any)
	if len(data) != 1 || data[0].(map[string]any)["secret"] != siemTestSecret {
		t.Fatalf("the exporter did not receive the unsealed secret: %s", response.Body.String())
	}

	// Every other principal, including one that holds org:read:any, is refused
	// before the service is reached.
	before := h.fake.exported
	for principal, token := range map[string]string{"billing-core": siemTestBillToken, "verevon-gateway": modelPolicyGatewayToken} {
		if got := h.asService(principal, token, "/internal/orgs/siem-destinations"); got.Code != http.StatusForbidden {
			t.Errorf("%s reached the exporter read: status=%d body=%s", principal, got.Code, got.Body.String())
		}
	}
	if h.fake.exported != before {
		t.Fatalf("a refused principal reached the exporter read")
	}
	// audit-core does not hold the admin scope either.
	request := httptest.NewRequest(http.MethodGet, siemCollectionPath, nil)
	request.Header.Set("X-Service-Id", "audit-core")
	request.Header.Set("X-Service-Token", siemTestAuditToken)
	denied := httptest.NewRecorder()
	h.router.ServeHTTP(denied, request)
	if denied.Code != http.StatusForbidden {
		t.Fatalf("audit-core reached the admin list: status=%d", denied.Code)
	}
	if got := h.asService("audit-core", "iiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiiii", "/internal/orgs/siem-destinations"); got.Code != http.StatusUnauthorized {
		t.Fatalf("a wrong token status=%d; want 401", got.Code)
	}
}

func TestSIEMExporterReadParsesUpdatedSince(t *testing.T) {
	h := newSIEMHarness(t, true, siemTestRoles)
	path := "/internal/orgs/siem-destinations?updated_since=" + url.QueryEscape("2026-10-23T10:00:00+02:00")
	if response := h.asService("audit-core", siemTestAuditToken, path); response.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
	}
	if h.fake.gotSince == nil || !h.fake.gotSince.Equal(time.Date(2026, 10, 23, 8, 0, 0, 0, time.UTC)) {
		t.Fatalf("updated_since = %v", h.fake.gotSince)
	}
	before := h.fake.exported
	for _, bad := range []string{"", "yesterday", "2026-10-23", "1"} {
		path := "/internal/orgs/siem-destinations?updated_since=" + url.QueryEscape(bad)
		if response := h.asService("audit-core", siemTestAuditToken, path); response.Code != http.StatusBadRequest {
			t.Errorf("updated_since=%q status=%d; want 400 (a blank value must not read as absent)", bad, response.Code)
		}
	}
	if h.fake.exported != before {
		t.Fatalf("a bad updated_since reached the service")
	}
}

func TestSIEMScopesForRequest(t *testing.T) {
	for _, tc := range []struct {
		method, path string
		want         []string
	}{
		{http.MethodGet, "/orgs/o/siem-destinations", []string{"org:siem:manage:self"}},
		{http.MethodPost, "/orgs/o/siem-destinations", []string{"org:siem:manage:self"}},
		{http.MethodPatch, "/orgs/o/siem-destinations/d", []string{"org:siem:manage:self"}},
		{http.MethodDelete, "/orgs/o/siem-destinations/d", []string{"org:siem:manage:self"}},
		{http.MethodGet, "/internal/orgs/siem-destinations", []string{"org:siem:read:any"}},
		// Nothing else is opened by the new scopes.
		{http.MethodPut, "/orgs/o/siem-destinations", nil},
		{http.MethodDelete, "/orgs/o/siem-destinations", nil},
		{http.MethodPatch, "/orgs/o/siem-destinations/d/extra", nil},
		{http.MethodPost, "/internal/orgs/siem-destinations", nil},
		{http.MethodGet, "/api/v1/organizations/o/siem-destinations", nil},
		{http.MethodGet, "/internal/orgs/o/siem-destinations", nil},
	} {
		request := httptest.NewRequest(tc.method, tc.path, nil)
		got := serviceScopesForRequest(request)
		if strings.Join(got, ",") != strings.Join(tc.want, ",") {
			t.Errorf("%s %s scopes = %v, want %v", tc.method, tc.path, got, tc.want)
		}
	}
}

func TestSIEMScopesAreBoundToTheirPrincipals(t *testing.T) {
	registry := func(principal, scope string) string {
		return fmt.Sprintf(`[{"principal":%q,"audience":"org-core","token":"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz","scopes":[%q]}]`, principal, scope)
	}
	for _, tc := range []struct {
		principal, scope string
		ok               bool
	}{
		{"audit-core", "org:siem:read:any", true},
		{"verevon-gateway", "org:siem:manage:self", true},
		// The exporter read returns plaintext secrets: no other principal may hold it.
		{"billing-core", "org:siem:read:any", false},
		{"model-gateway", "org:siem:read:any", false},
		{"verevon-gateway", "org:siem:read:any", false},
		// The admin scope is a signed-user scope: gateway only.
		{"audit-core", "org:siem:manage:self", false},
		{"billing-core", "org:siem:manage:self", false},
	} {
		_, err := parseServiceCredentials(registry(tc.principal, tc.scope))
		if (err == nil) != tc.ok {
			t.Errorf("%s holding %s: err=%v, want ok=%v", tc.principal, tc.scope, err, tc.ok)
		}
	}
}

func TestSIEMExporterReadIsNotOpenedByOrgReadAny(t *testing.T) {
	// org:read:any holders reached /internal/orgs/:orgId for any one-segment id;
	// the siem path must resolve to its own scope instead.
	request := httptest.NewRequest(http.MethodGet, "/internal/orgs/siem-destinations", nil)
	scopes := serviceScopesForRequest(request)
	for _, scope := range scopes {
		if scope == "org:read:any" {
			t.Fatalf("org:read:any opens the exporter read: %v", scopes)
		}
	}
}
