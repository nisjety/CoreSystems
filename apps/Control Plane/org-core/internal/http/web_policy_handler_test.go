package http

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/gin-gonic/gin"
)

const (
	webPolicyOrg        = "org-web-1"
	webPolicyAuthToken  = "cccccccccccccccccccccccccccccccccccc"
	webPolicyBillToken  = "dddddddddddddddddddddddddddddddddddd"
	webPolicyInferToken = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
	webPolicyQuarryTok  = "ffffffffffffffffffffffffffffffffffff"
	webPolicyV1Path     = "/api/v1/organizations/" + webPolicyOrg + "/web-policy"
	webPolicyProxyPath  = "/orgs/" + webPolicyOrg + "/web-policy"
	webPolicyInternal   = "/internal/orgs/" + webPolicyOrg + "/web-policy"
)

func webPolicyRegistry() string {
	return fmt.Sprintf(`[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:provision:self","org:settings:write:self","org:onboarding:write:self"]},
  {"principal":"auth-core","audience":"org-core","token":%q,"scopes":["org:projection:write:any","org:projection:delete:any","org:retention:read:any","org:web-policy:read:any"]},
  {"principal":"billing-core","audience":"org-core","token":%q,"scopes":["org:read:any"]},
  {"principal":"inference-core","audience":"org-core","token":%q,"scopes":["org:model-entitlement:read:any"]},
  {"principal":"quarry-edge","audience":"org-core","token":%q,"scopes":["org:retention:read:any"]}
]`, modelPolicyGatewayToken, webPolicyAuthToken, webPolicyBillToken, webPolicyInferToken, webPolicyQuarryTok)
}

var webPolicyServiceTokens = map[string]string{
	"auth-core":      webPolicyAuthToken,
	"billing-core":   webPolicyBillToken,
	"inference-core": webPolicyInferToken,
	"quarry-edge":    webPolicyQuarryTok,
}

// memoryWebPolicyService is the web policy service the handlers depend on, with the
// repository's compare-and-set, entitlement check and append-only history, so the
// real router and the real middleware run without a database. It uses the real
// document functions (Normalize, Validate, CheckEntitled, Tightened, Digest) from
// package org; only the storage is a map.
type memoryWebPolicyService struct {
	mu        sync.Mutex
	plan      string
	notFound  bool
	err       error
	exists    bool
	revision  int64
	doc       orgcore.WebPolicyDoc
	updatedBy string
	history   []orgcore.WebPolicyHistoryEntry
	writes    int
	reads     int
	lastActor string
	lastRole  string
}

func newMemoryWebPolicyService(plan string) *memoryWebPolicyService {
	return &memoryWebPolicyService{plan: plan}
}

func (m *memoryWebPolicyService) GetWebPolicyPosture(_ context.Context, _ string) (*orgcore.WebPolicyView, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.reads++
	if m.err != nil {
		return nil, m.err
	}
	if m.notFound {
		return nil, orgcore.ErrNotFound
	}
	entitlements := orgcore.WebEntitlementsForPlan(m.plan)
	view := &orgcore.WebPolicyView{
		Exists: m.exists, Policy: m.doc.Normalize(), Effective: m.doc.Effective(entitlements),
		Revision: m.revision, Rev: "0", Entitlements: entitlements.Map(),
	}
	if m.exists {
		view.Rev = m.doc.Digest()
		by, at := m.updatedBy, "2026-11-02T10:00:00Z"
		view.UpdatedBy, view.UpdatedAt = &by, &at
	}
	return view, nil
}

func (m *memoryWebPolicyService) SetWebPolicy(_ context.Context, _, changedBy, actorRole string, doc orgcore.WebPolicyDoc, ifMatch int64) (*orgcore.WebPolicyPutResult, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.err != nil {
		return nil, m.err
	}
	doc = doc.Normalize()
	if err := doc.Validate(orgcore.DefaultWebPolicyCatalogue()); err != nil {
		return nil, err
	}
	if ifMatch != m.revision {
		return nil, &orgcore.RevisionConflictError{Current: m.revision}
	}
	if err := doc.CheckEntitled(orgcore.WebEntitlementsForPlan(m.plan)); err != nil {
		return nil, err
	}
	tightened := orgcore.Tightened(m.doc, doc)
	m.revision++
	m.writes++
	m.exists, m.doc, m.updatedBy, m.lastActor, m.lastRole = true, doc, changedBy, changedBy, actorRole
	m.history = append([]orgcore.WebPolicyHistoryEntry{{
		Revision: m.revision, Rev: doc.Digest(), Policy: doc, ChangedBy: changedBy,
		ChangedAt: "2026-11-02T10:00:00Z", Tightened: tightened,
	}}, m.history...)
	return &orgcore.WebPolicyPutResult{Revision: m.revision, Rev: doc.Digest(), Tightened: tightened}, nil
}

func (m *memoryWebPolicyService) ListWebPolicyHistory(_ context.Context, _ string, limit int, cursor int64) ([]orgcore.WebPolicyHistoryEntry, int64, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.err != nil {
		return nil, 0, m.err
	}
	var page []orgcore.WebPolicyHistoryEntry
	for _, entry := range m.history {
		if cursor == 0 || entry.Revision < cursor {
			page = append(page, entry)
		}
	}
	if len(page) > limit {
		page = page[:limit]
		return page, page[len(page)-1].Revision, nil
	}
	return page, 0, nil
}

type webPolicyHarness struct {
	router  *gin.Engine
	service *memoryWebPolicyService
}

// newWebPolicyHarness mounts the real web policy handlers behind the REAL service
// auth middleware and the REAL membership guard, over the in-memory service. The
// route table itself is pinned by TestWebPolicyRoutesAreMounted.
func newWebPolicyHarness(t *testing.T, roles map[string]string, plan string) *webPolicyHarness {
	t.Helper()
	credentials, err := parseServiceCredentials(webPolicyRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	service := newMemoryWebPolicyService(plan)
	server := &Server{callerRoles: fakeCallerRoles{roles: roles}}
	server.SetWebPolicyService(service)

	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	guard := requireActiveMembership(modelPolicyMembers(roles))
	router.GET("/api/v1/organizations/:id/web-policy", server.getWebPolicy)
	router.PUT("/api/v1/organizations/:id/web-policy", guard, server.putWebPolicy)
	router.GET("/api/v1/organizations/:id/web-policy/history", server.listWebPolicyHistory)
	router.GET("/orgs/:id/web-policy", server.getWebPolicy)
	router.PUT("/orgs/:id/web-policy", guard, server.putWebPolicy)
	router.GET("/orgs/:id/web-policy/history", server.listWebPolicyHistory)
	return &webPolicyHarness{router: router, service: service}
}

func (h *webPolicyHarness) do(method, path, userID, forwardedRole, ifMatch string, body []byte) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, bytes.NewReader(body))
	if len(body) > 0 {
		request.Header.Set("Content-Type", "application/json")
	}
	if ifMatch != "" {
		request.Header.Set("If-Match", ifMatch)
	}
	signModelPolicyDelegation(request, body, userID, webPolicyOrg, forwardedRole)
	response := httptest.NewRecorder()
	h.router.ServeHTTP(response, request)
	return response
}

func webPolicyErrorBody(t *testing.T, response *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	body := decodeBody(t, response)
	envelope, ok := body["error"].(map[string]any)
	if !ok {
		t.Fatalf("not a typed error envelope: %s", response.Body.String())
	}
	if _, ok := envelope["code"].(string); !ok {
		t.Fatalf("error without a code: %s", response.Body.String())
	}
	if _, ok := envelope["message"].(string); !ok {
		t.Fatalf("error without a message: %s", response.Body.String())
	}
	if _, ok := envelope["details"].(map[string]any); !ok {
		t.Fatalf("error without details: %s", response.Body.String())
	}
	return envelope
}

func webPolicyData(t *testing.T, response *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	data, ok := decodeBody(t, response)["data"].(map[string]any)
	if !ok {
		t.Fatalf("not a {data} envelope: %s", response.Body.String())
	}
	return data
}

const webPolicyUploadOff = `{"caps":{"browser.act.upload":"disabled"}}`

func TestWebPolicyRoutesAreMounted(t *testing.T) {
	t.Setenv(serviceCredentialEnv, webPolicyRegistry())
	server := NewServer(0, nil, nil, "", "")
	want := map[string]bool{
		"GET /api/v1/organizations/:id/web-policy":         false,
		"PUT /api/v1/organizations/:id/web-policy":         false,
		"GET /api/v1/organizations/:id/web-policy/history": false,
		"GET /orgs/:id/web-policy":                         false,
		"PUT /orgs/:id/web-policy":                         false,
		"GET /orgs/:id/web-policy/history":                 false,
		"GET /internal/orgs/:orgId/web-policy":             false,
	}
	for _, route := range server.router.Routes() {
		key := route.Method + " " + route.Path
		if _, ok := want[key]; ok {
			want[key] = true
		}
		if strings.Contains(route.Path, "web-policy") {
			if _, ok := want[key]; !ok {
				t.Fatalf("unexpected web-policy route %s", key)
			}
		}
	}
	for key, mounted := range want {
		if !mounted {
			t.Errorf("%s is not mounted", key)
		}
	}
}

func TestAdminMatrix(t *testing.T) {
	roles := map[string]string{"u-owner": "owner", "u-admin": "admin", "u-member": "member", "u-viewer": "viewer"}
	paths := []string{webPolicyV1Path, webPolicyProxyPath}

	for _, path := range paths {
		harness := newWebPolicyHarness(t, roles, "enterprise")

		// Owner and admin: the stored document and the history.
		for _, user := range []string{"u-owner", "u-admin"} {
			put := harness.do(http.MethodPut, path, user, "", strconv.Itoa(int(harness.service.revision)), []byte(webPolicyUploadOff))
			if put.Code != http.StatusOK {
				t.Fatalf("%s PUT %s: status=%d body=%s", user, path, put.Code, put.Body.String())
			}
			get := harness.do(http.MethodGet, path, user, "", "", nil)
			data := webPolicyData(t, get)
			if get.Code != http.StatusOK || data["policy"] == nil || data["updated_by"] != user {
				t.Fatalf("%s GET %s: status=%d body=%s", user, path, get.Code, get.Body.String())
			}
			if history := harness.do(http.MethodGet, path+"/history", user, "", "", nil); history.Code != http.StatusOK {
				t.Fatalf("%s history %s: status=%d body=%s", user, path, history.Code, history.Body.String())
			}
		}
		if harness.service.lastRole != "admin" && harness.service.lastRole != "owner" {
			t.Fatalf("the audit role must come from the membership table, got %q", harness.service.lastRole)
		}

		// Member and viewer: no write, no history; a read of the effective document
		// only, whatever role the gateway forwarded.
		writes := harness.service.writes
		for _, user := range []string{"u-member", "u-viewer"} {
			for _, forwarded := range []string{"", "admin", "owner", "superadmin"} {
				put := harness.do(http.MethodPut, path, user, forwarded, "2", []byte(webPolicyUploadOff))
				if put.Code != http.StatusForbidden || webPolicyErrorBody(t, put)["code"] != "admin_required" {
					t.Fatalf("%s (forwarded %q) PUT %s: status=%d body=%s; want 403 admin_required", user, forwarded, path, put.Code, put.Body.String())
				}
				history := harness.do(http.MethodGet, path+"/history", user, forwarded, "", nil)
				if history.Code != http.StatusForbidden || webPolicyErrorBody(t, history)["code"] != "admin_required" {
					t.Fatalf("%s (forwarded %q) history %s: status=%d; want 403 admin_required", user, forwarded, path, history.Code)
				}
			}
			get := harness.do(http.MethodGet, path, user, "admin", "", nil)
			data := webPolicyData(t, get)
			if get.Code != http.StatusOK || data["policy"] != nil || data["updated_by"] != nil || data["updated_at"] != nil {
				t.Fatalf("%s GET %s must not see the stored document: status=%d body=%s", user, path, get.Code, get.Body.String())
			}
			if data["effective"] == nil || data["entitlements"] == nil || data["rev"] == nil || data["revision"] == nil {
				t.Fatalf("%s GET %s lost the effective view: %s", user, path, get.Body.String())
			}
		}
		if harness.service.writes != writes {
			t.Fatalf("a refused caller wrote the policy (%d -> %d)", writes, harness.service.writes)
		}

		// A stranger is stopped before the handler: the guard on the write, the
		// membership lookup on the reads.
		for _, probe := range []struct{ method, suffix string }{
			{http.MethodPut, ""}, {http.MethodGet, ""}, {http.MethodGet, "/history"},
		} {
			response := harness.do(probe.method, path+probe.suffix, "u-stranger", "admin", "0", []byte(webPolicyUploadOff))
			if response.Code != http.StatusForbidden {
				t.Fatalf("non-member %s %s: status=%d body=%s; want 403", probe.method, path+probe.suffix, response.Code, response.Body.String())
			}
		}
		if harness.service.writes != writes {
			t.Fatalf("a non-member wrote the policy")
		}
	}
}

func TestWebPolicyAdminRoutesRefuseAnUnidentifiedOrUnconfiguredCaller(t *testing.T) {
	service := newMemoryWebPolicyService("enterprise")
	server := &Server{callerRoles: fakeCallerRoles{roles: map[string]string{"u-owner": "owner"}}}
	server.SetWebPolicyService(service)
	router := gin.New()
	router.GET("/orgs/:id/web-policy", server.getWebPolicy)

	response := httptest.NewRecorder()
	router.ServeHTTP(response, httptest.NewRequest(http.MethodGet, webPolicyProxyPath, nil))
	if response.Code != http.StatusUnauthorized || webPolicyErrorBody(t, response)["code"] != "unauthenticated" {
		t.Fatalf("no identity: status=%d body=%s", response.Code, response.Body.String())
	}

	noRoles := &Server{}
	noRoles.SetWebPolicyService(service)
	router = gin.New()
	router.GET("/orgs/:id/web-policy", noRoles.getWebPolicy)
	request := httptest.NewRequest(http.MethodGet, webPolicyProxyPath, nil)
	request.Header.Set("x-user-id", "u-owner")
	response = httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("no role reader: status=%d", response.Code)
	}

	noService := &Server{callerRoles: fakeCallerRoles{roles: map[string]string{"u-owner": "owner"}}}
	router = gin.New()
	router.GET("/orgs/:id/web-policy", noService.getWebPolicy)
	request = httptest.NewRequest(http.MethodGet, webPolicyProxyPath, nil)
	request.Header.Set("x-user-id", "u-owner")
	response = httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusServiceUnavailable || webPolicyErrorBody(t, response)["code"] != "unavailable" {
		t.Fatalf("no service: status=%d body=%s", response.Code, response.Body.String())
	}

	failing := &Server{callerRoles: fakeCallerRoles{err: errors.New("db down")}}
	failing.SetWebPolicyService(service)
	router = gin.New()
	router.GET("/orgs/:id/web-policy", failing.getWebPolicy)
	request = httptest.NewRequest(http.MethodGet, webPolicyProxyPath, nil)
	request.Header.Set("x-user-id", "u-owner")
	response = httptest.NewRecorder()
	router.ServeHTTP(response, request)
	if response.Code != http.StatusInternalServerError || strings.Contains(response.Body.String(), "db down") {
		t.Fatalf("role lookup failure: status=%d body=%s (must not leak the cause)", response.Code, response.Body.String())
	}
}

func TestPutCreatesWithIfMatchZeroAndGetReturnsTheSameRev(t *testing.T) {
	harness := newWebPolicyHarness(t, map[string]string{"u-owner": "owner"}, "enterprise")

	// Absent policy: revision 0, the empty document, rev "0".
	empty := harness.do(http.MethodGet, webPolicyProxyPath, "u-owner", "", "", nil)
	data := webPolicyData(t, empty)
	if data["revision"] != float64(0) || data["rev"] != "0" || !reflect.DeepEqual(data["policy"], map[string]any{}) {
		t.Fatalf("absent policy answered %v", data)
	}
	if empty.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("a policy answer must be no-store, got %q", empty.Header().Get("Cache-Control"))
	}

	put := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", "0", []byte(webPolicyUploadOff))
	if put.Code != http.StatusOK {
		t.Fatalf("first write: %d %s", put.Code, put.Body.String())
	}
	written := webPolicyData(t, put)
	if written["revision"] != float64(1) || len(written) != 2 {
		t.Fatalf("PUT answered %v, want exactly {revision, rev}", written)
	}
	rev, _ := written["rev"].(string)
	if len(rev) != 12 {
		t.Fatalf("rev %q is not a 12 hex digest", rev)
	}

	got := webPolicyData(t, harness.do(http.MethodGet, webPolicyV1Path, "u-owner", "", "", nil))
	if got["rev"] != rev || got["revision"] != float64(1) {
		t.Fatalf("GET after PUT answered %v, want rev %s", got, rev)
	}
	policy, _ := got["policy"].(map[string]any)
	caps, _ := policy["caps"].(map[string]any)
	if caps["browser.act.upload"] != "disabled" {
		t.Fatalf("stored policy = %v", got["policy"])
	}
	for _, field := range []string{"policy", "effective", "revision", "rev", "entitlements", "updated_by", "updated_at"} {
		if _, ok := got[field]; !ok {
			t.Errorf("GET lacks %s", field)
		}
	}
}

func TestStalePutIs409(t *testing.T) {
	harness := newWebPolicyHarness(t, map[string]string{"u-owner": "owner"}, "enterprise")
	if first := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", "0", []byte(webPolicyUploadOff)); first.Code != http.StatusOK {
		t.Fatalf("first write: %d %s", first.Code, first.Body.String())
	}
	stale := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", "0", []byte(`{"caps":{"browser.act.input":"disabled"}}`))
	if stale.Code != http.StatusConflict {
		t.Fatalf("stale write: status=%d body=%s; want 409", stale.Code, stale.Body.String())
	}
	envelope := webPolicyErrorBody(t, stale)
	details := envelope["details"].(map[string]any)
	if envelope["code"] != "revision_conflict" || details["current"] != float64(1) {
		t.Fatalf("a conflict must name the revision to re-read: %v", envelope)
	}
	if harness.service.writes != 1 {
		t.Fatalf("the stale write was applied, writes=%d", harness.service.writes)
	}
}

func TestPutRequiresAWellFormedIfMatch(t *testing.T) {
	harness := newWebPolicyHarness(t, map[string]string{"u-owner": "owner"}, "enterprise")
	missing := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", "", []byte(webPolicyUploadOff))
	if missing.Code != http.StatusPreconditionRequired || webPolicyErrorBody(t, missing)["code"] != "precondition_required" {
		t.Fatalf("missing If-Match: status=%d body=%s; want 428", missing.Code, missing.Body.String())
	}
	for _, bad := range []string{"abc", "-1", "1.5", "*", "0x10", `""`, "1 2"} {
		response := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", bad, []byte(webPolicyUploadOff))
		if response.Code != http.StatusBadRequest || webPolicyErrorBody(t, response)["code"] != "invalid_if_match" {
			t.Fatalf("If-Match %q: status=%d body=%s; want 400 invalid_if_match", bad, response.Code, response.Body.String())
		}
	}
	if harness.service.writes != 0 {
		t.Fatalf("a request without a usable If-Match wrote the policy")
	}
	for _, good := range []string{"0", `"0"`, `W/"0"`} {
		harness := newWebPolicyHarness(t, map[string]string{"u-owner": "owner"}, "enterprise")
		if response := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", good, []byte(webPolicyUploadOff)); response.Code != http.StatusOK {
			t.Fatalf("If-Match %s: status=%d body=%s", good, response.Code, response.Body.String())
		}
	}
}

func TestPutMapsRefusalsToTypedErrors(t *testing.T) {
	cases := []struct {
		name       string
		plan       string
		body       string
		wantStatus int
		wantCode   string
		wantKey    string
	}{
		{"unknown capability", "enterprise", `{"caps":{"browser.act.teleport":"disabled"}}`, http.StatusUnprocessableEntity, "unknown_capability", "browser.act.teleport"},
		{"forced on an ability", "enterprise", `{"caps":{"browser.act.upload":"forced"}}`, http.StatusUnprocessableEntity, "illegal_mode", "browser.act.upload"},
		{"ceiling in caps", "enterprise", `{"caps":{"robots.max_mode":"disabled"}}`, http.StatusUnprocessableEntity, "illegal_mode", "robots.max_mode"},
		{"bad ceiling", "enterprise", `{"robots_max":"always"}`, http.StatusUnprocessableEntity, "illegal_mode", "robots.max_mode"},
		{"wildcard domain", "enterprise", `{"domains":{"deny":["*.example.com"]}}`, http.StatusUnprocessableEntity, "invalid_domain", "domains.deny"},
		{"ip literal", "enterprise", `{"domains":{"allow":["10.0.0.1"]}}`, http.StatusUnprocessableEntity, "invalid_domain", "domains.allow"},
		{"robots override on a plan without it", "pro", `{"robots_max":"ignore"}`, http.StatusPaymentRequired, "not_entitled", "robots.max_mode"},
		{"proxy pool on a plan without it", "standard", `{"proxy_max":"pool"}`, http.StatusPaymentRequired, "not_entitled", "proxy.max_tier"},
		{"paid search on the free plan", "free", `{"caps":{"search.paid_provider":"allowed"}}`, http.StatusPaymentRequired, "not_entitled", "search.paid_provider"},
		{"unknown top-level field", "enterprise", `{"caps":{},"extra":true}`, http.StatusBadRequest, "invalid_body", ""},
		{"caps value is not a string", "enterprise", `{"caps":{"browser.act.upload":true}}`, http.StatusBadRequest, "invalid_body", ""},
		{"not json", "enterprise", `not json`, http.StatusBadRequest, "invalid_body", ""},
		{"empty body", "enterprise", ``, http.StatusBadRequest, "invalid_body", ""},
		{"two documents", "enterprise", `{} {}`, http.StatusBadRequest, "invalid_body", ""},
	}
	for _, tc := range cases {
		harness := newWebPolicyHarness(t, map[string]string{"u-owner": "owner"}, tc.plan)
		response := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", "0", []byte(tc.body))
		if response.Code != tc.wantStatus {
			t.Errorf("%s: status=%d body=%s; want %d", tc.name, response.Code, response.Body.String(), tc.wantStatus)
			continue
		}
		envelope := webPolicyErrorBody(t, response)
		if envelope["code"] != tc.wantCode {
			t.Errorf("%s: code=%v want %s", tc.name, envelope["code"], tc.wantCode)
		}
		details := envelope["details"].(map[string]any)
		if tc.wantKey != "" && details["key"] != tc.wantKey {
			t.Errorf("%s: details=%v want key %s", tc.name, details, tc.wantKey)
		}
		if tc.wantCode == "not_entitled" {
			capability, _ := details["capability"].(string)
			if !strings.HasPrefix(capability, "web.") {
				t.Errorf("%s: details=%v lacks the entitlement key", tc.name, details)
			}
		}
		if harness.service.writes != 0 {
			t.Errorf("%s: a refused document was stored", tc.name)
		}
	}

	// An entitled plan stores what the unentitled one was refused.
	harness := newWebPolicyHarness(t, map[string]string{"u-owner": "owner"}, "enterprise")
	if response := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", "0", []byte(`{"robots_max":"ignore","proxy_max":"pool","caps":{"search.paid_provider":"allowed"}}`)); response.Code != http.StatusOK {
		t.Fatalf("enterprise unlock refused: %d %s", response.Code, response.Body.String())
	}
}

func TestPutRejectsOverlargeDocument(t *testing.T) {
	harness := newWebPolicyHarness(t, map[string]string{"u-owner": "owner"}, "enterprise")

	// Over the document cap but under the body cap: a typed 422 too_large.
	hosts := make([]string, 0, 26)
	for i := 0; i < 26; i++ {
		hosts = append(hosts, fmt.Sprintf("%s%02d.example.com", strings.Repeat("a", 60), i))
	}
	body, _ := json.Marshal(map[string]any{"domains": map[string]any{"deny": hosts}})
	response := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", "0", body)
	if response.Code != http.StatusUnprocessableEntity || webPolicyErrorBody(t, response)["code"] != "too_large" {
		t.Fatalf("2 KiB document: status=%d body=%s; want 422 too_large", response.Code, response.Body.String())
	}

	// More than 64 rules.
	many := make([]string, 0, 65)
	for i := 0; i < 65; i++ {
		many = append(many, fmt.Sprintf("h%d.example.com", i))
	}
	body, _ = json.Marshal(map[string]any{"domains": map[string]any{"deny": many}})
	response = harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", "0", body)
	if response.Code != http.StatusUnprocessableEntity || webPolicyErrorBody(t, response)["code"] != "too_large" {
		t.Fatalf("65 rules: status=%d body=%s; want 422 too_large", response.Code, response.Body.String())
	}

	// Over the body cap: 413 before the document is read.
	huge := []byte(`{"caps":{},"pad":"` + strings.Repeat("x", webPolicyMaxBodyBytes+1) + `"}`)
	response = harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", "0", huge)
	if response.Code != http.StatusRequestEntityTooLarge || webPolicyErrorBody(t, response)["code"] != "too_large" {
		t.Fatalf("oversized body: status=%d body=%s; want 413 too_large", response.Code, response.Body.String())
	}
	if harness.service.writes != 0 {
		t.Fatalf("an oversized document was stored")
	}
}

func TestWebPolicyHistoryIsNewestFirstAndPaged(t *testing.T) {
	harness := newWebPolicyHarness(t, map[string]string{"u-owner": "owner"}, "enterprise")
	bodies := []string{
		`{"caps":{"browser.act.upload":"disabled"}}`,
		`{"caps":{"browser.act.upload":"disabled","browser.act.input":"disabled"}}`,
		`{}`,
	}
	for index, body := range bodies {
		if response := harness.do(http.MethodPut, webPolicyProxyPath, "u-owner", "", strconv.Itoa(index), []byte(body)); response.Code != http.StatusOK {
			t.Fatalf("write %d: %d %s", index, response.Code, response.Body.String())
		}
	}
	first := harness.do(http.MethodGet, webPolicyProxyPath+"/history?limit=2", "u-owner", "", "", nil)
	if first.Code != http.StatusOK {
		t.Fatalf("history: %d %s", first.Code, first.Body.String())
	}
	var page struct {
		Data []map[string]any `json:"data"`
		Meta map[string]any   `json:"meta"`
	}
	if err := json.Unmarshal(first.Body.Bytes(), &page); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if len(page.Data) != 2 || page.Data[0]["revision"] != float64(3) || page.Data[1]["revision"] != float64(2) {
		t.Fatalf("history page = %s", first.Body.String())
	}
	if page.Data[0]["tightened"] != false || page.Data[1]["tightened"] != true {
		t.Fatalf("tightened flags = %v, %v (revision 3 loosened, revision 2 tightened)", page.Data[0]["tightened"], page.Data[1]["tightened"])
	}
	if page.Meta["cursor"] != "2" {
		t.Fatalf("next cursor = %v, want 2", page.Meta["cursor"])
	}
	second := harness.do(http.MethodGet, webPolicyProxyPath+"/history?limit=2&cursor=2", "u-owner", "", "", nil)
	if err := json.Unmarshal(second.Body.Bytes(), &page); err != nil || len(page.Data) != 1 || page.Data[0]["revision"] != float64(1) || page.Meta["cursor"] != nil {
		t.Fatalf("second page = %s (err %v)", second.Body.String(), err)
	}
	for _, bad := range []string{"limit=0", "limit=201", "limit=abc", "cursor=abc", "cursor=0", "cursor=-3"} {
		response := harness.do(http.MethodGet, webPolicyProxyPath+"/history?"+bad, "u-owner", "", "", nil)
		if response.Code != http.StatusBadRequest {
			t.Errorf("history?%s: status=%d, want 400", bad, response.Code)
		}
	}
	if response := harness.do(http.MethodPut, webPolicyProxyPath+"/history", "u-owner", "", "0", nil); response.Code == http.StatusOK {
		t.Fatal("the history route accepted a write")
	}
}

func TestWebPolicyUnknownOrganizationAndFailuresAreTyped(t *testing.T) {
	harness := newWebPolicyHarness(t, map[string]string{"u-owner": "owner"}, "enterprise")
	harness.service.notFound = true
	response := harness.do(http.MethodGet, webPolicyProxyPath, "u-owner", "", "", nil)
	if response.Code != http.StatusNotFound || webPolicyErrorBody(t, response)["code"] != "not_found" {
		t.Fatalf("unknown org: status=%d body=%s", response.Code, response.Body.String())
	}
	harness.service.notFound = false
	harness.service.err = errors.New("connection refused to 10.1.2.3:5432")
	for _, probe := range []struct{ method, ifMatch string }{{http.MethodGet, ""}, {http.MethodPut, "0"}} {
		response := harness.do(probe.method, webPolicyProxyPath, "u-owner", "", probe.ifMatch, []byte(`{}`))
		if response.Code != http.StatusInternalServerError || strings.Contains(response.Body.String(), "10.1.2.3") {
			t.Fatalf("%s failure: status=%d body=%s (must be a 500 without the cause)", probe.method, response.Code, response.Body.String())
		}
		webPolicyErrorBody(t, response)
	}
}

// --- the internal read and its own scope ---

func webPolicyInternalServer(t *testing.T, service webPolicyService) *Server {
	t.Helper()
	t.Setenv(serviceCredentialEnv, webPolicyRegistry())
	server := NewServer(0, nil, nil, "", "")
	server.webPolicy = service
	return server
}

func serveWebPolicyInternal(server *Server, method, path, principal, token string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(method, path, nil)
	if principal != "" {
		request.Header.Set("X-Service-Id", principal)
	}
	if token != "" {
		request.Header.Set("X-Service-Token", token)
	}
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, request)
	return response
}

func TestInternalRouteRequiresItsOwnScope(t *testing.T) {
	service := newMemoryWebPolicyService("pro")
	service.exists, service.revision, service.updatedBy = true, 4, "u-owner"
	service.doc = orgcore.WebPolicyDoc{Caps: map[string]string{"browser.act.upload": "disabled"}, ProxyMax: "pool"}
	server := webPolicyInternalServer(t, service)

	// Every principal in the registry other than auth-core is refused, including
	// the broad org:read:any holder and the retention-posture holders.
	for principal, token := range webPolicyServiceTokens {
		if principal == "auth-core" {
			continue
		}
		response := serveWebPolicyInternal(server, http.MethodGet, webPolicyInternal, principal, token)
		if response.Code != http.StatusForbidden {
			t.Fatalf("principal %s reached the web policy route: status=%d body=%s", principal, response.Code, response.Body.String())
		}
	}
	gateway := serveWebPolicyInternal(server, http.MethodGet, webPolicyInternal, "verevon-gateway", modelPolicyGatewayToken)
	if gateway.Code != http.StatusForbidden {
		t.Fatalf("the gateway reached the internal route: status=%d", gateway.Code)
	}
	if service.reads != 0 {
		t.Fatalf("a refused principal reached the service %d times", service.reads)
	}

	// No credentials and wrong credentials never reach the route.
	if response := serveWebPolicyInternal(server, http.MethodGet, webPolicyInternal, "", ""); response.Code != http.StatusUnauthorized {
		t.Fatalf("no credentials: status=%d", response.Code)
	}
	if response := serveWebPolicyInternal(server, http.MethodGet, webPolicyInternal, "auth-core", "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww"); response.Code != http.StatusUnauthorized {
		t.Fatalf("wrong token: status=%d", response.Code)
	}

	// Auth Core gets the effective document.
	response := serveWebPolicyInternal(server, http.MethodGet, webPolicyInternal, "auth-core", webPolicyAuthToken)
	if response.Code != http.StatusOK {
		t.Fatalf("auth-core: status=%d body=%s", response.Code, response.Body.String())
	}
	if response.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("Cache-Control=%q, want no-store", response.Header().Get("Cache-Control"))
	}
	data := webPolicyData(t, response)
	if data["revision"] != float64(4) || data["rev"] != service.doc.Digest() {
		t.Fatalf("answer = %v", data)
	}
	policy, _ := data["policy"].(map[string]any)
	if policy["proxy_max"] != "pool" {
		t.Fatalf("a pro plan keeps its proxy unlock: %v", policy)
	}
	caps, _ := policy["caps"].(map[string]any)
	if caps["browser.act.upload"] != "disabled" {
		t.Fatalf("policy caps = %v", caps)
	}
	if len(data) != 4 {
		t.Fatalf("the internal answer must carry exactly revision, rev, policy and entitlements, got %v", data)
	}

	// The same credential does not widen: it reads nothing else about the org and
	// cannot write the policy.
	for _, probe := range []struct{ method, path string }{
		{http.MethodGet, "/internal/orgs/" + webPolicyOrg},
		{http.MethodGet, "/internal/orgs"},
		{http.MethodPut, webPolicyInternal},
		{http.MethodPut, webPolicyProxyPath},
		{http.MethodGet, webPolicyProxyPath},
		{http.MethodGet, webPolicyV1Path + "/history"},
	} {
		response := serveWebPolicyInternal(server, probe.method, probe.path, "auth-core", webPolicyAuthToken)
		if response.Code != http.StatusForbidden {
			t.Fatalf("auth-core %s %s: status=%d, want 403 (the grant must stay narrow)", probe.method, probe.path, response.Code)
		}
	}
}

func TestInternalRouteFoldsEntitlementsAndAnswersTypedFailures(t *testing.T) {
	service := newMemoryWebPolicyService("free")
	server := webPolicyInternalServer(t, service)

	// An organization that never wrote a policy: rev "0", revision 0, and the plan
	// derived restriction (a free plan has no paid search).
	response := serveWebPolicyInternal(server, http.MethodGet, webPolicyInternal, "auth-core", webPolicyAuthToken)
	data := webPolicyData(t, response)
	policy, _ := data["policy"].(map[string]any)
	caps, _ := policy["caps"].(map[string]any)
	if data["rev"] != "0" || data["revision"] != float64(0) || caps["search.paid_provider"] != "disabled" {
		t.Fatalf("no row on a free plan answered %v", data)
	}
	entitlements, _ := data["entitlements"].(map[string]any)
	if entitlements["web.paid_search"] != false || entitlements["web.proxy_pool"] != false || entitlements["web.robots_override"] != false {
		t.Fatalf("entitlements = %v", entitlements)
	}

	// A stored unlock the plan no longer carries is folded away.
	service.exists, service.revision, service.doc = true, 2, orgcore.WebPolicyDoc{RobotsMax: "ignore", ProxyMax: "pool"}
	data = webPolicyData(t, serveWebPolicyInternal(server, http.MethodGet, webPolicyInternal, "auth-core", webPolicyAuthToken))
	policy, _ = data["policy"].(map[string]any)
	if policy["robots_max"] != "respect" || policy["proxy_max"] != "none" {
		t.Fatalf("a lapsed plan kept its unlock: %v", policy)
	}

	service.notFound = true
	missing := serveWebPolicyInternal(server, http.MethodGet, webPolicyInternal, "auth-core", webPolicyAuthToken)
	if missing.Code != http.StatusNotFound || webPolicyErrorBody(t, missing)["code"] != "not_found" {
		t.Fatalf("unknown org: status=%d body=%s", missing.Code, missing.Body.String())
	}
	service.notFound = false
	service.err = errors.New("dial tcp 10.9.9.9:5432: connect: refused")
	failed := serveWebPolicyInternal(server, http.MethodGet, webPolicyInternal, "auth-core", webPolicyAuthToken)
	if failed.Code != http.StatusInternalServerError || strings.Contains(failed.Body.String(), "10.9.9.9") {
		t.Fatalf("failure: status=%d body=%s (a 5xx with no detail)", failed.Code, failed.Body.String())
	}
	webPolicyErrorBody(t, failed)
	if failed.Header().Get("Cache-Control") != "no-store" {
		t.Fatal("an internal failure must be no-store too")
	}

	unconfigured := webPolicyInternalServer(t, nil)
	unconfigured.webPolicy = nil
	if response := serveWebPolicyInternal(unconfigured, http.MethodGet, webPolicyInternal, "auth-core", webPolicyAuthToken); response.Code != http.StatusServiceUnavailable {
		t.Fatalf("no service configured: status=%d", response.Code)
	}
}

func TestRetentionPostureStillOnlyTwoBooleans(t *testing.T) {
	reader := &fakeRetentionPostureReader{posture: &orgcore.RetentionPosture{ZDRIntent: true, Entitled: true}}
	t.Setenv(serviceCredentialEnv, webPolicyRegistry())
	server := NewServer(0, nil, nil, "", "")
	server.retentionPosture = reader
	server.webPolicy = newMemoryWebPolicyService("enterprise")

	response := serveWebPolicyInternal(server, http.MethodGet, "/internal/orgs/"+webPolicyOrg+"/retention-posture", "auth-core", webPolicyAuthToken)
	if response.Code != http.StatusOK {
		t.Fatalf("retention posture: status=%d body=%s", response.Code, response.Body.String())
	}
	body := decodeBody(t, response)
	if len(body) != 5 || body["zdrIntent"] != true || body["entitled"] != true || body["effectiveZdr"] != true || body["lapsed"] != false || body["planLapsePolicy"] != "intent_stays_zdr" {
		t.Fatalf("the retention posture answer must stay exactly {zdrIntent, entitled, effectiveZdr, lapsed, planLapsePolicy}, got %v", body)
	}
	if _, wrapped := body["data"]; wrapped {
		t.Fatal("the retention posture contract was wrapped in an envelope")
	}
}

func TestWebPolicyReadScopeIsPinnedToAuthCore(t *testing.T) {
	const token = "fedcba9876543210fedcba9876543210"
	for _, principal := range []string{"quarry-edge", "billing-core", "audit-core", "browser-broker", "session-core", "model-gateway"} {
		if _, err := parseServiceCredentials(`[{"principal":"` + principal + `","audience":"org-core","token":"` + token + `","scopes":["org:web-policy:read:any"]}]`); err == nil {
			t.Errorf("principal %s was accepted with org:web-policy:read:any; only auth-core may hold it", principal)
		}
	}
	if _, err := parseServiceCredentials(`[{"principal":"auth-core","audience":"org-core","token":"` + token + `","scopes":["org:web-policy:read:any"]}]`); err != nil {
		t.Fatalf("auth-core refused its own scope: %v", err)
	}
}

func TestWebPolicyScopeMapping(t *testing.T) {
	cases := []struct {
		method, path string
		want         []string
	}{
		{http.MethodGet, "/orgs/org-1/web-policy", []string{"org:read:self"}},
		{http.MethodPut, "/orgs/org-1/web-policy", []string{"org:settings:write:self"}},
		{http.MethodGet, "/orgs/org-1/web-policy/history", []string{"org:read:self"}},
		{http.MethodGet, "/api/v1/organizations/org-1/web-policy", []string{"org:read:self"}},
		{http.MethodPut, "/api/v1/organizations/org-1/web-policy", []string{"org:settings:write:self"}},
		{http.MethodGet, "/api/v1/organizations/org-1/web-policy/history", []string{"org:read:self"}},
		{http.MethodGet, "/internal/orgs/org-1/web-policy", []string{"org:web-policy:read:any"}},
		// No scope at all: unmapped methods and paths are refused by the middleware.
		{http.MethodPost, "/orgs/org-1/web-policy", nil},
		{http.MethodDelete, "/orgs/org-1/web-policy", nil},
		{http.MethodPatch, "/orgs/org-1/web-policy", nil},
		{http.MethodPut, "/orgs/org-1/web-policy/history", nil},
		{http.MethodGet, "/orgs/org-1/web-policy/other", nil},
		{http.MethodPut, "/internal/orgs/org-1/web-policy", nil},
		{http.MethodPost, "/internal/orgs/org-1/web-policy", nil},
		{http.MethodGet, "/internal/orgs/org-1/web-policy/history", nil},
	}
	for _, tc := range cases {
		request := httptest.NewRequest(tc.method, tc.path, nil)
		got := serviceScopesForRequest(request)
		if strings.Join(got, ",") != strings.Join(tc.want, ",") {
			t.Errorf("%s %s scopes=%v want %v", tc.method, tc.path, got, tc.want)
		}
	}
}

func TestOrgReadAnyHoldersCannotReadTheStoredPolicyThroughTheProxyRoute(t *testing.T) {
	server := webPolicyInternalServer(t, newMemoryWebPolicyService("enterprise"))
	for _, path := range []string{webPolicyProxyPath, webPolicyV1Path, webPolicyProxyPath + "/history"} {
		response := serveWebPolicyInternal(server, http.MethodGet, path, "billing-core", webPolicyBillToken)
		if response.Code != http.StatusForbidden {
			t.Fatalf("org:read:any holder read %s: status=%d, want 403", path, response.Code)
		}
	}
}
