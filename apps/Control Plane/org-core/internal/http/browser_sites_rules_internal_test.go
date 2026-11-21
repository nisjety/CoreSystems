package http

import (
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/browsersites"
	"github.com/gin-gonic/gin"
)

const (
	browserSiteRulesTestToken = "mmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmmm"
	browserSitesRulesPath     = "/internal/orgs/" + modelPolicyOrg + "/browser-sites/rules"
)

// browserSiteRulesRegistry holds the gateway (user scopes only) and model-gateway
// (the one read scope the rules route is pinned to).
func browserSiteRulesRegistry() string {
	return fmt.Sprintf(`[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:settings:write:self"]},
  {"principal":"model-gateway","audience":"org-core","token":%q,"scopes":["org:browser-site:read:any"]}
]`, modelPolicyGatewayToken, browserSiteRulesTestToken)
}

// newBrowserSiteRulesHarness mounts the rules route exactly as server.go does, behind
// the REAL service-auth middleware, on the real browsersites service over the memory store.
func newBrowserSiteRulesHarness(t *testing.T, configured bool) (*gin.Engine, *memoryBrowserSiteStore) {
	t.Helper()
	credentials, err := parseServiceCredentials(browserSiteRulesRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	store := &memoryBrowserSiteStore{}
	server := &Server{callerRoles: fakeCallerRoles{roles: map[string]string{}}}
	if configured {
		server.SetBrowserSites(browsersites.NewService(store, time.Now))
	}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, time.Now))
	router.GET("/internal/orgs/:orgId/browser-sites/rules", server.getBrowserSiteRulesInternal)
	return router, store
}

func rulesRequest(router *gin.Engine, path, principal, token string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodGet, path, nil)
	request.Header.Set("X-Service-Id", principal)
	request.Header.Set("X-Service-Token", token)
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	return response
}

func TestRulesRouteIsMountedOnTheRealRouter(t *testing.T) {
	server := NewServer(0, nil, nil, "", "")
	mounted := false
	for _, route := range server.router.Routes() {
		if route.Method == http.MethodGet && route.Path == "/internal/orgs/:orgId/browser-sites/rules" {
			mounted = true
		}
	}
	if !mounted {
		t.Fatal("GET /internal/orgs/:orgId/browser-sites/rules is not mounted")
	}
}

func TestRulesRouteScopeIsTheBrowserSiteReadScope(t *testing.T) {
	request := httptest.NewRequest(http.MethodGet, browserSitesRulesPath, nil)
	scopes := serviceScopesForRequest(request)
	if len(scopes) != 1 || scopes[0] != "org:browser-site:read:any" {
		t.Fatalf("rules route scopes %v, want [org:browser-site:read:any]", scopes)
	}
	// Only a GET maps a scope; every other method on the path stays unmapped.
	post := httptest.NewRequest(http.MethodPost, browserSitesRulesPath, nil)
	if scopes := serviceScopesForRequest(post); len(scopes) != 0 {
		t.Fatalf("POST on the rules route must map no scope, got %v", scopes)
	}
}

func TestModelGatewayReadsTheOrgRulesAndNothingElse(t *testing.T) {
	router, store := newBrowserSiteRulesHarness(t, true)
	store.policy = browsersites.PolicySet{
		Revision:  3,
		Mode:      browsersites.ModeAllowlist,
		UpdatedBy: "u-admin",
		Rules: []browsersites.Rule{
			{Host: "docs.example.com", Rule: browsersites.RuleRead, Note: "internal remark"},
			{Host: "bad.example.com", Rule: browsersites.RuleBlock},
		},
	}
	response := rulesRequest(router, browserSitesRulesPath, "model-gateway", browserSiteRulesTestToken)
	if response.Code != http.StatusOK {
		t.Fatalf("model-gateway rules read: status=%d body=%s", response.Code, response.Body.String())
	}
	if got := response.Header().Get("Cache-Control"); got != "no-store" {
		t.Fatalf("Cache-Control = %q, want no-store", got)
	}
	body := decodeBody(t, response)
	if body["organization_id"] != modelPolicyOrg || body["mode"] != "allowlist" || body["revision"] != float64(3) {
		t.Fatalf("unexpected answer: %s", response.Body.String())
	}
	rules, _ := body["rules"].([]any)
	if len(rules) != 2 {
		t.Fatalf("want 2 rules, got %s", response.Body.String())
	}
	for _, item := range rules {
		rule := item.(map[string]any)
		if len(rule) != 2 || rule["host"] == nil || rule["rule"] == nil {
			t.Fatalf("a rule must carry host and rule only: %v", rule)
		}
	}
	for _, leaked := range []string{"internal remark", "u-admin", "updated_by", "updated_at", "note"} {
		if strings.Contains(strings.ToLower(response.Body.String()), leaked) {
			t.Fatalf("the rules answer must not carry %q: %s", leaked, response.Body.String())
		}
	}
}

func TestAnOrgWithNoPolicyAnswersOpenNotNotFound(t *testing.T) {
	router, _ := newBrowserSiteRulesHarness(t, true)
	response := rulesRequest(router, "/internal/orgs/org-never-wrote/browser-sites/rules", "model-gateway", browserSiteRulesTestToken)
	if response.Code != http.StatusOK {
		t.Fatalf("an org with no policy must answer 200, got status=%d body=%s", response.Code, response.Body.String())
	}
	body := decodeBody(t, response)
	if body["mode"] != "open" || body["revision"] != float64(0) || body["organization_id"] != "org-never-wrote" {
		t.Fatalf("an org with no policy must read open at revision 0: %s", response.Body.String())
	}
	rules, ok := body["rules"].([]any)
	if !ok || len(rules) != 0 {
		t.Fatalf("an org with no policy must carry an empty rules array, not null: %s", response.Body.String())
	}
}

func TestBrowserSiteRulesAnswerShapesANilSetAsOpen(t *testing.T) {
	got := browserSiteRulesAnswer("org-x", nil)
	if got.Mode != browsersites.ModeOpen || got.Revision != 0 || got.OrganizationID != "org-x" || got.Rules == nil || len(got.Rules) != 0 {
		t.Fatalf("a nil set must read open with an empty rule list: %+v", got)
	}
	empty := browserSiteRulesAnswer("org-y", &browsersites.PolicySet{Revision: 2})
	if empty.Mode != browsersites.ModeOpen || empty.Revision != 2 {
		t.Fatalf("an empty mode must read open: %+v", empty)
	}
}

func TestAnotherPrincipalIsRefusedTheRulesRead(t *testing.T) {
	router, _ := newBrowserSiteRulesHarness(t, true)
	// The gateway holds only :self scopes, so the :any rules read is forbidden.
	if response := rulesRequest(router, browserSitesRulesPath, gatewayServicePrincipal, modelPolicyGatewayToken); response.Code != http.StatusForbidden {
		t.Fatalf("the gateway must not read the rules: status=%d", response.Code)
	}
	// A wrong token for model-gateway is refused before the handler runs.
	if response := rulesRequest(router, browserSitesRulesPath, "model-gateway", "wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww"); response.Code != http.StatusUnauthorized {
		t.Fatalf("a wrong token must be 401: status=%d", response.Code)
	}
	// An unknown principal holds nothing.
	if response := rulesRequest(router, browserSitesRulesPath, "audit-core", browserSiteRulesTestToken); response.Code != http.StatusUnauthorized {
		t.Fatalf("an unregistered principal must be 401: status=%d", response.Code)
	}
}

func TestUnconfiguredRulesServiceAnswers503NoStore(t *testing.T) {
	router, _ := newBrowserSiteRulesHarness(t, false)
	response := rulesRequest(router, browserSitesRulesPath, "model-gateway", browserSiteRulesTestToken)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("no service configured must answer 503: status=%d", response.Code)
	}
	if got := response.Header().Get("Cache-Control"); got != "no-store" {
		t.Fatalf("Cache-Control = %q, want no-store on an error too", got)
	}
}

func TestComposeModelGatewayRulesReadIsAdmittedByTheRealMiddleware(t *testing.T) {
	server, tokens := contractServer(t)
	request := httptest.NewRequest(http.MethodGet, browserSitesRulesPath, nil)
	request.Header.Set("X-Service-Id", "model-gateway")
	request.Header.Set("X-Service-Token", tokens["model-gateway"])
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, request)
	// The handler is unconfigured in this server, so 503 proves the credential and the
	// scope were admitted (a refusal would be 401 or 403).
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("the compose model-gateway must be admitted to the rules route: status=%d body=%s", response.Code, response.Body.String())
	}
}
